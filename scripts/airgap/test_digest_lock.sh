#!/usr/bin/env bash
# Self-contained regression scenarios for issue #5. PATH shims model Docker's
# save/load behavior: loading restores .Id but not .RepoDigests.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/airgap/lib.sh
. "${SCRIPT_DIR}/lib.sh"

TEST_DIR="$(mktemp -d -t kubemetal-digest-lock)"
trap 'rm -rf "$TEST_DIR"' EXIT
SHIM_DIR="${TEST_DIR}/bin"
BUNDLE="${TEST_DIR}/bundle"
mkdir -p "$SHIM_DIR" "$BUNDLE/images" "$BUNDLE/charts" "$BUNDLE/binaries"

export TEST_DOCKER_STATE="${TEST_DIR}/docker-state"
export TEST_DOCKER_LOG="${TEST_DIR}/docker.log"
export TEST_SOURCE_REPO_DIGEST='example.invalid/demo@sha256:registry-provenance'
export TEST_SOURCE_IMAGE_ID='sha256:expected-image-id'

cat > "${SHIM_DIR}/docker" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C
command="$1"
shift
case "$command" in
  inspect)
    # docker load loses distribution metadata, so RepoDigests is empty once loaded.
    if [ -s "$TEST_DOCKER_STATE" ]; then exit 0; fi
    printf '%s\n' "$TEST_SOURCE_REPO_DIGEST"
    ;;
  image)
    [ "$1" = inspect ] || exit 64
    shift
    format=''
    while [ "$#" -gt 1 ]; do
      case "$1" in
        --format) format="$2"; shift 2 ;;
        --format=*) format="${1#--format=}"; shift ;;
        *) shift ;;
      esac
    done
    if [[ "$format" == *'.Id'* ]]; then
      if [ -s "$TEST_DOCKER_STATE" ]; then
        cat "$TEST_DOCKER_STATE"
      elif [ -n "${TEST_PREEXISTING_ID:-}" ]; then
        printf '%s\n' "$TEST_PREEXISTING_ID"
      else
        exit 1
      fi
    fi
    ;;
  load)
    if [ "${1:-}" = -i ]; then cat "$2" > "${TEST_DOCKER_STATE}.archive"; else cat > "${TEST_DOCKER_STATE}.archive"; fi
    sed -n 's/^IMAGE_ID=//p' "${TEST_DOCKER_STATE}.archive" | head -n 1 > "$TEST_DOCKER_STATE"
    printf 'docker-load\n' >> "$TEST_DOCKER_LOG"
    printf 'Loaded image: example.invalid/demo:1.0\n'
    ;;
  pull)
    printf 'docker-pull\n' >> "$TEST_DOCKER_LOG"
    ;;
  save)
    printf 'IMAGE_ID=%s\n' "$TEST_SOURCE_IMAGE_ID"
    ;;
  *)
    echo "unexpected docker invocation: ${command} $*" >&2
    exit 64
    ;;
esac
EOF

cat > "${SHIM_DIR}/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
for arg in "$@"; do
  if [ "${next:-0}" = 1 ]; then
    cp "$TEST_ASSET_PAYLOAD" "$arg"
    exit 0
  fi
  [ "$arg" = -o ] && next=1
done
case " $* " in
  *sha256sum-arm64.txt*) printf '%s  k3s-arm64\n' "$TEST_ASSET_SHA" ;;
  *) printf '%s\n' "$TEST_ASSET_SHA" ;;
esac
EOF

cat > "${SHIM_DIR}/helm" <<'EOF'
#!/usr/bin/env bash
printf 'helm %s\n' "$*" >> "$TEST_PROVISION_LOG"
EOF

cat > "${SHIM_DIR}/kubectl" <<'EOF'
#!/usr/bin/env bash
printf 'kubectl %s\n' "$*" >> "$TEST_PROVISION_LOG"
EOF

chmod +x "${SHIM_DIR}/docker" "${SHIM_DIR}/curl" "${SHIM_DIR}/helm" "${SHIM_DIR}/kubectl"
export PATH="${SHIM_DIR}:${PATH}"
export TEST_PROVISION_LOG="${TEST_DIR}/provision.log"

payload="${TEST_DIR}/asset-payload"
dd if=/dev/urandom of="$payload" bs=1024 count=2 2>/dev/null
export TEST_ASSET_PAYLOAD="$payload"
TEST_ASSET_SHA="$(shasum -a 256 "$payload" | awk '{print $1}')"
export TEST_ASSET_SHA
cp "$payload" "$BUNDLE/binaries/k3s"
cp "$payload" "$BUNDLE/binaries/kubescape"
cp "$payload" "$BUNDLE/charts/kagent-crds-0.9.12.tgz"
cp "$payload" "$BUNDLE/charts/kagent-0.9.12.tgz"

archive_for() {
  local image="$1" image_id="$2" archive
  archive="${BUNDLE}/images/$(image_archive_name "$image").tar.gz"
  {
    printf 'IMAGE_ID=%s\n' "$image_id"
    cat "$payload"
  } | gzip > "$archive"
}

records="${TEST_DIR}/records"
: > "$records"
first_image=''
while IFS= read -r image; do
  [ -n "$first_image" ] || first_image="$image"
  archive_for "$image" "$TEST_SOURCE_IMAGE_ID"
  printf '%s %s %s\n' "$image" "$TEST_SOURCE_REPO_DIGEST" "$TEST_SOURCE_IMAGE_ID" >> "$records"
done < <(
  {
    read_image_list "${SCRIPT_DIR}/images-helm.txt"
    manifest_images "$(cd "${SCRIPT_DIR}/../.." && pwd)"
  } | LC_ALL=C sort -u
)
printf '\n' >> "$records"
write_digest_lock "$records" "${BUNDLE}/digests.lock"

# 5c022a1: exercise the downloader itself with every image archive cached. It
# must preserve all three lock fields and never re-pull an image to invent data.
AIRGAP_DIR="$BUNDLE" "${SCRIPT_DIR}/download_airgap_bundle.sh" > "${TEST_DIR}/download.out" 2>&1
grep -q '이미 보유:' "${TEST_DIR}/download.out"
grep -qx "${first_image} ${TEST_SOURCE_REPO_DIGEST} ${TEST_SOURCE_IMAGE_ID}" "${BUNDLE}/digests.lock"
if [ -f "$TEST_DOCKER_LOG" ] && grep -q docker-pull "$TEST_DOCKER_LOG"; then
  echo 'cached downloader unexpectedly pulled an image' >&2
  exit 1
fi
echo 'PASS downloader cache reuse preserves repository provenance and image ID'

write_manifest() {
  local manifest_tmp="${TEST_DIR}/manifest.part"
  (
    cd "$BUNDLE"
    find . -type f ! -name manifest.sha256 -print0 | sort -z | xargs -0 shasum -a 256 > "$manifest_tmp"
  )
  mv "$manifest_tmp" "${BUNDLE}/manifest.sha256"
}

write_manifest
rm -f "$TEST_DOCKER_STATE" "$TEST_DOCKER_LOG" "$TEST_PROVISION_LOG"
if ! AIRGAP_DIR="$BUNDLE" KUBE_CONTEXT=test "${SCRIPT_DIR}/install_from_airgap.sh" > "${TEST_DIR}/clean-install.out" 2>&1; then
  cat "${TEST_DIR}/clean-install.out" >&2
  exit 1
fi
grep -q '프로비저닝 성공' "${TEST_DIR}/clean-install.out"
if docker inspect --format='{{range .RepoDigests}}{{println .}}{{end}}' example.invalid/demo:1.0 | grep -q .; then
  echo 'docker shim failed to model RepoDigests loss after load' >&2
  exit 1
fi
echo 'PASS clean load accepts matching image ID without RepoDigest'

cp "${BUNDLE}/digests.lock" "${TEST_DIR}/digests.lock.valid"
printf 'malformed lock\n' >> "${BUNDLE}/digests.lock"
write_manifest
rm -f "$TEST_DOCKER_STATE" "$TEST_DOCKER_LOG" "$TEST_PROVISION_LOG"
if AIRGAP_DIR="$BUNDLE" KUBE_CONTEXT=test "${SCRIPT_DIR}/install_from_airgap.sh" > "${TEST_DIR}/malformed.out" 2>&1; then
  echo 'expected malformed lock rejection, but install succeeded' >&2
  exit 1
fi
grep -q 'digests.lock 항목이 없거나 형식이 잘못되었습니다' "${TEST_DIR}/malformed.out"
if [ -e "$TEST_DOCKER_LOG" ]; then
  echo 'malformed lock reached docker load' >&2
  exit 1
fi
mv "${TEST_DIR}/digests.lock.valid" "${BUNDLE}/digests.lock"
echo 'PASS install rejects malformed lock before load'

archive_for "$first_image" 'sha256:tampered-image-id'
write_manifest
rm -f "$TEST_DOCKER_STATE" "$TEST_PROVISION_LOG"
if AIRGAP_DIR="$BUNDLE" KUBE_CONTEXT=test "${SCRIPT_DIR}/install_from_airgap.sh" > "${TEST_DIR}/tamper.out" 2>&1; then
  echo 'expected image-ID tamper rejection, but install succeeded' >&2
  exit 1
fi
grep -q 'image ID 불일치' "${TEST_DIR}/tamper.out"
if [ -e "$TEST_PROVISION_LOG" ]; then
  echo 'tampered archive reached provisioning' >&2
  exit 1
fi
echo 'PASS install rejects archive with a different image ID before provisioning'

rm -f "$TEST_DOCKER_STATE" "$TEST_DOCKER_LOG" "$TEST_PROVISION_LOG"
if TEST_PREEXISTING_ID='sha256:stale-cache-id' AIRGAP_DIR="$BUNDLE" KUBE_CONTEXT=test \
  "${SCRIPT_DIR}/install_from_airgap.sh" > "${TEST_DIR}/cache.out" 2>&1; then
  echo 'expected stale daemon cache rejection, but install succeeded' >&2
  exit 1
fi
grep -q 'load 전 daemon cache image ID 불일치' "${TEST_DIR}/cache.out"
if [ -e "$TEST_DOCKER_LOG" ]; then
  echo 'stale daemon cache was loaded despite fail-closed guard' >&2
  exit 1
fi
echo 'PASS install rejects stale daemon cache before load'

rm -f "$TEST_DOCKER_STATE" "$TEST_PROVISION_LOG"
AIRGAP_ALLOW_UNLOCKED=1 TEST_PREEXISTING_ID='sha256:stale-cache-id' AIRGAP_DIR="$BUNDLE" KUBE_CONTEXT=test \
  "${SCRIPT_DIR}/install_from_airgap.sh" > "${TEST_DIR}/opt-out.out" 2>&1
grep -q 'AIRGAP_ALLOW_UNLOCKED=1' "${TEST_DIR}/opt-out.out"
grep -q '프로비저닝 성공' "${TEST_DIR}/opt-out.out"
echo 'PASS explicit unlocked opt-out permits the intentionally unverified install'
