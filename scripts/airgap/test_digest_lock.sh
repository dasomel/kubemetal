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
TEST_SOURCE_REPO_DIGEST="example.invalid/demo@sha256:$(printf 'a%.0s' $(seq 1 64))"
TEST_SOURCE_IMAGE_ID="sha256:$(python3 -c 'import hashlib,json; f="".join(hashlib.sha256(str(i).encode()).hexdigest() for i in range(128)); print(hashlib.sha256(json.dumps({"architecture":"arm64","os":"linux","rootfs":{"type":"layers","diff_ids":[]},"marker":"source","filler":f},separators=(",", ":")).encode()).hexdigest())')"
TEST_CHANGED_IMAGE_ID="sha256:$(python3 -c 'import hashlib,json; f="".join(hashlib.sha256(str(i).encode()).hexdigest() for i in range(128)); print(hashlib.sha256(json.dumps({"architecture":"arm64","os":"linux","rootfs":{"type":"layers","diff_ids":[]},"marker":"changed","filler":f},separators=(",", ":")).encode()).hexdigest())')"
export TEST_SOURCE_REPO_DIGEST TEST_SOURCE_IMAGE_ID TEST_CHANGED_IMAGE_ID

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
    python3 - "${TEST_DOCKER_STATE}.archive" > "$TEST_DOCKER_STATE" <<'PY'
import hashlib
import json
import sys
import tarfile
with tarfile.open(sys.argv[1]) as archive:
    manifest = json.load(archive.extractfile("manifest.json"))
    config = archive.extractfile(manifest[0]["Config"]).read()
print("sha256:" + hashlib.sha256(config).hexdigest())
PY
    printf 'docker-load\n' >> "$TEST_DOCKER_LOG"
    printf 'Loaded image: example.invalid/demo:1.0\n'
    ;;
  pull)
    printf 'docker-pull\n' >> "$TEST_DOCKER_LOG"
    ;;
  save)
    variant=source
    [ "${TEST_PREEXISTING_ID:-$TEST_SOURCE_IMAGE_ID}" = "$TEST_CHANGED_IMAGE_ID" ] && variant=changed
    python3 - "$variant" "${@: -1}" <<'PY'
import hashlib
import io
import json
import sys
import tarfile
variant, image = sys.argv[1:]
filler = "".join(hashlib.sha256(str(i).encode()).hexdigest() for i in range(128))
config = json.dumps({"architecture": "arm64", "os": "linux", "rootfs": {"type": "layers", "diff_ids": []}, "marker": variant, "filler": filler}, separators=(",", ":")).encode()
config_path = hashlib.sha256(config).hexdigest() + ".json"
manifest = json.dumps([{"Config": config_path, "RepoTags": [image], "Layers": []}]).encode()
with tarfile.open(fileobj=sys.stdout.buffer, mode="w|", format=tarfile.USTAR_FORMAT) as archive:
    for name, data in ((config_path, config), ("manifest.json", manifest)):
        member = tarfile.TarInfo(name)
        member.size = len(data)
        archive.addfile(member, io.BytesIO(data))
PY
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

cat > "${SHIM_DIR}/syft" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[ "$1" = scan ] && [[ "$2" == docker-archive:* ]] && [ -s "${2#docker-archive:}" ]
cat <<'JSON'
{"spdxVersion":"SPDX-2.3","SPDXID":"SPDXRef-DOCUMENT","name":"fixture","dataLicense":"CC0-1.0","documentNamespace":"https://example.invalid/test","creationInfo":{"creators":["Tool: syft-test"],"created":"2026-09-29T00:00:00Z"},"packages":[{"SPDXID":"SPDXRef-a","name":"fixture","licenseDeclared":"MIT"}]}
JSON
EOF

cat > "${SHIM_DIR}/helm" <<'EOF'
#!/usr/bin/env bash
printf 'helm %s\n' "$*" >> "$TEST_PROVISION_LOG"
EOF

cat > "${SHIM_DIR}/kubectl" <<'EOF'
#!/usr/bin/env bash
printf 'kubectl %s\n' "$*" >> "$TEST_PROVISION_LOG"
EOF

chmod +x "${SHIM_DIR}/docker" "${SHIM_DIR}/curl" "${SHIM_DIR}/syft" "${SHIM_DIR}/helm" "${SHIM_DIR}/kubectl"
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
  local image="$1" variant_id="$2" archive variant
  archive="${BUNDLE}/images/$(image_archive_name "$image").tar.gz"
  variant=source
  [ "$variant_id" = "$TEST_CHANGED_IMAGE_ID" ] && variant=changed
  [ "$variant_id" = 'sha256:tampered-image-id' ] && variant=tampered
  python3 - "$image" "$variant" <<'PY' | gzip > "$archive"
import hashlib
import io
import json
import sys
import tarfile
image, variant = sys.argv[1:]
filler = "".join(hashlib.sha256(str(i).encode()).hexdigest() for i in range(128))
config = json.dumps({"architecture": "arm64", "os": "linux", "rootfs": {"type": "layers", "diff_ids": []}, "marker": variant, "filler": filler}, separators=(",", ":")).encode()
config_path = hashlib.sha256(config).hexdigest() + ".json"
manifest = json.dumps([{"Config": config_path, "RepoTags": [image], "Layers": []}]).encode()
with tarfile.open(fileobj=sys.stdout.buffer, mode="w|", format=tarfile.USTAR_FORMAT) as archive:
    for name, data in ((config_path, config), ("manifest.json", manifest)):
        member = tarfile.TarInfo(name)
        member.size = len(data)
        archive.addfile(member, io.BytesIO(data))
PY
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
if ! AIRGAP_DIR="$BUNDLE" "${SCRIPT_DIR}/download_airgap_bundle.sh" > "${TEST_DIR}/download.out" 2>&1; then
  cat "${TEST_DIR}/download.out" >&2
  exit 1
fi
grep -q '이미 보유:' "${TEST_DIR}/download.out"
grep -qx "${first_image} ${TEST_SOURCE_REPO_DIGEST} ${TEST_SOURCE_IMAGE_ID}" "${BUNDLE}/digests.lock"
if [ -f "$TEST_DOCKER_LOG" ] && grep -q docker-pull "$TEST_DOCKER_LOG"; then
  echo 'cached downloader unexpectedly pulled an image' >&2
  exit 1
fi
echo 'PASS downloader cache reuse preserves repository provenance and image ID'

# #127: fault injection affects only the final transport manifest, leaving the
# earlier SBOM/digest checks real. No downloads or Docker daemon are involved.
export TEST_REAL_SHASUM="$(command -v shasum)"
export TEST_REAL_MV="$(command -v mv)"
export TEST_REAL_MKTEMP="$(command -v mktemp)"
cat > "${SHIM_DIR}/shasum" <<'EOF'
#!/usr/bin/env bash
if [ "${TEST_MANIFEST_FAILURE:-}" = hash ] && [ "$PWD" = "$TEST_MANIFEST_BUNDLE" ]; then exit 42; fi
if [ "${TEST_MANIFEST_FAILURE:-}" = verify ] && [ "${3:-}" = -c ] && [ "${4:-}" = manifest.sha256 ]; then exit 42; fi
exec "$TEST_REAL_SHASUM" "$@"
EOF
cat > "${SHIM_DIR}/mv" <<'EOF'
#!/usr/bin/env bash
if [ "${TEST_MANIFEST_FAILURE:-}" = publish ] && [ "${2:-}" = "$TEST_MANIFEST_BUNDLE/manifest.sha256" ]; then exit 42; fi
if [ "${TEST_MANIFEST_FAILURE:-}" = corrupt ] && [ "${2:-}" = "$TEST_MANIFEST_BUNDLE/manifest.sha256" ]; then
  "$TEST_REAL_MV" "$@" || exit "$?"
  printf 'tampered\n' >> "$TEST_MANIFEST_BUNDLE/charts/kagent-0.9.12.tgz"
  exit 0
fi
exec "$TEST_REAL_MV" "$@"
EOF
cat > "${SHIM_DIR}/mktemp" <<'EOF'
#!/usr/bin/env bash
if [ "${TEST_MANIFEST_FAILURE:-}" = temp ] && [ "${2:-}" = kubemetal-airgap-manifest ]; then exit 42; fi
exec "$TEST_REAL_MKTEMP" "$@"
EOF
chmod +x "$SHIM_DIR/shasum" "$SHIM_DIR/mv" "$SHIM_DIR/mktemp"
export TEST_MANIFEST_BUNDLE="$BUNDLE"
for failure in temp hash publish verify corrupt; do
  if TEST_MANIFEST_FAILURE="$failure" AIRGAP_DIR="$BUNDLE" bash "$SCRIPT_DIR/download_airgap_bundle.sh" > "$TEST_DIR/manifest-$failure.out" 2>&1; then
    echo "FAIL downloader accepted manifest $failure failure" >&2
    exit 1
  fi
  grep -q '실패 항목.*manifest-' "$TEST_DIR/manifest-$failure.out"
  if grep -q '완료: 모든 자원 수집 성공' "$TEST_DIR/manifest-$failure.out"; then
    echo "FAIL downloader printed success after manifest $failure failure" >&2
    exit 1
  fi
  echo "PASS downloader rejects manifest $failure failure"
done
AIRGAP_DIR="$BUNDLE" bash "$SCRIPT_DIR/download_airgap_bundle.sh" > "$TEST_DIR/manifest-success.out" 2>&1
(cd "$BUNDLE" && shasum -a 256 -c manifest.sha256) > "$TEST_DIR/manifest-verified.out"
grep -q '완료: 모든 자원 수집 성공' "$TEST_DIR/manifest-success.out"
echo 'PASS downloader success publishes a self-verifying manifest'

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
grep -q 'SBOM 증거 검증에 실패했습니다' "${TEST_DIR}/malformed.out"
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

rm -f "$TEST_DOCKER_STATE" "$TEST_DOCKER_LOG" "$TEST_PROVISION_LOG"
legacy_flag="AIRGAP""_ALLOW_UNLOCKED"
if env "$legacy_flag=1" TEST_PREEXISTING_ID='sha256:stale-cache-id' AIRGAP_DIR="$BUNDLE" KUBE_CONTEXT=test \
  "${SCRIPT_DIR}/install_from_airgap.sh" > "${TEST_DIR}/former-opt-out.out" 2>&1; then
  echo 'expected former environment opt-out to reject stale daemon cache, but install succeeded' >&2
  exit 1
fi
grep -q 'load 전 daemon cache image ID 불일치' "${TEST_DIR}/former-opt-out.out"
if [ -e "$TEST_DOCKER_LOG" ] || [ -e "$TEST_PROVISION_LOG" ]; then
  echo 'former environment opt-out reached image load or provisioning' >&2
  exit 1
fi
echo 'PASS former environment opt-out cannot bypass stale-cache image ID verification'

rm -f "$TEST_DOCKER_STATE" "$TEST_DOCKER_LOG" "$TEST_PROVISION_LOG"
mv "${BUNDLE}/digests.lock" "${TEST_DIR}/digests.lock.saved"
write_manifest
if env "$legacy_flag=1" TEST_PREEXISTING_ID='sha256:stale-cache-id' AIRGAP_DIR="$BUNDLE" KUBE_CONTEXT=test \
  "${SCRIPT_DIR}/install_from_airgap.sh" > "${TEST_DIR}/missing-lock.out" 2>&1; then
  echo 'expected missing digest lock rejection, but install succeeded' >&2
  exit 1
fi
grep -q 'digests.lock이 없어 필수 SBOM을 이미지 ID에 결속해 검증할 수 없습니다' "${TEST_DIR}/missing-lock.out"
if [ -e "$TEST_DOCKER_LOG" ] || [ -e "$TEST_PROVISION_LOG" ]; then
  echo 'missing digest lock reached image load or provisioning' >&2
  exit 1
fi
mv "${TEST_DIR}/digests.lock.saved" "${BUNDLE}/digests.lock"
write_manifest
echo 'PASS missing digest lock fails closed even with the former environment opt-out'

# Re-collecting a bundle whose digests actually changed must not let stale sbom/
# evidence ride along into the new manifest.sha256 (#98 follow-up): it would only
# surface at install time as "SBOM digest differs". Isolated bundle so it never
# touches $BUNDLE used above.
STALE_DIR="${TEST_DIR}/bundle-stale-sbom"
mkdir -p "$STALE_DIR/images" "$STALE_DIR/charts" "$STALE_DIR/binaries" "$STALE_DIR/manifests"
cp "$payload" "$STALE_DIR/binaries/k3s"
cp "$payload" "$STALE_DIR/binaries/kubescape"
cp "$payload" "$STALE_DIR/charts/kagent-crds-0.9.12.tgz"
cp "$payload" "$STALE_DIR/charts/kagent-0.9.12.tgz"

rm -f "$TEST_DOCKER_STATE" "$TEST_DOCKER_LOG" "$TEST_PROVISION_LOG"
AIRGAP_DIR="$STALE_DIR" TEST_PREEXISTING_ID="$TEST_SOURCE_IMAGE_ID" \
  "${SCRIPT_DIR}/download_airgap_bundle.sh" > "${TEST_DIR}/stale-run1.out" 2>&1
grep -q '완료: 모든 자원 수집 성공' "${TEST_DIR}/stale-run1.out" || {
  cat "${TEST_DIR}/stale-run1.out" >&2
  echo 'initial stale-sbom fixture collection failed' >&2
  exit 1
}
AIRGAP_DIR="$STALE_DIR" bash "${SCRIPT_DIR}/verify_sbom.sh" >/dev/null

# Fabricate SBOM evidence for this state, as `make airgap-sbom` would have, then
# fold it into manifest.sha256 the same way the downloader does at the end of a run.
mkdir -p "${STALE_DIR}/sbom"
echo '{"schema_version":1,"images":[]}' > "${STALE_DIR}/sbom/manifest.json"
echo '{"informational_only":true,"licenses":[]}' > "${STALE_DIR}/sbom/licenses.json"
(
  cd "$STALE_DIR" || exit 1
  find . -type f ! -name manifest.sha256 -print0 | sort -z | xargs -0 shasum -a 256
) > "${TEST_DIR}/stale-manifest.part"
mv "${TEST_DIR}/stale-manifest.part" "${STALE_DIR}/manifest.sha256"
grep -q './sbom/manifest.json' "${STALE_DIR}/manifest.sha256"

# Force a real digest change: drop the cached archives so every image is re-pulled
# with a different (fake) image ID.
rm -f "$TEST_DOCKER_STATE" "$TEST_DOCKER_LOG" "$TEST_PROVISION_LOG"
rm -rf "${STALE_DIR}/images"
mkdir -p "${STALE_DIR}/images"
AIRGAP_DIR="$STALE_DIR" TEST_PREEXISTING_ID="$TEST_CHANGED_IMAGE_ID" \
  "${SCRIPT_DIR}/download_airgap_bundle.sh" > "${TEST_DIR}/stale-run2.out" 2>&1
grep -q '완료: 모든 자원 수집 성공' "${TEST_DIR}/stale-run2.out" || {
  cat "${TEST_DIR}/stale-run2.out" >&2
  echo 'second stale-sbom fixture collection failed' >&2
  exit 1
}
grep -qi 'sbom' "${TEST_DIR}/stale-run2.out"
grep -q '현재 digest로 다시 생성합니다' "${TEST_DIR}/stale-run2.out"
if [ ! -f "${STALE_DIR}/sbom/manifest.json" ]; then
  echo 'required SBOM evidence was not regenerated after digest change' >&2
  exit 1
fi
AIRGAP_DIR="$STALE_DIR" bash "${SCRIPT_DIR}/verify_sbom.sh" >/dev/null
if ! grep -q './sbom/manifest.json' "${STALE_DIR}/manifest.sha256"; then
  echo 'manifest.sha256 does not cover regenerated SBOM evidence' >&2
  exit 1
fi
echo 'PASS re-collecting with changed digests regenerates and hashes required SBOM evidence'
