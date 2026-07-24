#!/usr/bin/env python3
"""
04_coding_agent_patch.py
KubeMetal E2E Verification Step 4: Step 3에서 관측된 장애 파드를 정상 이미지로 교정하는
매니페스트를 만들고, **실제로** kubectl dry-run 검증 후 적용해 Ready가 되는지 확인한다.

이 스크립트는 코딩 에이전트가 만들어 낼 패치와 동일한 형태의 매니페스트를 기준선으로
검증할 뿐, 에이전트를 대신 실행하지 않는다. GitOps PR 생성도 하지 않는다 —
하지 않은 일을 성공으로 보고하지 않기 위해 해당 단계는 이 스위트에서 제외했다.
"""

import json
import os
import subprocess
import sys
import time

CONTEXT = "colima"
DEPLOY_NAME = "e2e-broken-nginx"
TARGET_YAML_PATH = os.path.normpath(
    os.path.join(os.path.dirname(__file__), "..", "k8s", "e2e-remediated-nginx.yaml")
)

REMEDIATED_YAML = f"""apiVersion: apps/v1
kind: Deployment
metadata:
  name: {DEPLOY_NAME}
  namespace: default
  labels:
    remediated-by: coding-agent-mcp
spec:
  replicas: 1
  selector:
    matchLabels:
      app: {DEPLOY_NAME}
  template:
    metadata:
      labels:
        app: {DEPLOY_NAME}
    spec:
      containers:
      - name: nginx
        image: nginx:alpine
        ports:
        - containerPort: 80
"""


def kubectl(args):
    return subprocess.run(
        ["kubectl", "--context", CONTEXT, *args], text=True, capture_output=True
    )


def main() -> int:
    print("=== [E2E Step 4] 교정 매니페스트 dry-run 검증 및 적용 ===")

    os.makedirs(os.path.dirname(TARGET_YAML_PATH), exist_ok=True)
    with open(TARGET_YAML_PATH, "w", encoding="utf-8") as f:
        f.write(REMEDIATED_YAML)
    print(f" -> 매니페스트 작성: {TARGET_YAML_PATH}")

    res = kubectl(["apply", "-f", TARGET_YAML_PATH, "--dry-run=client"])
    if res.returncode != 0:
        print(f"[FAIL] dry-run=client 실패: {res.stderr.strip()}", file=sys.stderr)
        return 1
    print(f" -> dry-run=client: {res.stdout.strip()}")

    res = kubectl(["apply", "-f", TARGET_YAML_PATH, "--dry-run=server"])
    if res.returncode != 0:
        print(f"[FAIL] dry-run=server 실패(클러스터 미접속 포함): {res.stderr.strip()}", file=sys.stderr)
        return 1
    print(f" -> dry-run=server: {res.stdout.strip()}")

    res = kubectl(["apply", "-f", TARGET_YAML_PATH])
    if res.returncode != 0:
        print(f"[FAIL] 적용 실패: {res.stderr.strip()}", file=sys.stderr)
        return 1
    print(f" -> 적용: {res.stdout.strip()}")

    # 교정이 실제로 통했는지는 파드가 Ready가 되는지로만 확인한다.
    for _ in range(20):
        time.sleep(3)
        res = kubectl(["get", "pods", "-n", "default", "-l", f"app={DEPLOY_NAME}", "-o", "json"])
        if res.returncode != 0:
            print(f"[FAIL] 파드 조회 실패: {res.stderr.strip()}", file=sys.stderr)
            return 1
        for pod in json.loads(res.stdout).get("items", []):
            conditions = pod.get("status", {}).get("conditions", [])
            if any(c.get("type") == "Ready" and c.get("status") == "True" for c in conditions):
                print(f"[OK] 교정 후 파드 Ready 확인: {pod['metadata']['name']}")
                return 0

    print("[FAIL] 60초 내 교정된 파드가 Ready가 되지 않았습니다.", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
