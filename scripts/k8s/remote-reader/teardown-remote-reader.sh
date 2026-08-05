#!/usr/bin/env bash
# setup-remote-reader.sh가 만든 것을 전부 되돌린다. 외부 클러스터에 아무것도 남기지 않는 것이
# 이 기능의 계약이므로, 되돌리는 경로도 같은 무게로 제공한다.
#
# 사용: ./teardown-remote-reader.sh <REMOTE_CONTEXT> [LOCAL_CONTEXT]
set -euo pipefail

REMOTE_CONTEXT="${1:-}"
LOCAL_CONTEXT="${2:-colima}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KUBECTL_TIMEOUT="--request-timeout=60s"

if [ -z "$REMOTE_CONTEXT" ]; then
  echo "사용법: $0 <REMOTE_CONTEXT> [LOCAL_CONTEXT]" >&2
  exit 1
fi

echo "[1/2] 로컬 [$LOCAL_CONTEXT] 정리"
kubectl --context "$LOCAL_CONTEXT" $KUBECTL_TIMEOUT delete -f "${SCRIPT_DIR}/remote-reader-agent.yaml" --ignore-not-found
kubectl --context "$LOCAL_CONTEXT" $KUBECTL_TIMEOUT delete -f "${SCRIPT_DIR}/remote-reader-tools.yaml" --ignore-not-found
kubectl --context "$LOCAL_CONTEXT" $KUBECTL_TIMEOUT delete secret remote-reader-kubeconfig -n default --ignore-not-found

echo "[2/2] 원격 [$REMOTE_CONTEXT] 정리 (SA·롤·토큰)"
# 네임스페이스는 지우지 않는다 — 그 클러스터에 kagent이 따로 설치돼 있을 수 있고,
# 남의 리소스를 같이 지우는 편이 훨씬 비싼 실수다. 우리가 만든 것만 이름으로 지운다.
kubectl --context "$REMOTE_CONTEXT" $KUBECTL_TIMEOUT delete secret kagent-remote-reader-token -n kagent --ignore-not-found
kubectl --context "$REMOTE_CONTEXT" $KUBECTL_TIMEOUT delete serviceaccount kagent-remote-reader -n kagent --ignore-not-found
kubectl --context "$REMOTE_CONTEXT" $KUBECTL_TIMEOUT delete clusterrolebinding kagent-remote-reader-view --ignore-not-found
kubectl --context "$REMOTE_CONTEXT" $KUBECTL_TIMEOUT delete clusterrolebinding kagent-remote-cluster-reader --ignore-not-found
kubectl --context "$REMOTE_CONTEXT" $KUBECTL_TIMEOUT delete clusterrole kagent-remote-cluster-reader --ignore-not-found

echo ""
echo "남은 것 확인:"
kubectl --context "$REMOTE_CONTEXT" $KUBECTL_TIMEOUT get sa,secret -n kagent 2>&1 | grep -i remote-reader || echo "  원격: remote-reader 리소스 없음"
