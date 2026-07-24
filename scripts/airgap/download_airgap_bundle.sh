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

is_valid() {
  local f="$1"
  [ -f "$f" ] || return 1
  [ "$(wc -c < "$f" | tr -d ' ')" -ge "$MIN_VALID_BYTES" ]
}

fetch_binary() {
  local name="$1" url="$2"
  local dest="${AIRGAP_DIR}/binaries/${name}"
  if is_valid "$dest"; then
    echo "  -> 이미 보유: ${name}"
    return 0
  fi
  if [ -f "$dest" ]; then
    echo "  -> 손상 파일 폐기 후 재수집: ${name} ($(wc -c < "$dest" | tr -d ' ')B)"
    rm -f "$dest"
  fi
  echo "  -> 다운로드: ${name}"
  # -f: HTTP 에러를 실패로 처리(에러 페이지를 파일로 저장하지 않는다)
  if curl -fsSL "$url" -o "${dest}.part"; then
    mv "${dest}.part" "$dest"
    chmod +x "$dest"
  else
    rm -f "${dest}.part"
    FAILED+=("binary:${name}")
  fi
}

echo "[1/4] K3s & Kubescape 바이너리 수집..."
fetch_binary "k3s" "https://github.com/k3s-io/k3s/releases/download/v1.28.2%2Bk3s1/k3s"
fetch_binary "kubescape" "https://github.com/kubescape/kubescape/releases/download/v3.0.0/kubescape-macos-arm64"

echo "[2/4] Helm 차트 오프라인 번들링..."
if is_valid "${AIRGAP_DIR}/charts/kagent-0.9.12.tgz"; then
  echo "  -> 이미 보유: kagent-0.9.12.tgz"
elif ! helm pull oci://ghcr.io/kagent-dev/kagent/helm/kagent \
       --version 0.9.12 --destination "${AIRGAP_DIR}/charts"; then
  FAILED+=("chart:kagent-0.9.12")
fi

echo "[3/4] 컨테이너 이미지 수집 및 .tar.gz 압축..."
IMAGES=(
  "cr.kagent.dev/kagent-dev/kagent/controller:0.9.12"
  "cr.kagent.dev/kagent-dev/kagent/app:0.9.12"
  "cr.kagent.dev/kagent-dev/kagent/ui:0.9.12"
  "ghcr.io/kagent-dev/kagent/tools:0.2.1"
  "ghcr.io/kagent-dev/kmcp/controller:0.3.0"
  "ghcr.io/mlflow/mlflow:v2.10.0"
  "chrislusf/seaweedfs:3.60"
  "postgres:16-alpine"
  "aquasec/trivy:latest"
  "nginx:alpine"
)

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

echo "[4/4] K8s 매니페스트 복사..."
# CWD가 프로젝트 루트라는 보장이 없다 — 스크립트 위치 기준으로 해석한다.
if ! cp "${PROJECT_ROOT}"/scripts/k8s/*.yaml "${AIRGAP_DIR}/manifests/"; then
  FAILED+=("manifests")
fi

echo ""
if [ ${#FAILED[@]} -eq 0 ]; then
  echo "완료: 모든 자원 수집 성공 — ${AIRGAP_DIR}"
  exit 0
fi

echo "실패 항목 ${#FAILED[@]}건: ${FAILED[*]}" >&2
echo "부분 수집 상태 — ${AIRGAP_DIR}" >&2
exit 1
