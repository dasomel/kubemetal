#!/usr/bin/env bash
# Offline verification needs Python's standard library, never syft or a daemon.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/airgap/lib.sh
. "${SCRIPT_DIR}/lib.sh"
PYTHON="$(resolve_cli_path python3)"
AIRGAP_DIR="${AIRGAP_DIR:-${HOME}/.kubemetal/airgap}"
if [ ! -f "$AIRGAP_DIR/sbom/manifest.json" ]; then
  echo "SBOM 오류: 필수 SBOM manifest가 없습니다: $AIRGAP_DIR/sbom/manifest.json" >&2
  exit 1
fi
"$PYTHON" "$SCRIPT_DIR/sbom.py" verify "$AIRGAP_DIR/digests.lock" "$AIRGAP_DIR/sbom"
