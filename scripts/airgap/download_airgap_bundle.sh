#!/usr/bin/env bash
# ==============================================================================
# KubeMetal Air-Gap Bundle Downloader
# 컨테이너 이미지·차트·바이너리를 .tar.gz로 오프라인 보관한다.
#
# 규칙: 부분 수신 파일을 최종 경로에 남기지 않는다. `curl ... || true`로 실패를 삼키면
# 0바이트/HTML 에러 페이지가 남고, 상태 조회(get_airgap_status)가 그것을 "보유 완료"로
# 보고하게 된다. 모든 산출물은 .part로 받아 성공했을 때만 최종 이름으로 옮긴다.
# 하나라도 실패하면 마지막에 목록을 출력하고 0이 아닌 코드로 종료한다.
# ==============================================================================

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
AIRGAP_DIR="${HOME}/.kubemetal/airgap"
mkdir -p "${AIRGAP_DIR}/charts" "${AIRGAP_DIR}/images" "${AIRGAP_DIR}/binaries" "${AIRGAP_DIR}/manifests"

FAILED=()

# get_airgap_status(Rust)의 MIN_VALID_ASSET_BYTES와 같은 하한. `-s`(0바이트 아님)만으로
# 판정하면 9바이트짜리 "Not Found" 본문 같은 실패 응답을 "이미 보유"로 보고 건너뛴다
# (실기기에서 binaries/kubescape가 이 상태였다, 2026-07-25).
MIN_VALID_BYTES=1024

# 번들 무결성 목록. 업스트림이 체크섬을 게시하는 자산은 받은 즉시 그것과 대조하고,
# 우리가 만드는 아카이브(`docker save | gzip`은 바이트 재현성이 없다)는 생성 시점 해시를
# 여기에 기록해 두어 설치 시 이송 중 손상·변조를 잡는다.
MANIFEST="${AIRGAP_DIR}/manifest.sha256"

is_valid() {
  local f="$1"
  [ -f "$f" ] || return 1
  [ "$(wc -c < "$f" | tr -d ' ')" -ge "$MIN_VALID_BYTES" ]
}

sha256_of() { shasum -a 256 "$1" | awk '{print $1}'; }

# 업스트림이 게시한 기대 해시를 가져온다.
#   bare  : 파일 전체가 sha256 한 줄 (kubescape의 `<asset>.sha256`)
#   list  : `<sha>  <파일명>` 목록에서 해당 항목 (k3s의 `sha256sum-arm64.txt`)
fetch_expected_sha() {
  local kind="$1" url="$2" entry="${3:-}"
  case "$kind" in
    bare) curl -fsSL -m 30 "$url" 2>/dev/null | tr -d '[:space:]' ;;
    list) curl -fsSL -m 30 "$url" 2>/dev/null | awk -v e="$entry" '$2 == e {print $1; exit}' ;;
  esac
}

# 업스트림 체크섬이 있으면 반드시 대조한다 — 불일치 파일은 남기지 않고 폐기한다.
# 기대 해시 조회 자체가 실패하면 검증 없이 통과시키지 않고 실패로 처리한다.
fetch_binary() {
  local name="$1" url="$2" sha_kind="$3" sha_url="$4" sha_entry="${5:-}"
  local dest="${AIRGAP_DIR}/binaries/${name}"

  local expected
  expected="$(fetch_expected_sha "$sha_kind" "$sha_url" "$sha_entry")"
  if [ -z "$expected" ]; then
    echo "  !! ${name}: 업스트림 체크섬을 가져오지 못했습니다 — 검증 없이 저장하지 않습니다." >&2
    FAILED+=("checksum-unavailable:${name}")
    return 0
  fi

  if is_valid "$dest" && [ "$(sha256_of "$dest")" = "$expected" ]; then
    echo "  -> 이미 보유(체크섬 일치): ${name}"
    return 0
  fi
  if [ -f "$dest" ]; then
    echo "  -> 무효 파일 폐기 후 재수집: ${name} ($(wc -c < "$dest" | tr -d ' ')B)"
    rm -f "$dest"
  fi

  echo "  -> 다운로드: ${name}"
  # -f: HTTP 에러를 실패로 처리(에러 페이지를 파일로 저장하지 않는다)
  if ! curl -fsSL "$url" -o "${dest}.part"; then
    rm -f "${dest}.part"
    FAILED+=("binary:${name}")
    return 0
  fi

  local actual
  actual="$(sha256_of "${dest}.part")"
  if [ "$actual" != "$expected" ]; then
    echo "  !! ${name}: 체크섬 불일치 (기대 ${expected}, 실제 ${actual}) — 폐기합니다." >&2
    rm -f "${dest}.part"
    FAILED+=("checksum-mismatch:${name}")
    return 0
  fi

  mv "${dest}.part" "$dest"
  chmod +x "$dest"
  echo "     체크섬 검증 통과: ${expected}"
}

echo "[1/4] K3s & Kubescape 바이너리 수집..."
K3S_BASE="https://github.com/k3s-io/k3s/releases/download/v1.28.2%2Bk3s1"
# Apple Silicon 전용 프로젝트다 — K3s는 Colima(vz)의 **arm64 리눅스 VM**에서 돈다.
# 자산 `k3s`는 amd64라 이 VM에서 실행될 수 없다(실기기 확인 2026-07-25: 수집돼 있던
# 파일이 `ELF 64-bit x86-64`였다). arm64 자산은 `k3s-arm64`이며, 로컬 파일명은
# get_airgap_status가 기대하는 `binaries/k3s`를 유지한다.
fetch_binary "k3s" "${K3S_BASE}/k3s-arm64" list "${K3S_BASE}/sha256sum-arm64.txt" "k3s-arm64"
# 자산명은 `kubescape-arm64-macos-latest` — `kubescape-macos-arm64`는 존재하지 않는 이름이라
# 404를 돌려주며, 구버전 스크립트는 그 "Not Found" 본문을 바이너리로 저장했다(D23, 2026-07-25).
KS_BASE="https://github.com/kubescape/kubescape/releases/download/v3.0.0"
fetch_binary "kubescape" "${KS_BASE}/kubescape-arm64-macos-latest" \
  bare "${KS_BASE}/kubescape-arm64-macos-latest.sha256"

echo "[2/4] Helm 차트 오프라인 번들링..."
if is_valid "${AIRGAP_DIR}/charts/kagent-0.9.12.tgz"; then
  echo "  -> 이미 보유: kagent-0.9.12.tgz"
elif ! helm pull oci://ghcr.io/kagent-dev/kagent/helm/kagent \
       --version 0.9.12 --destination "${AIRGAP_DIR}/charts"; then
  FAILED+=("chart:kagent-0.9.12")
fi

echo "[3/4] 컨테이너 이미지 수집 및 .tar.gz 압축..."
# 매니페스트가 요구하는 이미지는 **매니페스트에서 직접 뽑는다**. 목록을 여기에 손으로
# 적어두면 매니페스트가 올라갈 때 조용히 어긋난다 — 실제로 mlflow(v2.10.0 vs v3.14.0)·
# seaweedfs(3.60 vs 4.40)가 구버전으로 굳어 있었고 prefect·curl은 아예 빠져 있어서,
# 폐쇄망 설치 시 세 파드가 ImagePullBackOff로 죽는 상태였다(2026-07-25).
HELM_IMAGES=(
  # kagent Helm 차트가 배포하는 이미지 — 어떤 매니페스트에도 선언되지 않으므로 여기서 관리한다.
  "cr.kagent.dev/kagent-dev/kagent/controller:0.9.12"
  "cr.kagent.dev/kagent-dev/kagent/app:0.9.12"
  "cr.kagent.dev/kagent-dev/kagent/ui:0.9.12"
  "ghcr.io/kagent-dev/kagent/tools:0.2.1"
  "ghcr.io/kagent-dev/kmcp/controller:0.3.0"
  "postgres:16-alpine"
  # 보안 스캐닝 도구(docs/11) — 매니페스트 배포는 아니지만 폐쇄망에서 필요하다.
  "aquasec/trivy:latest"
)

# bash 3.2(macOS 기본)에는 mapfile이 없다 — while-read로 채운다.
MANIFEST_IMAGES=()
while IFS= read -r img; do
  [ -n "$img" ] && MANIFEST_IMAGES+=("$img")
done < <(grep -rhoE 'image: *[^ ]+' "${PROJECT_ROOT}"/scripts/k8s/*.yaml | sed 's/image: *//' | sort -u)

if [ ${#MANIFEST_IMAGES[@]} -eq 0 ]; then
  echo "  !! 매니페스트에서 이미지를 하나도 찾지 못했습니다 — 경로/형식을 확인하세요." >&2
  FAILED+=("manifest-images-empty")
fi
echo "  -> 매니페스트 유래 ${#MANIFEST_IMAGES[@]}개 + Helm/도구 ${#HELM_IMAGES[@]}개"

# 중복 제거(매니페스트와 Helm 목록이 겹칠 수 있다).
IMAGES=()
for img in "${HELM_IMAGES[@]}" "${MANIFEST_IMAGES[@]}"; do
  dup=0
  for seen in ${IMAGES[@]+"${IMAGES[@]}"}; do [ "$seen" = "$img" ] && dup=1 && break; done
  [ "$dup" -eq 0 ] && IMAGES+=("$img")
done

if ! command -v docker >/dev/null 2>&1; then
  echo "  !! docker CLI가 없어 이미지 수집을 건너뜁니다." >&2
  FAILED+=("images:docker-missing")
else
  for img in "${IMAGES[@]}"; do
    safe_name="$(echo "$img" | tr '/:' '_')"
    targz_path="${AIRGAP_DIR}/images/${safe_name}.tar.gz"
    tar_path="${AIRGAP_DIR}/images/${safe_name}.tar"

    # 이전 버전이 남긴 비압축 .tar가 있으면 .tar.gz로 전환한다.
    if is_valid "$tar_path" && ! is_valid "$targz_path"; then
      echo "  -> .tar → .tar.gz 전환: $(basename "$tar_path")"
      gzip -f "$tar_path" || FAILED+=("gzip:${safe_name}")
      continue
    fi

    if is_valid "$targz_path"; then
      echo "  -> 이미 보유: ${safe_name}.tar.gz"
      continue
    fi
    rm -f "$targz_path"

    echo "  -> 수집: $img"
    if ! docker pull "$img"; then
      FAILED+=("pull:${img}")
      continue
    fi
    if docker save "$img" | gzip > "${targz_path}.part"; then
      mv "${targz_path}.part" "$targz_path"
    else
      rm -f "${targz_path}.part"
      FAILED+=("save:${img}")
    fi
  done
fi

echo "[4/4] K8s 매니페스트 동기화..."
# CWD가 프로젝트 루트라는 보장이 없다 — 스크립트 위치 기준으로 해석한다.
# 추가만 하면 소스에서 지워지거나 옮겨진 파일이 번들에 남아 설치 때 되살아난다
# (kagent-values.yaml을 scripts/helm/으로 옮긴 뒤 실제로 재현됐다) — 매번 비우고 채운다.
rm -f "${AIRGAP_DIR}/manifests/"*.yaml
if ! cp "${PROJECT_ROOT}"/scripts/k8s/*.yaml "${AIRGAP_DIR}/manifests/"; then
  FAILED+=("manifests")
fi

echo "[5/5] 번들 무결성 목록 생성..."
# 이송(외장 매체 → 폐쇄망) 중 손상·변조를 설치 시점에 잡기 위한 목록.
# 경로는 AIRGAP_DIR 기준 상대경로여야 `shasum -c`가 그대로 검증할 수 있다.
# 작성 중인 임시 파일은 **스캔 대상 밖**에 둔다 — AIRGAP_DIR 안에 두면 find가 그것까지
# 목록에 넣고, 곧 rename으로 사라져 검증이 항상 깨진다(실측으로 확인).
MANIFEST_TMP="$(mktemp -t kubemetal-airgap-manifest)"
trap 'rm -f "$MANIFEST_TMP"' EXIT
(
  cd "${AIRGAP_DIR}" || exit 1
  find . -type f ! -name "$(basename "$MANIFEST")" ! -name '*.part' -print0 \
    | sort -z \
    | xargs -0 shasum -a 256
) > "$MANIFEST_TMP" && mv "$MANIFEST_TMP" "$MANIFEST"
echo "  -> $(wc -l < "$MANIFEST" | tr -d ' ')개 파일 해시 기록: ${MANIFEST}"

echo ""
if [ ${#FAILED[@]} -eq 0 ]; then
  echo "완료: 모든 자원 수집 성공 — ${AIRGAP_DIR}"
  exit 0
fi

echo "실패 항목 ${#FAILED[@]}건: ${FAILED[*]}" >&2
echo "부분 수집 상태 — ${AIRGAP_DIR}" >&2
exit 1
