#!/usr/bin/env python3
"""KubeMetal MLX LoRA 파인튜닝 래퍼 (mlx-lm | mlx-vlm, D29).

`python -m mlx_lm lora --train ...` 또는 `python -m mlx_vlm.lora ...`를
서브프로세스로 실행하며 stdout의 "Iter N: Train loss X, ..." 라인을 파싱해
JSON 라인으로 재출력하고(러스트가 stdout을 읽는다), MLflow REST API(urllib,
mlflow 패키지 미설치 가정)로 experiment/run/메트릭을 기록한다. 표준 라이브러리만
사용한다.

두 런타임의 실측 차이(2026-07-27, mlx-lm 0.31.3 / mlx-vlm 0.6.7 스모크 학습):
- 진행 라인 포맷은 같지만 mlx_vlm.lora는 loss 값을 ANSI 색코드로 감싼다
  (`Train loss \x1b[92m4.437...\x1b[0m`) — 매칭 전에 ANSI를 벗겨야 한다.
- 인자 의미가 다르다: mlx_lm에서 `--adapter-path`는 출력이지만 mlx_vlm에서는
  **재개(resume) 입력**이다(빈 디렉터리를 주면 adapter_config.json을 찾다 죽는다).
  mlx_vlm의 출력은 `--output-path`다.
- 데이터: mlx_lm은 train.jsonl 디렉터리, mlx_vlm은 HF load_dataset 경로
  (train.jsonl을 담은 로컬 디렉터리도 빌더 추론으로 동작함을 실측).
- mlx_vlm의 adapter_config.json에는 `model` 키가 없다 — 서빙 시 베이스 모델을
  자동 해석할 수 없으므로 명시적으로 지정해야 한다.

MLflow 접근 실패는 학습을 막지 않고 "warning" 이벤트만 내보낸다. MLflow REST 클라이언트
자체는 `mlflow_reporter.py`(Phase 4b에서 `host_runner.py`의 evaluate_flow와 공유하도록
분리)의 `MlflowReporter`를 재사용한다.
"""
import argparse
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Optional

from mlflow_reporter import MlflowReporter

# 두 런타임 공통 stdout 실측 라인(--steps-per-report 1):
#   mlx-lm : "Iter 1: Train loss 4.650, Learning Rate 1.000e-05, ..."
#   mlx-vlm: "Iter 1: Train loss \x1b[92m4.437...\x1b[0m, Learning Rate ..."
# ANSI를 벗긴 뒤 매칭하므로 정규식은 하나로 충분하다.
ITER_RE = re.compile(r"^Iter (\d+): Train loss ([0-9.eE+\-]+)")
ANSI_RE = re.compile(r"\x1b\[[0-9;]*m")


def strip_ansi(text: str) -> str:
    return ANSI_RE.sub("", text)

EXPERIMENT_NAME = "kubemetal-finetune"


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description="KubeMetal MLX LoRA 파인튜닝 래퍼")
    p.add_argument("--model", required=True)
    p.add_argument("--data", required=True)
    p.add_argument("--iters", type=int, required=True)
    p.add_argument("--batch-size", type=int, required=True)
    p.add_argument("--learning-rate", type=float, required=True)
    p.add_argument("--adapter-name", required=True)
    p.add_argument("--runtime", choices=["mlx-lm", "mlx-vlm"], default="mlx-lm")
    # 기본값은 앱 없이 단독 실행할 때만 쓰인다 — 앱은 실제 배정된 포트를 명시로 넘긴다.
    # `localhost`가 아니라 `127.0.0.1`이다(D1): macOS에서 localhost는 ::1로도 풀려
    # 와일드카드로 바인딩한 남의 프로세스와 만난다(mistakes-log 2026-07-21).
    p.add_argument("--mlflow-uri", default="http://127.0.0.1:5001")
    p.add_argument("--train-vision", action="store_true")
    return p.parse_args()


def emit(event: dict) -> None:
    """러스트가 읽는 표준 출력 채널로 JSON 한 줄을 내보낸다."""
    print(json.dumps(event), flush=True)


def main() -> int:
    args = parse_args()

    if args.runtime == "mlx-lm" and args.train_vision:
        emit({"type": "error", "message": "--train-vision은 mlx-vlm 런타임 전용입니다"})
        return 2

    adapter_path = Path.home() / ".kubemetal" / "adapters" / args.adapter_name
    adapter_path.mkdir(parents=True, exist_ok=True)

    reporter = MlflowReporter(
        args.mlflow_uri,
        EXPERIMENT_NAME,
        warn=lambda message: emit({"type": "warning", "message": message}),
    )
    experiment_id = reporter.get_or_create_experiment()
    reporter.start_run(
        experiment_id,
        {
            "model": args.model,
            "data": args.data,
            "iters": args.iters,
            "batch_size": args.batch_size,
            "learning_rate": args.learning_rate,
            "adapter_name": args.adapter_name,
            "runtime": args.runtime,
            "train_vision": args.train_vision,
        },
    )

    if args.runtime == "mlx-vlm":
        cmd = [
            sys.executable,
            "-u",
            "-m",
            "mlx_vlm.lora",
            "--model-path", args.model,
            "--dataset", args.data,
            "--iters", str(args.iters),
            "--batch-size", str(args.batch_size),
            "--learning-rate", str(args.learning_rate),
            # 주의: mlx_vlm에서 --adapter-path는 resume 입력이다. 출력은 --output-path.
            "--output-path", str(adapter_path),
            "--steps-per-report", "1",
        ]
        if args.train_vision:
            cmd.append("--train-vision")
    else:
        cmd = [
            sys.executable,
            "-u",
            "-m",
            "mlx_lm",
            "lora",
            "--model", args.model,
            "--train",
            "--data", args.data,
            "--iters", str(args.iters),
            "--batch-size", str(args.batch_size),
            "--learning-rate", str(args.learning_rate),
            "--adapter-path", str(adapter_path),
            "--steps-per-report", "1",
        ]

    proc = subprocess.Popen(
        cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1
    )

    last_loss: Optional[float] = None
    assert proc.stdout is not None
    for line in proc.stdout:
        line = strip_ansi(line.rstrip("\n"))
        if not line:
            continue
        match = ITER_RE.match(line)
        if match:
            iter_num = int(match.group(1))
            train_loss = float(match.group(2))
            last_loss = train_loss
            emit({"type": "progress", "iter": iter_num, "train_loss": train_loss})
            reporter.log_metric("train_loss", train_loss, step=iter_num)

    proc.wait()
    stderr_text = proc.stderr.read() if proc.stderr else ""

    if proc.returncode != 0:
        reporter.end_run("FAILED")
        message = stderr_text.strip()[-4000:] or f"mlx_lm lora exited with {proc.returncode}"
        emit({"type": "error", "message": message})
        return proc.returncode

    reporter.end_run("FINISHED")
    emit({"type": "done", "adapter_path": str(adapter_path), "last_loss": last_loss})
    return 0


if __name__ == "__main__":
    sys.exit(main())
