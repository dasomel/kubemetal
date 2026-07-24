#!/usr/bin/env python3
"""
03_kagent_fault_injection.py
KubeMetal E2E Verification Step 3: 고의 장애 파드 주입 후 **클러스터가 실제로 보고하는**
상태·이벤트를 읽어 출력한다.

진단 문장을 코드에 미리 적어 두고 출력하지 않는다 — 아래 reason/message는 전부 kubectl이
돌려준 값이다. 클러스터에 접속할 수 없으면 실패한다. kagent 에이전트의 LLM 진단 품질
자체는 kagent UI / A2A 호출로 확인한다(이 스크립트 범위 밖).
"""

import json
import subprocess
import sys
import time

CONTEXT = "colima"
DEPLOY_NAME = "e2e-broken-nginx"
BROKEN_IMAGE = "nginx:non-existent-tag-123456"

BROKEN_DEPLOYMENT_YAML = f"""
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {DEPLOY_NAME}
  namespace: default
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
        image: {BROKEN_IMAGE}
        ports:
        - containerPort: 80
"""


def kubectl(args, stdin=None):
    return subprocess.run(
        ["kubectl", "--context", CONTEXT, *args],
        input=stdin,
        text=True,
        capture_output=True,
    )


def main() -> int:
    print("=== [E2E Step 3] 장애 주입 및 클러스터 실측 상태 수집 ===")

    res = kubectl(["apply", "-f", "-"], stdin=BROKEN_DEPLOYMENT_YAML)
    if res.returncode != 0:
        print(f"[FAIL] 장애 파드 주입 실패(클러스터 미접속): {res.stderr.strip()}", file=sys.stderr)
        return 1
    print(f" -> 주입: {res.stdout.strip()}")

    # ImagePullBackOff는 즉시 나타나지 않는다 — 최대 60초까지 실제 상태를 폴링한다.
    reason = None
    message = ""
    for _ in range(20):
        time.sleep(3)
        res = kubectl(["get", "pods", "-n", "default", "-l", f"app={DEPLOY_NAME}", "-o", "json"])
        if res.returncode != 0:
            print(f"[FAIL] 파드 조회 실패: {res.stderr.strip()}", file=sys.stderr)
            return 1
        for pod in json.loads(res.stdout).get("items", []):
            for cs in pod.get("status", {}).get("containerStatuses", []):
                waiting = cs.get("state", {}).get("waiting")
                if waiting and waiting.get("reason") not in (None, "ContainerCreating"):
                    reason = waiting["reason"]
                    message = waiting.get("message", "")
        if reason:
            break

    if not reason:
        print("[FAIL] 60초 내 컨테이너 대기 사유가 관측되지 않았습니다.", file=sys.stderr)
        return 1

    print(f" -> 관측된 waiting.reason : {reason}")
    print(f" -> 관측된 waiting.message: {message.strip() or '(없음)'}")

    events = kubectl(
        [
            "get", "events", "-n", "default",
            "--sort-by", ".lastTimestamp",
            "-o", "custom-columns=REASON:.reason,MESSAGE:.message",
        ]
    )
    if events.returncode == 0 and events.stdout.strip():
        print(" -> 최근 이벤트(말미 5줄):")
        for line in events.stdout.strip().splitlines()[-5:]:
            print(f"    {line}")

    if "ImagePull" not in reason and "ErrImage" not in reason:
        print(f"[FAIL] 기대한 이미지 풀 실패가 아닌 사유가 관측됨: {reason}", file=sys.stderr)
        return 1

    print(f"[OK] 장애가 클러스터에서 {reason}로 실제 관측됨")
    return 0


if __name__ == "__main__":
    sys.exit(main())
