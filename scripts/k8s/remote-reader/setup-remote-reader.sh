#!/usr/bin/env bash
# 로컬 kagent가 외부 클러스터를 진단하게 만든다.
#
#   외부 클러스터: 읽기 전용 ServiceAccount 하나만 (워크로드 0개)
#   로컬 클러스터: 그 자격증명을 쓰는 도구 서버 + kagent CR 2개
#
# 사용: ./setup-remote-reader.sh <REMOTE_CONTEXT> [LOCAL_CONTEXT]
#       REMOTE_CONTEXT  진단 대상 외부 클러스터의 kubeconfig 컨텍스트 (필수)
#       LOCAL_CONTEXT   kagent이 도는 클러스터 (기본 colima)
#
# 원칙(D22): 단계마다 실측으로 확인하고 넘어간다. 실패는 그대로 노출하며, 확인하지 못한 것을
# 성공으로 출력하지 않는다. 생성한 kubeconfig는 디스크에 남기지 않는다.
set -euo pipefail

REMOTE_CONTEXT="${1:-}"
LOCAL_CONTEXT="${2:-colima}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KUBECTL_TIMEOUT="--request-timeout=60s"

if [ -z "$REMOTE_CONTEXT" ]; then
  echo "사용법: $0 <REMOTE_CONTEXT> [LOCAL_CONTEXT]" >&2
  echo "  사용 가능한 컨텍스트:" >&2
  kubectl config get-contexts -o name 2>/dev/null | sed 's/^/    /' >&2
  exit 1
fi

if [ "$REMOTE_CONTEXT" = "$LOCAL_CONTEXT" ]; then
  echo "!! REMOTE_CONTEXT와 LOCAL_CONTEXT가 같다($REMOTE_CONTEXT) — 자기 자신을 원격으로 읽을 이유가 없다." >&2
  exit 1
fi

for ctx in "$REMOTE_CONTEXT" "$LOCAL_CONTEXT"; do
  if ! kubectl config get-contexts -o name 2>/dev/null | grep -qx "$ctx"; then
    echo "!! kubeconfig에 컨텍스트 [$ctx]가 없다." >&2
    exit 1
  fi
done

cleanup() { [ -n "${TMP_KUBECONFIG:-}" ] && rm -f "$TMP_KUBECONFIG"; }
trap cleanup EXIT

echo "[1/5] 이미지 드리프트 확인 (로컬 차트의 kagent-tools와 같은 태그여야 한다)"
CHART_IMAGE="$(kubectl --context "$LOCAL_CONTEXT" $KUBECTL_TIMEOUT get deploy kagent-tools -n kagent \
  -o jsonpath='{.spec.template.spec.containers[0].image}' 2>/dev/null || true)"
MANIFEST_IMAGE="$(grep -oE 'image: \S+' "${SCRIPT_DIR}/remote-reader-tools.yaml" | head -1 | awk '{print $2}')"
if [ -z "$CHART_IMAGE" ]; then
  echo "  !! [$LOCAL_CONTEXT]에 kagent-tools 배포가 없다 — kagent을 먼저 설치해야 한다(make kagent-up)." >&2
  exit 1
fi
if [ "$CHART_IMAGE" != "$MANIFEST_IMAGE" ]; then
  echo "  !! 이미지 불일치: 차트=[$CHART_IMAGE] 매니페스트=[$MANIFEST_IMAGE]" >&2
  echo "     remote-reader-tools.yaml의 image를 차트 쪽에 맞춰야 한다." >&2
  exit 1
fi
echo "  -> 일치: $CHART_IMAGE"

echo "[2/5] 원격 클러스터 [$REMOTE_CONTEXT]에 읽기 전용 SA 적용"
kubectl --context "$REMOTE_CONTEXT" $KUBECTL_TIMEOUT apply -f "${SCRIPT_DIR}/remote-reader-rbac.yaml"

echo "[3/5] SA 토큰 발급 대기"
TOKEN=""
for _ in $(seq 1 30); do
  TOKEN="$(kubectl --context "$REMOTE_CONTEXT" $KUBECTL_TIMEOUT get secret kagent-remote-reader-token \
    -n kagent -o jsonpath='{.data.token}' 2>/dev/null || true)"
  [ -n "$TOKEN" ] && break
  sleep 2
done
if [ -z "$TOKEN" ]; then
  echo "  !! 60초 안에 토큰이 채워지지 않았다. 컨트롤러가 레거시 SA 토큰을 발급하지 않는 클러스터일 수 있다." >&2
  exit 1
fi
CA="$(kubectl --context "$REMOTE_CONTEXT" $KUBECTL_TIMEOUT get secret kagent-remote-reader-token \
  -n kagent -o jsonpath='{.data.ca\.crt}')"
SERVER="$(kubectl config view --minify --context "$REMOTE_CONTEXT" -o jsonpath='{.clusters[0].cluster.server}')"
if [ -z "$CA" ] || [ -z "$SERVER" ]; then
  echo "  !! CA 또는 API 서버 주소를 얻지 못했다 (CA=${#CA}B server=[$SERVER])." >&2
  exit 1
fi
echo "  -> 토큰·CA 확보, API 서버 $SERVER"

echo "[4/5] 권한 상한 실측 (읽기는 되고 쓰기는 막혀야 한다)"
SA="system:serviceaccount:kagent:kagent-remote-reader"
can() { kubectl --context "$REMOTE_CONTEXT" $KUBECTL_TIMEOUT auth can-i "$1" "$2" --all-namespaces --as="$SA" 2>/dev/null | tail -1; }
for pair in "get nodes" "list pods"; do
  # shellcheck disable=SC2086
  if [ "$(can $pair)" != "yes" ]; then echo "  !! 읽기 권한 없음: $pair" >&2; exit 1; fi
done
for pair in "list secrets" "create deployments" "delete pods"; do
  # shellcheck disable=SC2086
  if [ "$(can $pair)" != "no" ]; then echo "  !! 권한 과다: $pair 가 허용된다" >&2; exit 1; fi
done
echo "  -> 읽기 허용 / secrets·쓰기·삭제 거부 확인"

# kubeconfig는 임시 파일로만 존재하고 Secret 생성 직후 지운다(trap cleanup).
TMP_KUBECONFIG="$(mktemp -t remote-reader-kubeconfig)"
chmod 600 "$TMP_KUBECONFIG"
cat > "$TMP_KUBECONFIG" <<EOF
apiVersion: v1
kind: Config
clusters:
- name: remote
  cluster:
    server: ${SERVER}
    certificate-authority-data: ${CA}
users:
- name: remote-reader
  user:
    token: $(printf '%s' "$TOKEN" | base64 -d)
contexts:
- name: remote
  context: {cluster: remote, user: remote-reader}
current-context: remote
EOF

echo "[5/5] 로컬 클러스터 [$LOCAL_CONTEXT]에 Secret·도구 서버·kagent CR 적용"
kubectl --context "$LOCAL_CONTEXT" $KUBECTL_TIMEOUT create secret generic remote-reader-kubeconfig \
  --from-file=config="$TMP_KUBECONFIG" -n default --dry-run=client -o yaml \
  | kubectl --context "$LOCAL_CONTEXT" $KUBECTL_TIMEOUT apply -f -
kubectl --context "$LOCAL_CONTEXT" $KUBECTL_TIMEOUT apply -f "${SCRIPT_DIR}/remote-reader-tools.yaml"
# Secret이 바뀌었을 수 있으므로 기존 파드를 새 자격증명으로 갈아끼운다.
kubectl --context "$LOCAL_CONTEXT" $KUBECTL_TIMEOUT rollout restart deploy/remote-reader-tools -n default >/dev/null
kubectl --context "$LOCAL_CONTEXT" $KUBECTL_TIMEOUT rollout status deploy/remote-reader-tools -n default --timeout=180s
kubectl --context "$LOCAL_CONTEXT" $KUBECTL_TIMEOUT apply -f "${SCRIPT_DIR}/remote-reader-agent.yaml"

echo ""
echo "적용 완료. 실제 연결 여부는 아직 확인되지 않았다 — 아래로 확인한다:"
echo "  kubectl --context $LOCAL_CONTEXT get remotemcpserver remote-reader-tool-server -n kagent \\"
echo "    -o jsonpath='{.status.discoveredTools[*].name}'"
echo "  (비어 있으면 도구 서버에 닿지 못한 것이다)"
echo "그 다음 kagent UI(http://127.0.0.1:8090)에서 remote-cluster-agent에게 [$REMOTE_CONTEXT] 상태를 물어본다."
