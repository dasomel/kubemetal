#!/usr/bin/env bash
# Issue #98: disposable docker-save fixtures and PATH shims; no daemon/network.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/airgap/lib.sh
. "${SCRIPT_DIR}/lib.sh"
TEST_DIR="$(mktemp -d -t kubemetal-sbom)"
trap 'rm -rf "$TEST_DIR"' EXIT
export AIRGAP_DIR="${TEST_DIR}/bundle with spaces"
export TEST_CALLS="${TEST_DIR}/calls"
mkdir -p "$TEST_DIR/bin" "$TEST_DIR/no-syft" "$AIRGAP_DIR/images" \
  "$AIRGAP_DIR/charts" "$AIRGAP_DIR/manifests"

python3 - "$AIRGAP_DIR" <<'PY'
import hashlib
import io
import json
from pathlib import Path
import sys
import tarfile

bundle = Path(sys.argv[1])
records = []
for index in range(2):
    ref = f"example.invalid/demo{index}:1.0"
    config = json.dumps({"architecture": "arm64", "os": "linux", "index": index}).encode()
    digest = hashlib.sha256(config).hexdigest()
    config_path = f"{digest}.json" if index == 0 else f"blobs/sha256/{digest}"
    repo = f"example.invalid/demo{index}@sha256:{'a' * 64}" if index == 0 else "unverified"
    records.append(f"{ref} {repo} sha256:{digest}\n")
    suffix, mode = (".tar.gz", "w:gz") if index == 0 else (".tar", "w")
    archive = bundle / "images" / (ref.replace("/", "_").replace(":", "_") + suffix)
    manifest = json.dumps([{"Config": config_path, "RepoTags": [ref], "Layers": []}]).encode()
    with tarfile.open(archive, mode) as tar:
        for name, data in [(config_path, config), ("manifest.json", manifest)]:
            info = tarfile.TarInfo(name)
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
(bundle / "digests.lock").write_text("".join(records))
for chart in ["kagent", "kagent-crds"]:
    (bundle / "charts" / f"{chart}-0.9.12.tgz").write_text("fixture")
PY

cat > "$TEST_DIR/bin/syft" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
[ "$1" = scan ] && [[ "$2" == docker-archive:* ]] && [ "$3" = -o ] && [ "$4" = spdx-json ]
[ -s "${2#docker-archive:}" ]
[ "${SYFT_CHECK_FOR_APP_UPDATE:-}" = false ]
printf 'syft\n' >> "$TEST_CALLS"
if [ "${TEST_SYFT_EMPTY:-0}" = 1 ] && [ "$(wc -l < "$TEST_CALLS" | tr -d ' ')" -eq 2 ]; then exit 0; fi
if [ "${TEST_SYFT_ERROR:-0}" = 1 ]; then exit 42; fi
cat <<'JSON'
{"spdxVersion":"SPDX-2.3","SPDXID":"SPDXRef-DOCUMENT","name":"fixture","dataLicense":"CC0-1.0","documentNamespace":"https://example.invalid/fixture","creationInfo":{"creators":["Tool: syft-stub"],"created":"2026-09-24T00:00:00Z"},"packages":[{"SPDXID":"SPDXRef-a","name":"a","licenseDeclared":"MIT"},{"SPDXID":"SPDXRef-b","name":"b","licenseDeclared":"GPL-2.0-only OR LGPL-2.1-only"},{"SPDXID":"SPDXRef-c","name":"c","licenseDeclared":"NOASSERTION"}]}
JSON
printf '%s' "${TEST_SYFT_WHITESPACE:-}"
SH
cat > "$TEST_DIR/bin/docker" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf 'docker %s\n' "$*" >> "$TEST_CALLS"
case "$1" in
  image) awk -v ref="${@: -1}" '$1 == ref {print $3}' "$AIRGAP_DIR/digests.lock" ;;
  load) if [ "${2:-}" = -i ]; then cat "$3" >/dev/null; else cat >/dev/null; fi ;;
  *) echo 'unexpected docker call' >&2; exit 64 ;;
esac
SH
for cli in helm kubectl; do
  cat > "$TEST_DIR/bin/$cli" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf 'provision\n' >> "$TEST_CALLS"
SH
done
chmod +x "$TEST_DIR/bin/"*
for cli in bash dirname python3; do ln -s "$(command -v "$cli")" "$TEST_DIR/no-syft/$cli"; done
export PATH="$TEST_DIR/bin:$PATH"

generate() { bash "$SCRIPT_DIR/generate_sbom.sh"; }
verify() { bash "$SCRIPT_DIR/verify_sbom.sh"; }
install_bundle() { KUBE_CONTEXT='test' bash "$SCRIPT_DIR/install_from_airgap.sh"; }
expect_failure() {
  local label="$1"
  shift
  if "$@" > "$TEST_DIR/failure.out" 2>&1; then
    echo "FAIL expected rejection: $label" >&2
    exit 1
  fi
  echo "PASS $label"
}
write_bundle_manifest() {
  (cd "$AIRGAP_DIR" && find . -type f ! -name manifest.sha256 -print0 | sort -z | xargs -0 shasum -a 256) > "$TEST_DIR/manifest"
  mv "$TEST_DIR/manifest" "$AIRGAP_DIR/manifest.sha256"
}

python3 - "$SCRIPT_DIR" <<'PY'
import sys
sys.path.insert(0, sys.argv[1])
from sbom import is_gpl_family

positive = [
    "GPLv2", "GPL2", "LicenseRef-GPLv2+", "LicenseRef-LGPLv2.1",
    "BSD AND GPLv3+", "GNU General Public License", "AGPL-3.0-only",
    "GPL-2.0-only OR LGPL-2.1-only",
]
negative = ["MIT", "Apache-2.0", "BSD-3-Clause"]
for name in positive:
    assert is_gpl_family(name), f"expected GPL family: {name}"
for name in negative:
    assert not is_gpl_family(name), f"unexpected GPL family: {name}"
PY
echo 'PASS GPL-family heuristic covers common spellings without false-flagging MIT/Apache/BSD'

write_bundle_manifest
if ! generate > "$TEST_DIR/generate.out" 2>&1; then
  cat "$TEST_DIR/generate.out" >&2
  echo 'FAIL normal SBOM generation' >&2
  exit 1
fi
verify
python3 - "$AIRGAP_DIR" <<'PY'
import hashlib
import json
from pathlib import Path
import sys
bundle = Path(sys.argv[1])
manifest = json.loads((bundle / "sbom/manifest.json").read_text())
lock = [line.split() for line in (bundle / "digests.lock").read_text().splitlines()]
assert len(manifest["images"]) == len(lock)
for entry, (ref, repo, image_id) in zip(manifest["images"], lock):
    assert (entry["image_ref"], entry["repo_digest"], entry["image_id"]) == (ref, repo, image_id)
    assert entry["sbom_sha256"] == hashlib.sha256((bundle / entry["sbom"]).read_bytes()).hexdigest()
summary = json.loads((bundle / "sbom/licenses.json").read_text())
assert summary["package_count"] == 6
assert {item["license"]: item["count"] for item in summary["licenses"]} == {
    "MIT": 2, "GPL-2.0-only OR LGPL-2.1-only": 2, "NOASSERTION": 2}
assert [item["license"] for item in summary["licenses"] if item["gpl_family"]] == ["GPL-2.0-only OR LGPL-2.1-only"]
PY
[ "$(wc -l < "$TEST_CALLS" | tr -d ' ')" -eq 2 ]
echo 'PASS all locked images linked, SPDX hashes and informational GPL summary'

# Regeneration after the downloader has hashed the evidence must not stale or
# silently re-baseline hashes of unrelated assets.
write_bundle_manifest
cp "$AIRGAP_DIR/manifest.sha256" "$TEST_DIR/original-checksums"
TEST_SYFT_WHITESPACE=' ' generate > "$TEST_DIR/regenerate.out" 2>&1
(cd "$AIRGAP_DIR" && shasum -a 256 -c manifest.sha256 --status)
diff <(grep -v './sbom/' "$TEST_DIR/original-checksums") <(grep -v './sbom/' "$AIRGAP_DIR/manifest.sha256")
install_bundle > "$TEST_DIR/install.out" 2>&1
grep -q '프로비저닝 성공' "$TEST_DIR/install.out"
echo 'PASS regeneration and installer accept complete evidence'

expect_failure 'requested syft missing' env PATH="$TEST_DIR/no-syft" /bin/bash "$SCRIPT_DIR/generate_sbom.sh"
grep -qi syft "$TEST_DIR/failure.out"
cp "$AIRGAP_DIR/sbom/manifest.json" "$TEST_DIR/valid-manifest"
: > "$TEST_CALLS"
expect_failure 'one empty SBOM' env TEST_SYFT_EMPTY=1 bash "$SCRIPT_DIR/generate_sbom.sh"
grep -qi SBOM "$TEST_DIR/failure.out"
cmp "$TEST_DIR/valid-manifest" "$AIRGAP_DIR/sbom/manifest.json"
expect_failure 'syft command error' env TEST_SYFT_ERROR=1 bash "$SCRIPT_DIR/generate_sbom.sh"

for corruption in digest image_id sha256 missing duplicate path empty; do
  cp "$TEST_DIR/valid-manifest" "$AIRGAP_DIR/sbom/manifest.json"
  python3 - "$AIRGAP_DIR/sbom/manifest.json" "$corruption" <<'PY'
import json
from pathlib import Path
import sys
path, mode = Path(sys.argv[1]), sys.argv[2]
data = json.loads(path.read_text())
entry = data["images"][0]
if mode == "digest": entry["repo_digest"] = "example.invalid/demo0@sha256:" + "b" * 64
if mode == "image_id": entry["image_id"] = "sha256:" + "b" * 64
if mode == "sha256": entry["sbom_sha256"] = "0" * 64
if mode == "missing": data["images"].pop()
if mode == "duplicate": data["images"].append(entry)
if mode == "path": entry["sbom"] = "../outside.json"
path.write_text("" if mode == "empty" else json.dumps(data))
PY
  expect_failure "manifest $corruption" verify
  # Recompute transport hashes so installer must reject the SBOM contract itself.
  write_bundle_manifest
  : > "$TEST_CALLS"
  expect_failure "installer rejects $corruption before any mutation" install_bundle
  [ ! -s "$TEST_CALLS" ]
done
cp "$TEST_DIR/valid-manifest" "$AIRGAP_DIR/sbom/manifest.json"
sbom="$AIRGAP_DIR/sbom/$(image_archive_name example.invalid/demo0:1.0).spdx.json"
cp "$sbom" "$TEST_DIR/valid-sbom"
printf '\n' >> "$sbom"
expect_failure 'SBOM content tamper' verify
cp "$TEST_DIR/valid-sbom" "$sbom"
rm "$sbom"
expect_failure 'missing SBOM file' verify
cp "$TEST_DIR/valid-sbom" "$sbom"
: > "$sbom"
expect_failure 'empty SBOM file' verify
cp "$TEST_DIR/valid-sbom" "$sbom"

cp "$AIRGAP_DIR/digests.lock" "$TEST_DIR/valid-lock"
sed 's/sha256:/sha256:b/' "$TEST_DIR/valid-lock" > "$AIRGAP_DIR/digests.lock"
expect_failure 'malformed digest lock' verify
cp "$TEST_DIR/valid-lock" "$AIRGAP_DIR/digests.lock"
python3 - "$AIRGAP_DIR/digests.lock" <<'PY'
from pathlib import Path
import sys
p = Path(sys.argv[1])
rows = p.read_text().splitlines()
parts = rows[0].split()
parts[2] = 'sha256:' + 'f' * 64
rows[0] = ' '.join(parts)
p.write_text('\n'.join(rows) + '\n')
PY
expect_failure 'archive config digest differs from lock' generate
cp "$TEST_DIR/valid-lock" "$AIRGAP_DIR/digests.lock"

rm -rf "$AIRGAP_DIR/sbom"
write_bundle_manifest
install_bundle > "$TEST_DIR/no-sbom.out" 2>&1
grep -q 'SBOM 없음' "$TEST_DIR/no-sbom.out"
grep -q '프로비저닝 성공' "$TEST_DIR/no-sbom.out"
echo 'PASS absent optional SBOM is explicit and permits install'
echo 'PASS airgap SBOM regression suite (stub syft; no real syft or image pulls)'
