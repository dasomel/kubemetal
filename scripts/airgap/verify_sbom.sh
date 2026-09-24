#!/usr/bin/env bash
# Offline verification needs Python's standard library, never syft or a daemon.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/airgap/lib.sh
. "${SCRIPT_DIR}/lib.sh"
PYTHON="$(resolve_cli_path python3)"
AIRGAP_DIR="${AIRGAP_DIR:-${HOME}/.kubemetal/airgap}"
"$PYTHON" "$SCRIPT_DIR/sbom.py" verify "$AIRGAP_DIR/digests.lock" "$AIRGAP_DIR/sbom"
