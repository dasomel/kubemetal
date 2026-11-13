#!/usr/bin/env bash
set -euo pipefail
# D22: keep downloads outside the checkout; disk cost is temporary, rerun on failure.
# Usage: probe.sh OUTPUT_DIR [OLD_VERSION [NEW_VERSION]]
if [[ $# -lt 1 || $# -gt 3 ]]; then
  printf 'Usage: %s OUTPUT_DIR [OLD_VERSION [NEW_VERSION]]\n' "$0" >&2
  exit 2
fi
helper="${BASH_SOURCE[0]%/*}/diff_crds.py"
work=$(mktemp -d /private/tmp/kagent-chart-probe.XXXXXXXX)
trap 'python3 -B -c '\''import shutil,sys; shutil.rmtree(sys.argv[1])'\'' "$work"' EXIT
python3 -B "$helper" probe "$1" "$work" "${2:-0.9.12}" "${3:-1.0.0-alpha17}"
