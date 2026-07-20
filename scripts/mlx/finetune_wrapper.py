#!/usr/bin/env python3
"""KubeMetal MLX LoRA 파인튜닝 래퍼.

`python -m mlx_lm lora --train ...`를 서브프로세스로 실행하며 stdout의
"Iter N: Train loss X, ..." 라인(mlx-lm 0.31.3 실측 포맷 — mlx-community/
Qwen2.5-0.5B-Instruct-4bit 스모크 학습으로 확인)을 파싱해 JSON 라인으로
재출력하고(러스트가 stdout을 읽는다), MLflow REST API(urllib, mlflow 패키지
미설치 가정)로 experiment/run/메트릭을 기록한다. 표준 라이브러리만 사용한다.

MLflow 접근 실패는 학습을 막지 않고 "warning" 이벤트만 내보낸다.
"""
import argparse
import json
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Optional

# mlx-lm 0.31.3 lora 실행 시 --steps-per-report 1 기준 stdout 실측 라인:
#   "Iter 1: Train loss 4.650, Learning Rate 1.000e-05, It/sec 12.481, ..."
ITER_RE = re.compile(r"^Iter (\d+): Train loss ([0-9.eE+\-]+)")

EXPERIMENT_NAME = "kubemetal-finetune"


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description="KubeMetal MLX LoRA 파인튜닝 래퍼")
    p.add_argument("--model", required=True)
    p.add_argument("--data", required=True)
    p.add_argument("--iters", type=int, required=True)
    p.add_argument("--batch-size", type=int, required=True)
    p.add_argument("--learning-rate", type=float, required=True)
    p.add_argument("--adapter-name", required=True)
    p.add_argument("--mlflow-uri", default="http://localhost:5001")
    return p.parse_args()


def emit(event: dict) -> None:
    """러스트가 읽는 표준 출력 채널로 JSON 한 줄을 내보낸다."""
    print(json.dumps(event), flush=True)


class MlflowReporter:
    """MLflow REST(urllib) 클라이언트. 접근 실패는 warning 이벤트만 내고 계속한다."""

    def __init__(self, base_uri: str):
        self.base_uri = base_uri.rstrip("/")
        self.run_id: Optional[str] = None
        self.enabled = True

    def _request(self, method: str, path: str, body: Optional[dict] = None,
                 query: Optional[str] = None):
        url = f"{self.base_uri}{path}"
        if query:
            url = f"{url}?{query}"
        data = json.dumps(body).encode("utf-8") if body is not None else None
        req = urllib.request.Request(
            url, data=data, headers={"Content-Type": "application/json"}, method=method
        )
        with urllib.request.urlopen(req, timeout=5) as resp:
            return json.loads(resp.read().decode("utf-8"))

    def _warn(self, message: str) -> None:
        emit({"type": "warning", "message": f"MLflow: {message}"})

    def get_or_create_experiment(self) -> Optional[str]:
        try:
            resp = self._request(
                "GET",
                "/api/2.0/mlflow/experiments/get-by-name",
                query=f"experiment_name={urllib.parse.quote(EXPERIMENT_NAME)}",
            )
            return resp["experiment"]["experiment_id"]
        except urllib.error.HTTPError as e:
            if e.code != 404:
                self._warn(f"experiment 조회 실패({e})")
                self.enabled = False
                return None
        except Exception as e:  # noqa: BLE001 - 외부 서비스 실패는 학습을 막지 않는다
            self._warn(f"experiment 조회 실패({e})")
            self.enabled = False
            return None

        try:
            resp = self._request(
                "POST", "/api/2.0/mlflow/experiments/create", {"name": EXPERIMENT_NAME}
            )
            return resp["experiment_id"]
        except Exception as e:  # noqa: BLE001
            self._warn(f"experiment 생성 실패({e})")
            self.enabled = False
            return None

    def start_run(self, experiment_id: Optional[str], params: dict) -> None:
        if not self.enabled or experiment_id is None:
            return
        try:
            resp = self._request(
                "POST",
                "/api/2.0/mlflow/runs/create",
                {"experiment_id": experiment_id, "start_time": int(time.time() * 1000)},
            )
            self.run_id = resp["run"]["info"]["run_id"]
            self._request(
                "POST",
                "/api/2.0/mlflow/runs/log-batch",
                {
                    "run_id": self.run_id,
                    "params": [{"key": k, "value": str(v)} for k, v in params.items()],
                },
            )
        except Exception as e:  # noqa: BLE001
            self._warn(f"run 생성 실패({e})")
            self.enabled = False

    def log_metric(self, iter_num: int, train_loss: float) -> None:
        if not self.enabled or self.run_id is None:
            return
        try:
            self._request(
                "POST",
                "/api/2.0/mlflow/runs/log-batch",
                {
                    "run_id": self.run_id,
                    "metrics": [
                        {
                            "key": "train_loss",
                            "value": train_loss,
                            "timestamp": int(time.time() * 1000),
                            "step": iter_num,
                        }
                    ],
                },
            )
        except Exception as e:  # noqa: BLE001
            self._warn(f"메트릭 기록 실패({e})")

    def end_run(self, status: str) -> None:
        if not self.enabled or self.run_id is None:
            return
        try:
            self._request(
                "POST",
                "/api/2.0/mlflow/runs/update",
                {"run_id": self.run_id, "status": status, "end_time": int(time.time() * 1000)},
            )
        except Exception as e:  # noqa: BLE001
            self._warn(f"run 종료 실패({e})")


def main() -> int:
    args = parse_args()

    adapter_path = Path.home() / ".kubemetal" / "adapters" / args.adapter_name
    adapter_path.mkdir(parents=True, exist_ok=True)

    reporter = MlflowReporter(args.mlflow_uri)
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
        },
    )

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
        line = line.rstrip("\n")
        if not line:
            continue
        match = ITER_RE.match(line)
        if match:
            iter_num = int(match.group(1))
            train_loss = float(match.group(2))
            last_loss = train_loss
            emit({"type": "progress", "iter": iter_num, "train_loss": train_loss})
            reporter.log_metric(iter_num, train_loss)

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
