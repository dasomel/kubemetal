#!/usr/bin/env python3
"""
02_mlx_finetune_eval.py
KubeMetal E2E Verification Step 2: MLOps 등록 경로(MLflow · SeaweedFS S3) 실도달 검증.

**이 단계가 검증하는 것**: Step 1 데이터셋 존재 + MLflow REST API(:5001) 응답 +
SeaweedFS S3(:8333) 응답 — 즉 학습 결과를 등록할 경로가 실제로 살아 있는지.

**이 단계가 검증하지 않는 것**: 실제 LoRA 학습과 메트릭. MLX 파인튜닝은 호스트
프로세스로 수 분~수십 분이 걸리므로 이 스위트에 포함하지 않는다. 앱의 MLX 스튜디오에서
실행하고 MLflow UI에서 확인한다. 더미 어댑터를 만들어 학습이 된 것처럼 보고하지 않는다.
"""

import json
import os
import sys
import urllib.error
import urllib.request

DATASET_PATH = os.path.expanduser("~/.kubemetal/datasets/e2e_synthetic.jsonl")
MLFLOW_URL = os.getenv("MLFLOW_URL", "http://127.0.0.1:5001")
S3_URL = os.getenv("SEAWEEDFS_S3_URL", "http://127.0.0.1:8333")


def http_status(url: str, timeout: int = 5) -> int:
    """도달 가능하면 HTTP 상태 코드, 아니면 0."""
    try:
        with urllib.request.urlopen(url, timeout=timeout) as resp:
            return resp.status
    except urllib.error.HTTPError as exc:
        return exc.code  # 4xx/5xx도 "서버는 응답했다"는 뜻이므로 도달로 본다
    except (urllib.error.URLError, OSError):
        return 0


def main() -> int:
    print("=== [E2E Step 2] MLOps 등록 경로 도달성 검증 ===")
    failures = []

    if not os.path.exists(DATASET_PATH):
        print(f"[FAIL] Step 1 데이터셋 없음: {DATASET_PATH}", file=sys.stderr)
        return 1

    with open(DATASET_PATH, encoding="utf-8") as f:
        lines = [line for line in f if line.strip()]
    for line in lines:
        json.loads(line)  # 형식이 깨졌으면 여기서 예외로 실패한다
    print(f"[OK] 데이터셋 {len(lines)}건 파싱 성공: {DATASET_PATH}")

    experiments_url = f"{MLFLOW_URL}/api/2.0/mlflow/experiments/search?max_results=1"
    status = http_status(experiments_url)
    if status == 0:
        failures.append(f"MLflow 미도달: {MLFLOW_URL} (make forward 로 포트포워딩 확인)")
    else:
        print(f"[OK] MLflow REST 응답: {MLFLOW_URL} -> HTTP {status}")

    status = http_status(S3_URL)
    if status == 0:
        failures.append(f"SeaweedFS S3 미도달: {S3_URL} (make forward 로 포트포워딩 확인)")
    else:
        print(f"[OK] SeaweedFS S3 응답: {S3_URL} -> HTTP {status}")

    if failures:
        for item in failures:
            print(f"[FAIL] {item}", file=sys.stderr)
        return 1

    print("[OK] 등록 경로 도달성 확인 완료 (실제 학습/등록은 MLX 스튜디오에서 수행)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
