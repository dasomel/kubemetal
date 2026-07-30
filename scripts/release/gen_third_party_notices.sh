#!/usr/bin/env bash
# ==============================================================================
# gen_third_party_notices.sh — 배포 바이너리 동봉용 THIRD-PARTY-NOTICES.md 생성
#
# 목록은 cargo metadata와 pnpm licenses에서 도출한다 — 수기 목록은 lockfile과
# 반드시 어긋나므로 두지 않는다. 릴리스 워크플로가 zip에 함께 담는다.
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
OUT="${1:-${PROJECT_ROOT}/THIRD-PARTY-NOTICES.md}"

{
  echo "# Third-Party Notices"
  echo
  echo "KubeMetal binary distributions include the following third-party components,"
  echo "each under its own license. Full license texts are available in each project's"
  echo "repository (see crates.io / npmjs.com for the exact version listed)."
  echo
  echo "## Rust crates"
  echo
  cargo metadata --manifest-path "${PROJECT_ROOT}/src-tauri/Cargo.toml" --format-version 1 \
    | python3 -c '
import json, sys
m = json.load(sys.stdin)
for p in sorted(m["packages"], key=lambda p: p["name"]):
    if p["name"] == "kubemetal":
        continue
    print("- {} {} — {}".format(p["name"], p["version"], p.get("license") or "see repository"))'
  echo
  echo "## npm packages (production)"
  echo
  (cd "${PROJECT_ROOT}" && pnpm licenses list --prod --json) \
    | python3 -c '
import json, sys
data = json.load(sys.stdin)
rows = []
for lic, pkgs in data.items():
    for p in pkgs:
        rows.append((p["name"], ", ".join(p.get("versions", [])), lic))
for name, vers, lic in sorted(rows):
    print("- {} {} — {}".format(name, vers, lic))'
} > "$OUT"

count="$(grep -c '^- ' "$OUT")"
echo "생성: ${OUT} (구성요소 ${count}개)"
