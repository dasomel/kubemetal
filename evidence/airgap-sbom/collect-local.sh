#!/usr/bin/env bash
# Evidence-only collector: Docker Desktop; no Colima or Kubernetes mutations.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
. "$ROOT/scripts/airgap/lib.sh"
COLIMA_CONTEXT=desktop-linux
export DOCKER_CONTEXT=desktop-linux
BUNDLE="${HOME}/.kubemetal/airgap-sbom-evidence-20261008"
mkdir -p "$BUNDLE/images"
records="$(mktemp)"
trap 'rm -f "$records"' EXIT
{ manifest_images "$ROOT"; read_image_list "$ROOT/scripts/airgap/images-helm.txt"; } | sort -u > "$BUNDLE/required-images.txt"
while IFS= read -r image; do
  echo "Collecting $image with Docker Desktop"
  docker --context desktop-linux pull --platform linux/arm64 "$image"
  repo="$(image_repo_digest_lock_value "$image")"
  identity="$(image_id_lock_value "$image")"
  archive="$BUNDLE/images/$(image_archive_name "$image").tar.gz"
  docker --context desktop-linux save "$image" | gzip > "$archive.part"
  mv "$archive.part" "$archive"
  printf '%s %s %s\n' "$image" "$repo" "$identity" >> "$records"
done < "$BUNDLE/required-images.txt"
write_digest_lock "$records" "$BUNDLE/digests.lock"
AIRGAP_DIR="$BUNDLE" make -C "$ROOT" airgap-sbom
AIRGAP_DIR="$BUNDLE" make -C "$ROOT" verify-airgap-sbom
