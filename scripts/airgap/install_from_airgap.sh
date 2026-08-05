#!/usr/bin/env bash
# ==============================================================================
# KubeMetal Air-Gap Offline Installer
# 수집된 .tar.gz 이미지를 로드하고 폐쇄망에서 kagent/MLOps 스택을 프로비저닝한다.
#
# 규칙: 각 단계의 실패를 삼키지 않는다(`|| true` 금지). 실패 항목을 모아 마지막에
# 출력하고 0이 아닌 코드로 종료한다 — 아무것도 설치되지 않았는데 "완료"를 보고하면
# 폐쇄망에서 원인 추적이 불가능해진다.
# ==============================================================================

set -uo pipefail

AIRGAP_DIR="${HOME}/.kubemetal/airgap"
KUBE_CONTEXT="${KUBE_CONTEXT:-colima}"

if [ ! -d "${AIRGAP_DIR}" ]; then
  echo "Air-Gap 저장소가 없습니다: ${AIRGAP_DIR} — 먼저 패키지 다운로드를 실행하세요." >&2
  exit 1
fi

FAILED=()

# 폐쇄망 설치는 되돌리기 어렵다 — 로드를 시작하기 전에 번들이 수집 당시와 같은지 먼저 본다.
# manifest는 download_airgap_bundle.sh가 생성한다. 없으면 검증을 조용히 건너뛰지 않고
# 사실을 알린다(구버전 번들일 수 있으므로 중단까지는 하지 않는다).
MANIFEST="${AIRGAP_DIR}/manifest.sha256"
echo "[0/3] 번들 무결성 검증..."
if [ ! -f "$MANIFEST" ]; then
  echo "  !! manifest.sha256이 없습니다 — 무결성 검증 없이 진행합니다." >&2
  echo "     (인터넷 연결 환경에서 패키지 다운로드를 다시 실행하면 생성됩니다.)" >&2
else
  if ( cd "$AIRGAP_DIR" && shasum -a 256 -c "$(basename "$MANIFEST")" --status ); then
    echo "  -> $(wc -l < "$MANIFEST" | tr -d ' ')개 파일 해시 일치"
  else
    echo "  !! 번들이 수집 당시와 다릅니다 — 손상되었거나 변조되었습니다." >&2
    ( cd "$AIRGAP_DIR" && shasum -a 256 -c "$(basename "$MANIFEST")" 2>/dev/null | grep -v ': OK$' | head -10 ) >&2
    echo "     설치를 중단합니다. 번들을 다시 수집하세요." >&2
    exit 1
  fi
fi

echo "[1/3] .tar.gz 컨테이너 이미지 로드..."
if ! command -v docker >/dev/null 2>&1; then
  echo "  !! docker CLI가 없습니다." >&2
  FAILED+=("docker-missing")
else
  loaded=0
  shopt -s nullglob
  for archive in "${AIRGAP_DIR}/images/"*.tar.gz; do
    echo "  -> 로드: $(basename "$archive")"
    if gunzip -c "$archive" | docker load; then
      loaded=$((loaded + 1))
    else
      FAILED+=("load:$(basename "$archive")")
    fi
  done
  for archive in "${AIRGAP_DIR}/images/"*.tar; do
    echo "  -> 로드(비압축): $(basename "$archive")"
    if docker load -i "$archive"; then
      loaded=$((loaded + 1))
    else
      FAILED+=("load:$(basename "$archive")")
    fi
  done
  shopt -u nullglob
  echo "  -> 이미지 ${loaded}건 로드"
  if [ "$loaded" -eq 0 ]; then
    FAILED+=("images:none-found")
  fi
fi

echo "[2/3] 오프라인 kagent Helm 차트 프로비저닝..."
# CRD 차트가 먼저다(D33 개정 2) — 본 차트 템플릿의 Agent/ModelConfig/RemoteMCPServer는
# CRD 없이 렌더되지 않는다. 순서를 바꾸면 최초 설치만 실패하고 재설치는 성공해 원인이 숨는다.
CRD_CHART="${AIRGAP_DIR}/charts/kagent-crds-0.9.12.tgz"
CHART="${AIRGAP_DIR}/charts/kagent-0.9.12.tgz"
if [ ! -s "$CRD_CHART" ]; then
  echo "  !! CRD 차트 없음: ${CRD_CHART}" >&2
  FAILED+=("crd-chart-missing")
elif ! helm upgrade --install kagent-crds "$CRD_CHART" \
       --namespace kagent --create-namespace \
       --kube-context "$KUBE_CONTEXT" --reuse-values; then
  FAILED+=("helm-install-crds")
elif [ ! -s "$CHART" ]; then
  echo "  !! 차트 없음: ${CHART}" >&2
  FAILED+=("chart-missing")
elif ! helm upgrade --install kagent "$CHART" \
       --namespace kagent --create-namespace \
       --kube-context "$KUBE_CONTEXT" --reuse-values; then
  FAILED+=("helm-install")
fi

echo "[3/3] 오프라인 K8s 매니페스트 적용..."
if [ ! -d "${AIRGAP_DIR}/manifests" ]; then
  echo "  !! 매니페스트 디렉터리 없음" >&2
  FAILED+=("manifests-missing")
elif ! kubectl --context "$KUBE_CONTEXT" apply -f "${AIRGAP_DIR}/manifests/"; then
  FAILED+=("kubectl-apply")
fi

echo ""
if [ ${#FAILED[@]} -eq 0 ]; then
  echo "완료: 오프라인 번들 기반 프로비저닝 성공 (context=${KUBE_CONTEXT})"
  exit 0
fi

echo "실패 항목 ${#FAILED[@]}건: ${FAILED[*]}" >&2
exit 1
