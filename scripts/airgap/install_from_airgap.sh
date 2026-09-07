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

# digests.lock은 tag가 수집 뒤 이동했는지 증명하는 provenance와, docker load가
# RepoDigests를 버리는 경우에도 비교할 수 있는 image ID를 함께 담는다. 없을 때만
# 구버전 번들 호환을 위해 전부 건너뛴다; 일부 누락/형식 오류는 D23대로 중단한다.
VERIFY_DIGESTS=0
DIGEST_VERIFICATION_FAILED=0
if [ ! -f "$DIGESTS_LOCK" ]; then
  echo "  !! digests.lock이 없는 구버전 번들 — 이미지 digest 검증을 건너뜁니다." >&2
else
  VERIFY_DIGESTS=1
fi

# docker load의 stdout에서 실제로 로드된 참조를 뽑는다: 태그가 있으면
# `Loaded image: <ref>`, 태그 없이 로드되면 `Loaded image ID: sha256:<id>`.
loaded_refs_from_output() { sed -n 's/^Loaded image: //p' "$1"; }
loaded_ids_from_output() { sed -n 's/^Loaded image ID: //p' "$1"; }

verify_loaded_image_digest() {
  local archive="$1" load_output="$2"
  local record locked_image locked_provenance locked_id actual_record actual_provenance actual_id
  local loaded_refs loaded_ids ref
  if ! record="$(digest_lock_record_for_archive "$DIGESTS_LOCK" "$(basename "$archive")")"; then
    echo "  !! $(basename "$archive"): digests.lock에 유효한 항목이 없습니다." >&2
    FAILED+=("digest-lock-missing-or-malformed:$(basename "$archive")")
    DIGEST_VERIFICATION_FAILED=1
    return 1
  fi
  IFS=$'\t' read -r locked_image locked_provenance locked_id <<EOF
$record
EOF

  # docker load가 이번 호출에서 실제로 보고한 참조/ID만 신뢰한다. lock이 기대하는
  # 태그를 이름으로 바로 inspect하면, 데몬에 같은 태그의 이전(무관한) 이미지가 이미
  # 있을 때 이번 아카이브의 실제 페이로드가 검증 없이 다른 태그로(또는 태그 없이)
  # 자리잡아도 통과해 버린다 — docker 자신이 이번 load에서 무엇을 로드했다고 보고
  # 했는지부터 확인한다.
  loaded_refs="$(loaded_refs_from_output "$load_output")"
  loaded_ids="$(loaded_ids_from_output "$load_output")"

  if [ -n "$loaded_refs" ]; then
    if ! grep -Fxq "$locked_image" <<<"$loaded_refs"; then
      echo "  !! ${locked_image}: 이번 load가 보고한 참조($(tr '\n' ' ' <<<"$loaded_refs"))에 없습니다 — 아카이브의 페이로드가 다른 곳에 로드되었습니다." >&2
      FAILED+=("digest-mismatch:${locked_image}")
      DIGEST_VERIFICATION_FAILED=1
      while IFS= read -r ref; do
        [ -n "$ref" ] && docker rmi "$ref" >/dev/null 2>&1
      done <<<"$loaded_refs"
      return 1
    fi
  elif [ -n "$loaded_ids" ]; then
    # 태그 없이 로드됨 — inspect할 이름이 없으므로 docker load가 보고한 image ID를
    # lock의 ID와 직접 비교한다.
    if ! grep -Fxq "$locked_id" <<<"$loaded_ids"; then
      echo "  !! ${locked_image}: 이번 load가 보고한 image ID($(tr '\n' ' ' <<<"$loaded_ids"))가 lock(${locked_id})과 다릅니다." >&2
      FAILED+=("digest-mismatch:${locked_image}")
      DIGEST_VERIFICATION_FAILED=1
      while IFS= read -r ref; do
        [ -n "$ref" ] && docker rmi "$ref" >/dev/null 2>&1
      done <<<"$loaded_ids"
      return 1
    fi
  else
    echo "  !! ${locked_image}: docker load 출력에서 로드된 참조/ID를 찾지 못했습니다." >&2
    FAILED+=("digest-lock-missing-or-malformed:$(basename "$archive")")
    DIGEST_VERIFICATION_FAILED=1
    return 1
  fi

  if ! actual_record="$(image_digest_record "$locked_image")"; then
    echo "  !! ${locked_image}: load 뒤 image ID를 확인하지 못했습니다." >&2
    FAILED+=("digest-inspect:${locked_image}")
    DIGEST_VERIFICATION_FAILED=1
    return 1
  fi
  IFS=$'\t' read -r actual_provenance actual_id <<EOF
$actual_record
EOF

  # load가 RepoDigests를 보존했을 때만 provenance를 비교한다. 그렇지 않으면 수집
  # 당시 함께 잠근 ID와 대조한다; 수집 자체가 ID fallback이었던 경우도 이 경로다.
  if [ "$locked_provenance" != "$locked_id" ] && [ "$actual_provenance" != "$actual_id" ]; then
    if [ "$actual_provenance" != "$locked_provenance" ]; then
      echo "  !! ${locked_image}: RepoDigest 불일치 (lock=${locked_provenance}, load=${actual_provenance})" >&2
      FAILED+=("digest-mismatch:${locked_image}")
      DIGEST_VERIFICATION_FAILED=1
      docker rmi "$locked_image" >/dev/null 2>&1
      return 1
    fi
  elif [ "$actual_id" != "$locked_id" ]; then
    echo "  !! ${locked_image}: image ID 불일치 (lock=${locked_id}, load=${actual_id})" >&2
    FAILED+=("digest-mismatch:${locked_image}")
    DIGEST_VERIFICATION_FAILED=1
    docker rmi "$locked_image" >/dev/null 2>&1
    return 1
  fi
  return 0
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
    load_output="$(mktemp)"
    if gunzip -c "$archive" | docker load | tee "$load_output"; then
      loaded=$((loaded + 1))
      [ "$VERIFY_DIGESTS" -eq 0 ] || verify_loaded_image_digest "$archive" "$load_output"
    else
      FAILED+=("load:$(basename "$archive")")
    fi
    rm -f "$load_output"
  done
  for archive in "${AIRGAP_DIR}/images/"*.tar; do
    echo "  -> 로드(비압축): $(basename "$archive")"
    load_output="$(mktemp)"
    if docker load -i "$archive" | tee "$load_output"; then
      loaded=$((loaded + 1))
      [ "$VERIFY_DIGESTS" -eq 0 ] || verify_loaded_image_digest "$archive" "$load_output"
    else
      FAILED+=("load:$(basename "$archive")")
    fi
    rm -f "$load_output"
  done
  shopt -u nullglob
  echo "  -> 이미지 ${loaded}건 로드"
  if [ "$DIGEST_VERIFICATION_FAILED" -ne 0 ]; then
    echo "  !! 이미지 digest 검증에 실패해 이후 설치를 중단합니다." >&2
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
