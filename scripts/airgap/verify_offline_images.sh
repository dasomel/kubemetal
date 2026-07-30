#!/usr/bin/env bash
# ==============================================================================
# verify_offline_images.sh
# 폐쇄망 기동 가능성 검증 — 네트워크를 끊지 않고 확인한다.
#
# 방법: 매니페스트가 요구하는 각 이미지로 `imagePullPolicy: Never` 파드를 띄운다.
# 이 정책은 kubelet에게 "레지스트리를 절대 조회하지 말라"고 강제하므로,
#   • 이미지가 런타임 저장소에 없으면  → waiting.reason = ErrImageNeverPull  (FAIL)
#   • 있으면                          → 그 외 상태로 진행                   (PASS)
# 즉 레지스트리 접근 0인 상태의 기동 가능성을 호스트 네트워크를 건드리지 않고 판정한다.
# (컨테이너가 이후 Crash/Completed로 끝나는지는 이 검증의 관심사가 아니다 — 우리가 보는
#  것은 "이미지를 가져올 필요가 있었는가"뿐이다.)
#
# 검증 후 네임스페이스를 삭제한다. 기존 워크로드는 건드리지 않는다.
# ==============================================================================

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
# shellcheck source=scripts/airgap/lib.sh
. "${SCRIPT_DIR}/lib.sh"
CONTEXT="${KUBE_CONTEXT:-colima}"
NS="airgap-verify"
TIMEOUT_SECS="${TIMEOUT_SECS:-60}"

k() { kubectl --context "$CONTEXT" "$@"; }

cleanup() { k delete namespace "$NS" --wait=false >/dev/null 2>&1 || true; }
trap cleanup EXIT

echo "======================================================================"
echo "  Air-Gap 기동 가능성 검증 (imagePullPolicy: Never)"
echo "  context=${CONTEXT}  namespace=${NS}"
echo "======================================================================"

# 수집 대상과 **같은 두 출처**를 읽는다(download_airgap_bundle.sh와 동일):
# 매니페스트의 image: 라인 + 매니페스트가 선언하지 않는 이미지 목록.
IMAGES=()

# 이미지 문자열은 아래에서 heredoc YAML로 보간된다. 출처는 리포 내 매니페스트와
# images-helm.txt뿐이지만, 레지스트리 참조에 쓰이는 문자만 통과시켜 YAML 구조를 깨뜨릴
# 수 있는 입력이 애초에 배열에 들어오지 못하게 한다(오타도 여기서 걸린다).
IMAGE_RE='^[A-Za-z0-9._/:@-]+$'

add_image() {
  local img="$1"
  if ! [[ "$img" =~ $IMAGE_RE ]]; then
    echo "  !! 이미지 참조로 볼 수 없는 값을 건너뜁니다: '${img}'" >&2
    SKIPPED+=("$img")
    return 0
  fi
  for seen in ${IMAGES[@]+"${IMAGES[@]}"}; do [ "$seen" = "$img" ] && return 0; done
  IMAGES+=("$img")
}
SKIPPED=()

while IFS= read -r img; do
  [ -n "$img" ] && add_image "$img"
done < <(manifest_images "$PROJECT_ROOT")
manifest_count=${#IMAGES[@]}

while IFS= read -r img; do
  add_image "$img"
done < <(read_image_list "${SCRIPT_DIR}/images-helm.txt")

if [ ${#IMAGES[@]} -eq 0 ]; then
  echo "검증할 이미지를 찾지 못했습니다." >&2
  exit 1
fi
echo "검증 대상 ${#IMAGES[@]}개 (매니페스트 ${manifest_count}개 + 비매니페스트 $(( ${#IMAGES[@]} - manifest_count ))개)"

k create namespace "$NS" >/dev/null 2>&1 || true

# 파드 이름은 이미지에서 파생 — DNS-1123 라벨 규칙에 맞춰 소문자/영숫자/하이픈만 남긴다.
# bash 3.2에는 `${1,,}`가 없어 tr로 소문자화한다.
pod_name_for() {
  echo "v-$(echo "$1" | tr '[:upper:]' '[:lower:]' \
    | sed -e 's#[^a-z0-9]#-#g' -e 's#-\{2,\}#-#g' | cut -c1-58)"
}

# 파드 이름은 여기서 한 번만 계산해 IMAGES와 같은 인덱스로 들고 간다
# (판정 루프에서 다시 파생하면 이미지당 4개 프로세스를 한 번 더 태우게 된다).
PODS=()
for img in "${IMAGES[@]}"; do
  pod="$(pod_name_for "$img")"
  PODS+=("$pod")
  k apply -n "$NS" -f - >/dev/null <<YAML
apiVersion: v1
kind: Pod
metadata:
  name: ${pod}
spec:
  restartPolicy: Never
  containers:
    - name: probe
      image: ${img}
      imagePullPolicy: Never
YAML
done

# 파드는 위에서 전부 만들어 클러스터에서 동시에 진행 중이다. 따라서 관측도 한 번에
# 모아서 한다 — 이미지별로 순차 폴링하면 하나가 느릴 때 그 이미지만 TIMEOUT_SECS를
# 통째로 태우고, 총 소요가 이미지 수에 비례해 늘어난다(20개 × 60초 = 20분).
# 틱마다 `get pods` 1회 + python3 1회만 쓰므로 프로세스 수도 이미지 수와 무관해진다.
IFS= read -r -d '' POD_STATES_PY <<'PY' || true
import json, sys
for p in json.load(sys.stdin).get('items', []):
    st = p.get('status', {})
    reason = 'phase:' + (st.get('phase') or '')
    for cs in st.get('containerStatuses') or []:
        s = cs.get('state', {})
        if 'waiting' in s:
            reason = 'waiting:' + (s['waiting'].get('reason') or '')
        elif 'running' in s:
            reason = 'running'
        elif 'terminated' in s:
            reason = 'terminated:' + (s['terminated'].get('reason') or '')
        break
    print(p['metadata']['name'] + '\t' + reason)
PY

echo ""
echo "판정 (최대 ${TIMEOUT_SECS}초 관측)..."

VERDICTS=()
for i in "${!IMAGES[@]}"; do VERDICTS[$i]=""; done
pending=${#IMAGES[@]}
elapsed=0

while [ "$pending" -gt 0 ] && [ "$elapsed" -lt "$TIMEOUT_SECS" ]; do
  states="$(k get pods -n "$NS" -o json 2>/dev/null | python3 -c "$POD_STATES_PY" 2>/dev/null)"
  # here-string은 현재 셸에서 돌아 VERDICTS/pending 갱신이 루프 밖까지 남는다.
  while IFS=$'\t' read -r pname reason; do
    [ -n "$pname" ] || continue
    for i in "${!PODS[@]}"; do
      [ "${PODS[$i]}" = "$pname" ] || continue
      [ -z "${VERDICTS[$i]}" ] || break
      case "$reason" in
        waiting:ErrImageNeverPull)
          VERDICTS[$i]="FAIL — ErrImageNeverPull (런타임에 이미지 없음 = 레지스트리 필요)"
          pending=$((pending - 1))
          ;;
        running|terminated:*|waiting:CrashLoopBackOff|waiting:RunContainerError|waiting:CreateContainerError|waiting:StartError)
          # 컨테이너가 기동을 시도했다 = 이미지는 로컬에 있었다.
          VERDICTS[$i]="PASS — ${reason}"
          pending=$((pending - 1))
          ;;
      esac
      break
    done
  done <<< "$states"

  [ "$pending" -gt 0 ] || break
  sleep 2
  elapsed=$((elapsed + 2))
done

FAILED=()
for i in "${!IMAGES[@]}"; do
  verdict="${VERDICTS[$i]:-관측 실패 (${TIMEOUT_SECS}초 내 판정 불가)}"
  printf "  %-42s %s\n" "${IMAGES[$i]}" "$verdict"
  case "$verdict" in PASS*) ;; *) FAILED+=("${IMAGES[$i]}") ;; esac
done

echo ""
echo "======================================================================"
if [ ${#FAILED[@]} -eq 0 ] && [ ${#SKIPPED[@]} -eq 0 ]; then
  echo "  RESULT: PASS — ${#IMAGES[@]}개 전부 레지스트리 접근 없이 기동 가능"
  echo "======================================================================"
  exit 0
fi
# 검증하지 못한 항목이 있으면 PASS라고 말하지 않는다 — 판정하지 않은 것을 통과로
# 보고하는 것이 이 리포지터리가 가장 자주 저지른 실패다(D22–D25).
if [ ${#SKIPPED[@]} -gt 0 ]; then
  echo "  RESULT: FAIL — 이미지 참조로 해석되지 않아 검증하지 못한 항목 ${#SKIPPED[@]}개: ${SKIPPED[*]}" >&2
fi
if [ ${#FAILED[@]} -gt 0 ]; then
  echo "  RESULT: FAIL — 레지스트리가 필요한 이미지 ${#FAILED[@]}개: ${FAILED[*]}" >&2
fi
# 안내 문구에 백틱을 쓰지 않는다 — 큰따옴표 안의 백틱은 명령 치환이라 실제로 실행된다.
echo "  'make provision-all' 후 Air-Gap 탭에서 패키지 다운로드 → 오프라인 설치를 실행하세요." >&2
echo "======================================================================"
exit 1
