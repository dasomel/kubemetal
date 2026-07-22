#!/usr/bin/env python3
"""KubeMetal Prefect 호스트 러너.

Rust(`start_prefect_runner`)가 앱 venv의 python으로 `process_group(0)`(D17과 동일 패턴)
기동한다. `PREFECT_API_URL`(기본 http://127.0.0.1:4200/api, 파드 포워딩 전제)에 finetune/
evaluate 두 deployment를 `flow.to_deployment()` + `serve()`로 등록하고 폴링 루프를 시작한다.
새 프로세스 그룹의 리더이므로 이 프로세스가 내부에서 `subprocess.Popen`으로 띄우는
`finetune_wrapper.py`(및 그 자식인 `mlx_lm` 학습 프로세스)는 그룹을 상속받아, Rust가
그룹 전체(`-pid`)로 보내는 SIGSTOP/SIGCONT/SIGTERM/SIGKILL이 트리 전체에 전파된다.

실물 확인(2026-07-23, prefect 3.7.8): `Flow.to_deployment(name, ...) -> RunnerDeployment`,
`serve(*deployments, ...)`가 실제 시그니처(venv에 설치한 prefect 패키지의
`inspect.signature`로 확인) — 두 deployment를 함께 `serve()`에 넘기면 하나의 프로세스에서
모두 폴링·실행된다.
"""
import json
import subprocess
import sys
from pathlib import Path
from typing import Optional

from prefect import flow, get_run_logger, serve

WRAPPER_PATH = Path(__file__).resolve().parent.parent / "mlx" / "finetune_wrapper.py"


@flow(name="finetune", log_prints=True)
def finetune_flow(
    model_path: str,
    data_path: str,
    iters: int,
    batch_size: int,
    learning_rate: float,
    adapter_name: str,
) -> dict:
    """`finetune_wrapper.py`(mlx_lm lora 래퍼)를 서브프로세스로 실행하며 stdout JSON
    라인(progress/warning/done/error — finetune_wrapper.py의 `emit()` 포맷과 동일)을
    Prefect 로그로 중계한다. 래퍼가 0이 아닌 코드로 종료하면 예외를 던져 flow run을
    FAILED로 만든다."""
    logger = get_run_logger()
    if not WRAPPER_PATH.is_file():
        raise RuntimeError(f"finetune_wrapper.py를 찾을 수 없습니다: {WRAPPER_PATH}")

    cmd = [
        sys.executable,
        "-u",
        str(WRAPPER_PATH),
        "--model", model_path,
        "--data", data_path,
        "--iters", str(iters),
        "--batch-size", str(batch_size),
        "--learning-rate", str(learning_rate),
        "--adapter-name", adapter_name,
    ]
    logger.info(f"finetune_wrapper 시작: {' '.join(cmd)}")

    proc = subprocess.Popen(
        cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1
    )

    result: dict = {}
    assert proc.stdout is not None
    for line in proc.stdout:
        line = line.strip()
        if not line:
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            logger.info(line)
            continue

        etype = event.get("type")
        if etype == "progress":
            logger.info(f"iter {event.get('iter')}: train_loss={event.get('train_loss')}")
        elif etype == "warning":
            logger.warning(event.get("message", ""))
        elif etype == "done":
            result = event
            logger.info(f"완료: adapter_path={event.get('adapter_path')}")
        elif etype == "error":
            logger.error(event.get("message", ""))

    proc.wait()
    stderr_text = proc.stderr.read() if proc.stderr else ""

    if proc.returncode != 0:
        message = stderr_text.strip()[-4000:] or f"finetune_wrapper exited with {proc.returncode}"
        raise RuntimeError(message)

    return result


@flow(name="evaluate", log_prints=True)
def evaluate_flow() -> None:
    """평가 파이프라인 placeholder — Phase 4b(lm-eval-harness + MLflow GenAI judge,
    docs/05-mlops-research.md Q2)에서 구현 예정. 지금은 deployment 등록만 해둔다."""
    pass


if __name__ == "__main__":
    finetune_deployment = finetune_flow.to_deployment(name="finetune")
    evaluate_deployment = evaluate_flow.to_deployment(name="evaluate")
    serve(finetune_deployment, evaluate_deployment)
