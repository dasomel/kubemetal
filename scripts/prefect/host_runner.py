#!/usr/bin/env python3
"""KubeMetal Prefect 호스트 러너.

Rust(`start_prefect_runner`)가 앱 venv의 python으로 `process_group(0)`(D17과 동일 패턴)
기동한다. `PREFECT_API_URL`(기본 http://127.0.0.1:4200/api, 파드 포워딩 전제)에 evaluate/
ingest 두 deployment를 `flow.to_deployment()` + `serve()`로 등록하고 폴링 루프를 시작한다.
파인튜닝은 이 러너에서 실행하지 않는다(D47) — Rust가 스테이징·검증·승격을 소유하는
`run_mlx_finetune` 경로만 어댑터를 만든다.

실물 확인(2026-07-23, prefect 3.7.8): `Flow.to_deployment(name, ...) -> RunnerDeployment`,
`serve(*deployments, ...)`가 실제 시그니처(venv에 설치한 prefect 패키지의
`inspect.signature`로 확인) — 두 deployment를 함께 `serve()`에 넘기면 하나의 프로세스에서
모두 폴링·실행된다.

`evaluate_flow`(Phase 4b, docs/05-mlops-research.md Q2)는 별도 서브프로세스 래퍼 없이
이 프로세스 안에서 직접 `lm_eval` CLI를 서브프로세스로 실행하고 결과를 파싱한다 — 한 번
실행하고 끝나는 평가 배치이므로 D17의 프로세스 그룹 전파가 불필요하다. MLflow REST
기록은 `scripts/mlx/mlflow_reporter.py`의 `MlflowReporter`를 재사용한다(experiment
"kubemetal-eval").
"""
import glob
import json
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path
from typing import Optional

from prefect import flow, get_run_logger, serve

MLX_SCRIPTS_DIR = Path(__file__).resolve().parent.parent / "mlx"

# scripts/mlx의 mlflow_reporter.py를 임포트하기 위해 sys.path에 추가한다 —
# 패키지화하지 않고 두 스크립트 디렉터리 모두 Tauri 리소스로
# 그대로 번들되므로(tauri.conf.json `resources: ["../scripts/mlx/*", ...]`), 상대 경로
# 삽입만으로 dev/패키지 빌드 양쪽에서 동작한다.
sys.path.insert(0, str(MLX_SCRIPTS_DIR))
from mlflow_reporter import MlflowReporter  # noqa: E402 - sys.path 삽입 후 임포트해야 함

EVAL_EXPERIMENT_NAME = "kubemetal-eval"
# 포트는 런타임 값이다(D1 개정) — MLflow가 5001을 못 잡으면 대체 포트로 밀린다. 그래서
# 앱이 MLFLOW_TRACKING_URI로 실제 주소를 주입하고(PREFECT_API_URL과 같은 규약), 여기서는
# 단독 실행용 폴백만 갖는다. 폴백도 `localhost`가 아니라 `127.0.0.1`이다(D1) — macOS에서
# localhost는 ::1로도 풀려 와일드카드 리스너와 만난다(mistakes-log 2026-07-21).
DEFAULT_MLFLOW_URI = os.environ.get("MLFLOW_TRACKING_URI", "http://127.0.0.1:5001")

# lm-eval 0.4.12 결과 JSON 실측(2026-07-23): results[task]에 메트릭("exact_match,strict-
# match" 형태의 "metric,filter" 키 → float)과 메타데이터(alias/name: 문자열, sample_len:
# 표본 수 정수)가 섞여 있다. 메타데이터 키는 메트릭으로 취급하지 않는다.
NON_METRIC_KEYS = {"alias", "name", "sample_len", "samples"}


def _fetch_serving_model_id(serving_url: str) -> str:
    """`serving_url`(예: http://127.0.0.1:8081/v1)의 `/models`에서 mlx_lm.server가 보고하는
    실제 모델 id(로컬 경로 문자열)를 조회한다. 조회 실패 시 `serving_url` 자체를 폴백으로
    쓴다 — lm_eval에서 `model=` 값은 요청 로깅/식별용일 뿐 실제 추론 대상은 `base_url`이
    결정하므로, 조회 실패가 평가 자체를 막지 않는다."""
    try:
        with urllib.request.urlopen(f"{serving_url.rstrip('/')}/models", timeout=5) as resp:
            data = json.loads(resp.read().decode("utf-8"))
        return data["data"][0]["id"]
    except Exception:  # noqa: BLE001 - 폴백으로 계속 진행
        return serving_url


def _flatten_metrics(results: dict) -> dict:
    """`{task: {"exact_match,strict-match": 0.5, ...}}` 형태를 MLflow 메트릭 키
    `"task/metric/filter"`로 평탄화한다. MLflow 메트릭 키는 콤마를 포함할 수 없어
    lm-eval의 "metric,filter" 키 구분자를 슬래시로 치환한다."""
    flat: dict = {}
    for task, task_results in results.items():
        for key, value in task_results.items():
            if key in NON_METRIC_KEYS or not isinstance(value, (int, float)):
                continue
            flat[f"{task}/{key.replace(',', '/')}"] = float(value)
    return flat


@flow(name="evaluate", log_prints=True)
def evaluate_flow(
    serving_url: str = "http://127.0.0.1:8081/v1",
    tasks: str = "gsm8k",
    limit: int = 8,
) -> dict:
    """venv `lm_eval` CLI(local-completions, 실기기 실측 2026-07-23: mlx_lm.server가
    `/v1/completions`를 지원해 무수정 연결 가능 — docs/05-mlops-research.md Q2)를
    서브프로세스로 실행하고, 결과 JSON을 파싱해 태스크별 메트릭을 Prefect 로그와 MLflow
    experiment "kubemetal-eval"(D20)에 run으로 기록한다. lm_eval 비정상 종료 시 예외를
    던져 flow run을 FAILED로 만든다(ingest_flow와 동일 원칙)."""
    logger = get_run_logger()
    if limit <= 0:
        raise ValueError("limit은 1 이상이어야 합니다.")

    completions_url = f"{serving_url.rstrip('/')}/completions"
    model_id = _fetch_serving_model_id(serving_url)
    logger.info(
        f"평가 시작: endpoint={completions_url} model={model_id} tasks={tasks} limit={limit}"
    )

    output_dir = tempfile.mkdtemp(prefix="kubemetal-eval-")
    model_args = (
        f"base_url={completions_url},model={model_id},"
        "num_concurrent=1,tokenized_requests=False"
    )
    cmd = [
        sys.executable,
        "-u",
        "-m",
        "lm_eval",
        "run",
        "--model", "local-completions",
        "--tasks", tasks,
        "--model_args", model_args,
        "--limit", str(limit),
        "--output_path", output_dir,
    ]
    logger.info(f"lm_eval 실행: {' '.join(cmd)}")

    try:
        proc = subprocess.run(cmd, capture_output=True, text=True)

        for line in proc.stdout.splitlines():
            if line.strip():
                logger.info(line)
        # lm-eval 0.4.12 실측: 진행률/정보 로그 대부분이 stderr로 나간다(tqdm 등) — 실패가
        # 아니면 stderr도 info로 중계한다.
        for line in proc.stderr.splitlines():
            if line.strip():
                logger.info(line)

        if proc.returncode != 0:
            message = proc.stderr.strip()[-4000:] or f"lm_eval exited with {proc.returncode}"
            raise RuntimeError(message)

        result_files = sorted(glob.glob(f"{output_dir}/**/results_*.json", recursive=True))
        if not result_files:
            raise RuntimeError(f"lm_eval 결과 파일을 찾을 수 없습니다: {output_dir}")
        with open(result_files[-1]) as f:
            payload = json.load(f)
    finally:
        shutil.rmtree(output_dir, ignore_errors=True)

    metrics = _flatten_metrics(payload.get("results", {}))

    reporter = MlflowReporter(DEFAULT_MLFLOW_URI, EVAL_EXPERIMENT_NAME, warn=logger.warning)
    experiment_id = reporter.get_or_create_experiment()
    reporter.start_run(
        experiment_id,
        {"serving_url": serving_url, "model": model_id, "tasks": tasks, "limit": limit},
    )
    reporter.log_metrics(metrics)
    reporter.end_run("FINISHED")

    for key, value in metrics.items():
        logger.info(f"{key} = {value}")

    return {"run_id": reporter.run_id, "metrics": metrics}


INGEST_SCRIPT_PATH = Path(__file__).resolve().parent.parent / "data" / "ingest_host.py"


@flow(name="ingest", log_prints=True)
def ingest_flow(
    source_type: str = "local",
    source_path: str = "docs",
    collection: str = "dataset_ingest",
    chunk_size: int = 500,
    chunk_overlap: int = 50,
    dvc_backup: bool = False,
) -> dict:
    """`ingest_host.py`를 실행하여 Web/HF/Local 데이터 수집, 청킹, LanceDB 인덱싱 및 DVC 커밋 DAG를 수행한다."""
    logger = get_run_logger()
    if not INGEST_SCRIPT_PATH.is_file():
        raise RuntimeError(f"ingest_host.py를 찾을 수 없습니다: {INGEST_SCRIPT_PATH}")

    cmd = [
        sys.executable,
        "-u",
        str(INGEST_SCRIPT_PATH),
        "--source-type", source_type,
        "--source-path", source_path,
        "--collection", collection,
        "--chunk-size", str(chunk_size),
        "--chunk-overlap", str(chunk_overlap),
    ]
    if dvc_backup:
        cmd.append("--dvc-backup")

    logger.info(f"ingest_host 시작: {' '.join(cmd)}")
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0:
        message = proc.stderr.strip() or proc.stdout.strip() or f"ingest_host exited with {proc.returncode}"
        raise RuntimeError(message)

    try:
        res = json.loads(proc.stdout)
    except json.JSONDecodeError as e:
        raise RuntimeError(f"ingest_host JSON 파싱 실패: {e}\nstdout: {proc.stdout}") from e

    logger.info(f"수집 완료: status={res.get('status')} chunks={res.get('total_chunks_created')}")
    return res


if __name__ == "__main__":
    evaluate_deployment = evaluate_flow.to_deployment(name="evaluate")
    ingest_deployment = ingest_flow.to_deployment(name="ingest")
    serve(evaluate_deployment, ingest_deployment)

