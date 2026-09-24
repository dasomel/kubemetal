#!/usr/bin/env bash
# Opt-in evidence for an existing bundle. Never pull or load images.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/airgap/lib.sh
. "${SCRIPT_DIR}/lib.sh"
SYFT="$(resolve_cli_path syft)"
PYTHON="$(resolve_cli_path python3)"
AIRGAP_DIR="${AIRGAP_DIR:-${HOME}/.kubemetal/airgap}"
AIRGAP_DIR="$(cd "$AIRGAP_DIR" && pwd)"
WORK="$(mktemp -d "${AIRGAP_DIR}/.sbom.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
mkdir "$WORK/sbom"
"$PYTHON" "$SCRIPT_DIR/sbom.py" lock "$AIRGAP_DIR/digests.lock" > "$WORK/lock"
: > "$WORK/paths"

# D-a (#98): syft is required only here; the cost is a separate operator step.
# Escape hatch: omit this step, never claim a requested scan succeeded without it.
# Disable update/enrichment requests even if the user's syft config enables them.
# https://oss.anchore.com/docs/reference/syft/configuration/
export SYFT_CHECK_FOR_APP_UPDATE=false
export SYFT_GOLANG_SEARCH_REMOTE_LICENSES=false SYFT_GOLANG_USE_PACKAGES_LIB=false
export SYFT_JAVA_USE_NETWORK=false SYFT_JAVA_RESOLVE_TRANSITIVE_DEPENDENCIES=false
export SYFT_JAVASCRIPT_SEARCH_REMOTE_LICENSES=false SYFT_PYTHON_SEARCH_REMOTE_LICENSES=false
export SYFT_GOLANG_SEARCH_LOCAL_MOD_CACHE_LICENSES=false SYFT_GOLANG_SEARCH_LOCAL_VENDOR_LICENSES=false
export SYFT_JAVA_USE_MAVEN_LOCAL_REPOSITORY=false

while read -r image repo_digest image_id; do
  name="$(image_archive_name "$image")"
  compressed="$AIRGAP_DIR/images/$name.tar.gz"
  archive="$AIRGAP_DIR/images/$name.tar"
  if [ -e "$compressed" ] && [ -e "$archive" ]; then
    echo "SBOM: 중복 archive — $image" >&2
    exit 1
  fi
  if [ -e "$compressed" ]; then
    gzip -dc "$compressed" > "$WORK/source.tar"
    archive="$WORK/source.tar"
  fi
  # D25: registry provenance is not in docker save. Bind the actual config bytes
  # to the lock's image ID; keep an unverified RepoDigest honestly unverified.
  "$PYTHON" "$SCRIPT_DIR/sbom.py" archive "$archive" "$image_id"
  echo "SBOM 생성: $image ($repo_digest; $image_id)"
  "$SYFT" scan "docker-archive:$archive" -o spdx-json > "$WORK/sbom/$name.spdx.json"
  printf '%s\t%s\n' "$image" "sbom/$name.spdx.json" >> "$WORK/paths"
done < "$WORK/lock"

"$PYTHON" "$SCRIPT_DIR/sbom.py" assemble "$AIRGAP_DIR/digests.lock" "$WORK"
"$PYTHON" "$SCRIPT_DIR/sbom.py" verify "$AIRGAP_DIR/digests.lock" "$WORK/sbom"
"$PYTHON" "$SCRIPT_DIR/sbom.py" publish "$AIRGAP_DIR" "$WORK"
echo "SBOM 증거 생성 완료: $AIRGAP_DIR/sbom/manifest.json (라이선스 요약: sbom/licenses.json)"
