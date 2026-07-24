#!/usr/bin/env python3
"""
01_synthetic_data_gen.py
KubeMetal E2E Verification Step 1: 온톨로지 기반 합성 QA 데이터셋 생성.

호스트 MLX 서빙(OpenAI 호환 엔드포인트)에 실제로 질의해 샘플을 만든다.
서빙이 떠 있지 않으면 **실패한다** — 모의 응답으로 데이터셋을 채우면 이후 단계가
학습할 수 없는 가짜 데이터를 진짜 데이터로 취급하게 된다.

D1: 서빙 기본 포트는 8080이며 URL은 항상 127.0.0.1(localhost는 ::1로 풀려 오판을 부른다).
다른 포트로 서빙 중이면 SERVE_URL 환경변수로 지정한다.
"""

import json
import os
import sys
import urllib.error
import urllib.request

SERVER_URL = os.getenv("SERVE_URL", "http://127.0.0.1:8080/v1/chat/completions")
MODEL = os.getenv("SERVE_MODEL", "default")
OUTPUT_PATH = os.path.expanduser("~/.kubemetal/datasets/e2e_synthetic.jsonl")

ONTOLOGY_CONTEXT = """
KubeMetal 온톨로지 규칙:
1. BaseModel -> Adapter -> RegisteredModel (MLflow/SeaweedFS) -> ServingInstance
2. TrainingDataset -> DatasetVersion (DVC)
3. DocumentCollection -> LanceDB (RAG)
4. FlowRun -> Prefect 3
5. mac-gpu-service -> host.lima.internal (K8s 파드에서 호스트 MLX 서빙 연결 브릿지)
"""

PROMPTS = [
    "KubeMetal에서 MLflow 파드가 종료되었을 때 RegisteredModel과 파이프라인에 미치는 영향은?",
    "seaweedfs 파드 OOM 장애 발생 시 BaseModel, Adapter, DatasetVersion에 발생하는 문제점과 조치 방법은?",
    "mac-gpu-service ExternalName CNAME 역할과 kagent가 호스트 MLX 서빙을 호출하는 구조를 설명해줘.",
]


def generate_sample(prompt: str) -> str:
    payload = {
        "model": MODEL,
        "messages": [
            {"role": "system", "content": f"너는 KubeMetal 도메인 전문가다.\n{ONTOLOGY_CONTEXT}"},
            {"role": "user", "content": prompt},
        ],
        "temperature": 0.3,
    }
    req = urllib.request.Request(
        SERVER_URL,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=120) as resp:
        data = json.loads(resp.read().decode("utf-8"))
    return data["choices"][0]["message"]["content"]


def main() -> int:
    print(f"=== [E2E Step 1] 온톨로지 기반 합성 데이터 생성 ({SERVER_URL}) ===")

    samples = []
    for prompt in PROMPTS:
        print(f" -> 합성 질의: {prompt[:40]}...")
        try:
            content = generate_sample(prompt)
        except (urllib.error.URLError, OSError, KeyError, ValueError) as exc:
            print(f"[FAIL] 서빙 엔드포인트 호출 실패: {exc}", file=sys.stderr)
            print(
                "       MLX 스튜디오에서 서빙을 시작하거나 SERVE_URL을 지정하세요.",
                file=sys.stderr,
            )
            return 1
        samples.append(
            {
                "messages": [
                    {"role": "system", "content": ONTOLOGY_CONTEXT},
                    {"role": "user", "content": prompt},
                    {"role": "assistant", "content": content},
                ]
            }
        )

    os.makedirs(os.path.dirname(OUTPUT_PATH), exist_ok=True)
    with open(OUTPUT_PATH, "w", encoding="utf-8") as f:
        for sample in samples:
            f.write(json.dumps(sample, ensure_ascii=False) + "\n")

    print(f"[OK] {len(samples)}개 합성 QA 샘플 저장: {OUTPUT_PATH}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
