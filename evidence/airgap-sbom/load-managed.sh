#!/usr/bin/env bash
# Local #98 probe setup: import the already collected archives into KubeMetal's VM.
set -euo pipefail
bundle="${HOME}/.kubemetal/airgap-sbom-evidence-20261008"
context="colima-kubemetal"
docker --context "$context" info >/dev/null
for archive in "$bundle"/images/*.tar.gz; do
  [ -f "$archive" ] || { echo "No image archives in $bundle" >&2; exit 1; }
  echo "Loading $(basename "$archive") into $context"
  docker --context "$context" load -i "$archive"
done
