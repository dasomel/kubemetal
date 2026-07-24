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
CHART="${AIRGAP_DIR}/charts/kagent-0.9.12.tgz"
if [ ! -s "$CHART" ]; then
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
