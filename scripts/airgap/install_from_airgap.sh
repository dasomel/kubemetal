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

# AIRGAP_DIR은 덮어쓸 수 있어야 한다 — 하드코딩돼 있을 때는 이 스크립트의 게이트를
# 실제 번들을 건드리지 않고 확인할 방법이 없었다(그래서 검증하려던 시도가 진짜 번들을
# 로드하기 시작했다). KUBE_CONTEXT가 이미 같은 규약이다.
AIRGAP_DIR="${AIRGAP_DIR:-${HOME}/.kubemetal/airgap}"
KUBE_CONTEXT="${KUBE_CONTEXT:-colima}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/airgap/lib.sh
. "${SCRIPT_DIR}/lib.sh"

if [ ! -d "${AIRGAP_DIR}" ]; then
  echo "Air-Gap 저장소가 없습니다: ${AIRGAP_DIR} — 먼저 패키지 다운로드를 실행하세요." >&2
  exit 1
fi

FAILED=()

# 폐쇄망 설치는 되돌리기 어렵다 — 로드를 시작하기 전에 번들이 수집 당시와 같은지 먼저 본다.
# manifest는 download_airgap_bundle.sh가 생성한다.
#
# manifest가 없으면 **중단한다**(이슈 #8). 예전에는 "구버전 번들일 수 있으므로" 경고만
# 하고 진행했는데, 검증할 수단이 없는 번들에서 "구버전"과 "변조됨"은 구분되지 않는다 —
# 그 둘을 구분하지 못하는 채로 진행하는 것이 정확히 공급망 검증이 막아야 할 상황이다.
# D23이 "불일치 시 아무것도 로드하지 않고 중단"을 정한 것과 같은 이유이고, 검증 자체가
# 없는 경우만 그 규약을 빠져나가고 있었다. 확인되지 않은 값으로는 렌더를 거부하는
# render.sh(D26)와 같은 태도다.
#
# 구버전 번들을 알면서 쓰려면 의도를 명시해야 한다 — 기본값이 아니라 옵트아웃이다.
MANIFEST="${AIRGAP_DIR}/manifest.sha256"
DIGESTS_LOCK="${AIRGAP_DIR}/digests.lock"
echo "[0/3] 번들 무결성 검증..."
if [ ! -f "$MANIFEST" ]; then
  if [ "${AIRGAP_ALLOW_UNVERIFIED:-0}" = "1" ]; then
    echo "  !! manifest.sha256이 없는데 AIRGAP_ALLOW_UNVERIFIED=1로 검증을 건너뜁니다." >&2
    echo "     이 번들의 무결성은 확인되지 않았습니다 — 손상·변조를 탐지할 수 없습니다." >&2
  else
    echo "  !! manifest.sha256이 없어 번들 무결성을 검증할 수 없습니다." >&2
    echo "     설치를 중단합니다 — 검증되지 않은 번들은 설치하지 않습니다." >&2
    echo "     인터넷 연결 환경에서 번들 다운로드를 다시 실행하면 생성됩니다." >&2
    echo "     구버전 번들인 것이 확실하다면 AIRGAP_ALLOW_UNVERIFIED=1로 재실행하세요." >&2
    exit 1
  fi
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

# legacy bundles predate digests.lock. Missing or malformed entries are otherwise a supply-chain
# failure, not a warning: tag references alone cannot prove what docker load restored.
VERIFY_IMAGE_IDS=1
if [ "${AIRGAP_ALLOW_UNLOCKED:-0}" = "1" ]; then
  VERIFY_IMAGE_IDS=0
  echo "  !! AIRGAP_ALLOW_UNLOCKED=1: digests.lock/image ID 검증을 건너뜁니다." >&2
  echo "     경고: 잘못된 archive나 기존 daemon cache의 다른 이미지를 탐지할 수 없습니다." >&2
elif [ ! -f "$DIGESTS_LOCK" ]; then
  echo "  !! digests.lock이 없어 로드할 이미지의 image ID를 검증할 수 없습니다." >&2
  echo "     설치를 중단합니다. 구버전 번들이 확실할 때만 AIRGAP_ALLOW_UNLOCKED=1로 재실행하세요." >&2
  exit 1
fi

read_digest_lock_record() {
  local archive="$1" record
  if ! record="$(digest_lock_record_for_archive "$DIGESTS_LOCK" "$(basename "$archive")")"; then
    echo "  !! $(basename "$archive"): digests.lock 항목이 없거나 형식이 잘못되었습니다." >&2
    return 1
  fi
  read -r LOCKED_IMAGE LOCKED_REPO_DIGEST LOCKED_IMAGE_ID <<< "$record"
  case "$LOCKED_REPO_DIGEST" in
    unverified|*@sha256:?*) ;;
    *)
      echo "  !! ${LOCKED_IMAGE}: digests.lock의 RepoDigest 형식이 잘못되었습니다." >&2
      return 1
      ;;
  esac
  case "$LOCKED_IMAGE_ID" in
    sha256:?*) ;;
    *)
      echo "  !! ${LOCKED_IMAGE}: digests.lock의 image ID 형식이 잘못되었습니다." >&2
      return 1
      ;;
  esac
}

verify_preexisting_image_id() {
  local archive="$1" existing_id
  read_digest_lock_record "$archive" || return 1
  if existing_id="$(image_id_lock_value "$LOCKED_IMAGE")"; then
    if [ "$existing_id" != "$LOCKED_IMAGE_ID" ]; then
      echo "  !! ${LOCKED_IMAGE}: load 전 daemon cache image ID 불일치 (lock=${LOCKED_IMAGE_ID}, cache=${existing_id})" >&2
      return 1
    fi
  fi
}

verify_loaded_image_id() {
  local archive="$1" actual_id
  read_digest_lock_record "$archive" || return 1
  if ! actual_id="$(image_id_lock_value "$LOCKED_IMAGE")" || [ -z "$actual_id" ]; then
    echo "  !! ${LOCKED_IMAGE}: load 뒤 image ID를 조회하지 못했습니다." >&2
    return 1
  fi
  if [ "$actual_id" != "$LOCKED_IMAGE_ID" ]; then
    echo "  !! ${LOCKED_IMAGE}: image ID 불일치 (lock=${LOCKED_IMAGE_ID}, load=${actual_id})" >&2
    return 1
  fi
}

echo "[1/3] .tar.gz 컨테이너 이미지 로드..."
if ! command -v docker >/dev/null 2>&1; then
  echo "  !! docker CLI가 없습니다." >&2
  FAILED+=("docker-missing")
else
  loaded=0
  shopt -s nullglob
  for archive in "${AIRGAP_DIR}/images/"*.tar.gz; do
    echo "  -> 로드: $(basename "$archive")"
    if [ "$VERIFY_IMAGE_IDS" -eq 1 ] && ! verify_preexisting_image_id "$archive"; then
      FAILED+=("image-id-cache-mismatch:$(basename "$archive")")
    elif gunzip -c "$archive" | docker load; then
      loaded=$((loaded + 1))
      if [ "$VERIFY_IMAGE_IDS" -eq 1 ] && ! verify_loaded_image_id "$archive"; then
        FAILED+=("image-id-mismatch:$(basename "$archive")")
      fi
    else
      FAILED+=("load:$(basename "$archive")")
    fi
  done
  for archive in "${AIRGAP_DIR}/images/"*.tar; do
    echo "  -> 로드(비압축): $(basename "$archive")"
    if [ "$VERIFY_IMAGE_IDS" -eq 1 ] && ! verify_preexisting_image_id "$archive"; then
      FAILED+=("image-id-cache-mismatch:$(basename "$archive")")
    elif docker load -i "$archive"; then
      loaded=$((loaded + 1))
      if [ "$VERIFY_IMAGE_IDS" -eq 1 ] && ! verify_loaded_image_id "$archive"; then
        FAILED+=("image-id-mismatch:$(basename "$archive")")
      fi
    else
      FAILED+=("load:$(basename "$archive")")
    fi
  done
  shopt -u nullglob
  echo "  -> 이미지 ${loaded}건 로드"
  if [ ${#FAILED[@]} -ne 0 ]; then
    echo "  !! 이미지 image ID 검증에 실패해 이후 설치를 중단합니다." >&2
    echo "실패 항목 ${#FAILED[@]}건: ${FAILED[*]}" >&2
    exit 1
  fi
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
