#!/usr/bin/env bash
# ==============================================================================
# gen_sbom.sh — 릴리스 산출물용 SBOM(CycloneDX/SPDX) 생성
#
# Cargo.lock / pnpm-lock.yaml에서 trivy fs로 도출한다 — THIRD-PARTY-NOTICES.md와
# 같은 두 lockfile이 출처이므로 구성요소 범위는 동일하지만, 여기 결과는 사람이
# 아니라 도구가 읽는 기계 판독용 인벤토리다(라이선스 문자열의 사람이 읽는 출처는
# 여전히 THIRD-PARTY-NOTICES.md — trivy는 lockfile만으로 패키지별 라이선스 문자열을
# 채우지 못한다, 실측 확인됨). K8s 파드/헬름 차트로 오케스트레이션되는 third-party
# 컴포넌트(MLflow, SeaweedFS, kagent 등)는 이 스캔 범위 밖이다 — 그건 NOTICE 파일이
# 다룬다.
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
OUT_DIR="${1:-${PROJECT_ROOT}}"

command -v trivy >/dev/null 2>&1 || {
  echo "trivy가 필요합니다: brew install trivy" >&2
  exit 1
}

mkdir -p "$OUT_DIR"

trivy fs --format cyclonedx --skip-version-check \
  -o "${OUT_DIR}/sbom-cyclonedx.json" "$PROJECT_ROOT"
trivy fs --format spdx-json --skip-version-check \
  -o "${OUT_DIR}/sbom-spdx.json" "$PROJECT_ROOT"

cyclonedx_count="$(python3 -c '
import json
print(len(json.load(open("'"${OUT_DIR}"'/sbom-cyclonedx.json")).get("components", [])))')"
spdx_count="$(python3 -c '
import json
print(len(json.load(open("'"${OUT_DIR}"'/sbom-spdx.json")).get("packages", [])))')"

echo "생성: ${OUT_DIR}/sbom-cyclonedx.json (구성요소 ${cyclonedx_count}개)"
echo "생성: ${OUT_DIR}/sbom-spdx.json (패키지 ${spdx_count}개)"
