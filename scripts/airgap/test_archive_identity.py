"""Offline fixtures for archive_identity.check_archive (#98); run by test_sbom.sh.

Fixtures mirror the measured Docker Desktop 29.8 containerd-store `docker save`
shape (2026-09-28, all 12 bundle images): index.json -> manifest list whose only
present entry is linux/arm64, gzip layer blobs named by their compressed digest,
and a legacy manifest.json whose Config/Layers are those same blobs.
"""

import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import sys
import tarfile
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
from archive_identity import check_archive  # noqa: E402


def digest(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def blob_name(value):
    return "blobs/" + value.replace(":", "/")


def layer(text):
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as tar:
        data = text.encode()
        info = tarfile.TarInfo(f"etc/{text}")
        info.size = len(data)
        tar.addfile(info, io.BytesIO(data))
    return raw.getvalue()


class Image:
    """A genuine index -> manifest -> config/layers chain, mutable per case."""

    def __init__(self, tag, platforms=(("linux", "arm64", None),), compress=True,
                 config_os="linux", config_arch="arm64"):
        self.plain = [layer(f"{tag}-base"), layer(f"{tag}-app")]
        self.stored = [gzip.compress(item, mtime=0) if compress else item for item in self.plain]
        self.config = json.dumps({"architecture": config_arch, "os": config_os, "tag": tag,
                                  "rootfs": {"type": "layers",
                                             "diff_ids": [digest(item) for item in self.plain]}}).encode()
        self.manifest = json.dumps({
            "schemaVersion": 2, "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {"digest": digest(self.config), "size": len(self.config)},
            "layers": [{"digest": digest(item), "size": len(item)} for item in self.stored]}).encode()
        entries = [{"digest": digest(self.manifest), "platform": {"os": os_, "architecture": arch,
                                                                  **({"variant": variant} if variant else {})}}
                   for os_, arch, variant in platforms]
        entries += [{"digest": "sha256:" + "a" * 64, "platform": {"os": "linux", "architecture": "amd64"}},
                    {"digest": "sha256:" + "b" * 64, "platform": {"os": "unknown", "architecture": "unknown"}}]
        self.index = json.dumps({"schemaVersion": 2, "manifests": entries}).encode()

    def members(self, config=None, stored=None):
        config = self.config if config is None else config
        stored = self.stored if stored is None else stored
        blobs = {blob_name(digest(item)): item for item in [self.index, self.manifest, config, *stored]}
        legacy = [{"Config": blob_name(digest(config)), "RepoTags": ["example.invalid/demo:1.0"],
                   "Layers": [blob_name(digest(item)) for item in stored]}]
        return {"oci-layout": b'{"imageLayoutVersion":"1.0.0"}',
                "index.json": json.dumps({"manifests": [{"digest": digest(self.index)}]}).encode(),
                "manifest.json": json.dumps(legacy).encode(), **blobs}


def write(path, members, links=()):
    with tarfile.open(path, "w") as tar:
        for name, data in members.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
        for name, target in links:
            info = tarfile.TarInfo(name)
            info.type, info.linkname = tarfile.SYMTYPE, target
            tar.addfile(info)
    return str(path)


def main(work):
    passed = []

    def accept(label, path, expected):
        check_archive(path, expected)
        passed.append(label)

    def reject(label, path, expected, needle):
        try:
            check_archive(path, expected)
        except ValueError as error:
            assert needle in str(error), f"{label}: wrong reason: {error}"
            passed.append(label)
            return
        raise AssertionError(f"{label}: forged archive was accepted")

    image, other = Image("genuine"), Image("unrelated")
    lock_list, lock_manifest = digest(image.index), digest(image.manifest)
    lock_config = digest(image.config)
    genuine = write(work / "genuine.tar", image.members())

    accept("genuine containerd archive: lock = manifest-list digest", genuine, lock_list)
    accept("genuine archive: lock = single-platform manifest digest", genuine, lock_manifest)
    accept("genuine archive: lock = config digest (classic .Id)", genuine, lock_config)
    variant = Image("variant", platforms=(("linux", "arm64", "v8"),))
    accept("genuine list with linux/arm64/v8 entry",
           write(work / "variant.tar", variant.members()), digest(variant.index))
    classic = Image("classic", compress=False)
    classic_members = {key: value for key, value in classic.members().items()
                       if key in ("manifest.json",) or key in {blob_name(digest(classic.config)),
                                                               *[blob_name(digest(item)) for item in classic.stored]}}
    classic_tar = write(work / "classic.tar", classic_members)
    accept("classic archive (uncompressed layers): lock = config digest", classic_tar, digest(classic.config))

    # The approval-review PoC: real list/manifest bytes, but manifest.json names an
    # unrelated (self-consistent) image that syft would scan instead.
    forged = image.members()
    forged.update({key: value for key, value in other.members().items() if key != "index.json"})
    forged["manifest.json"] = other.members()["manifest.json"]
    reject("reviewer PoC: genuine index bytes + unrelated manifest.json/config",
           write(work / "forged.tar", forged), lock_list, "Config is not the locked image's config")

    swapped = list(image.stored)
    swapped[1] = other.stored[1]
    reject("swapped layer bytes (config unchanged)",
           write(work / "swap.tar", image.members(stored=swapped)), lock_list, "does not match config diff_id")
    reordered = image.members(stored=list(reversed(image.stored)))
    reject("reordered layers", write(work / "order.tar", reordered), lock_list, "does not match config diff_id")
    # Swapped layer plus a config rewritten to match it: internally consistent, but
    # no longer the config the locked manifest names.
    rewritten = json.loads(image.config)
    rewritten["rootfs"]["diff_ids"][1] = digest(other.plain[1])
    rewritten = json.dumps(rewritten).encode()
    reject("swapped layer with a rewritten config",
           write(work / "swap-config.tar", image.members(config=rewritten, stored=swapped)),
           lock_list, "Config is not the locked image's config")
    mismatched = json.loads(image.config)
    mismatched["Env"] = ["INJECTED=1"]
    reject("mismatched config (same layers)",
           write(work / "config.tar", image.members(config=json.dumps(mismatched).encode())),
           lock_list, "Config is not the locked image's config")

    ambiguous = Image("ambiguous", platforms=(("linux", "arm64", None), ("linux", "arm64", "v8")))
    reject("two linux/arm64 manifests in the locked list",
           write(work / "ambiguous.tar", ambiguous.members()), digest(ambiguous.index), "exactly one linux/arm64")

    for label, missing, needle in [("locked list blob", digest(image.index), "no locked manifest blob"),
                                   ("platform manifest blob", lock_manifest, "invalid docker-save member"),
                                   ("layer blob", digest(image.stored[0]), "invalid docker-save member")]:
        members = image.members()
        del members[blob_name(missing)]
        reject(f"missing {label}", write(work / "missing.tar", members), lock_list, needle)

    tampered = image.members()
    tampered[blob_name(lock_manifest)] = image.manifest + b" "
    reject("platform manifest blob does not hash to its name",
           write(work / "tampered.tar", tampered), lock_list, "does not hash to its name")
    reject("unrelated lock digest", genuine, "sha256:" + "c" * 64, "no locked manifest blob")

    classic_swap = dict(classic_members)
    classic_swap[blob_name(digest(classic.stored[0]))] = other.plain[0]
    reject("classic archive layer swap",
           write(work / "classic-swap.tar", classic_swap), digest(classic.config), "does not match config diff_id")

    reject("tar member path traversal",
           write(work / "traversal.tar", {**image.members(), "../escape": b"x"}), lock_list, "unsafe")
    reject("symlink member",
           write(work / "link.tar", image.members(), links=[("blobs/sha256/link", "../../etc/passwd")]),
           lock_list, "non-regular")
    duplicate = work / "duplicate.tar"
    write(duplicate, image.members())
    with tarfile.open(duplicate, "a") as tar:
        data = other.members()["manifest.json"]
        info = tarfile.TarInfo("manifest.json")
        info.size = len(data)
        tar.addfile(info, io.BytesIO(data))
    reject("duplicate manifest.json member", str(duplicate), lock_list, "duplicate")

    # #98 critic review: platform enforcement previously ran only on the
    # manifest-list path, so an amd64 image locked by config digest (classic
    # dockerd) or by a direct single-manifest digest (no list) passed and got a
    # "verified" SBOM. arm64 passing on every path is already covered by the
    # accept cases above (list digest, single-manifest digest, config digest,
    # arm64/v8 variant, classic uncompressed).
    amd64_classic = Image("amd64-classic", config_os="linux", config_arch="amd64")
    reject("amd64 image via classic path (lock = config digest)",
           write(work / "amd64-classic.tar", amd64_classic.members()),
           digest(amd64_classic.config), "image config is not linux/arm64")
    amd64_direct = Image("amd64-direct", config_os="linux", config_arch="amd64")
    reject("amd64 image via direct single-manifest digest (no list)",
           write(work / "amd64-direct.tar", amd64_direct.members()),
           digest(amd64_direct.manifest), "image config is not linux/arm64")

    # #98 critic review: resource exhaustion. Caps are read fresh from env vars
    # on every check_archive() call, so setting them here (no reload/monkeypatch
    # needed) exercises the real production code path with a low, fast-to-hit
    # threshold instead of actually allocating gigabytes.
    old_layer_cap = os.environ.get("KUBEMETAL_ARCHIVE_LAYER_CAP_BYTES")
    os.environ["KUBEMETAL_ARCHIVE_LAYER_CAP_BYTES"] = "4096"
    try:
        # Small compressed input, huge expansion: zeros compress to a few KB but
        # decompress to 64 MiB, so this is a genuine gzip-bomb shape even though
        # building the plain bytes here (to compress them) is cheap and fast.
        bomb_plain = bytes(64 * 1024 * 1024)
        bomb_stored = gzip.compress(bomb_plain, mtime=0)
        bomb_config = json.dumps({"architecture": "arm64", "os": "linux", "tag": "bomb",
                                  "rootfs": {"type": "layers", "diff_ids": [digest(bomb_plain)]}}).encode()
        bomb_manifest_json = json.dumps([{"Config": blob_name(digest(bomb_config)),
                                          "RepoTags": ["example.invalid/bomb:1.0"],
                                          "Layers": [blob_name(digest(bomb_stored))]}]).encode()
        bomb_members = {"manifest.json": bomb_manifest_json,
                         blob_name(digest(bomb_config)): bomb_config,
                         blob_name(digest(bomb_stored)): bomb_stored}
        reject("gzip bomb layer rejected by the decompressed size cap, without decompressing it in full",
               write(work / "bomb.tar", bomb_members), digest(bomb_config), "decompressed size cap")
    finally:
        if old_layer_cap is None:
            del os.environ["KUBEMETAL_ARCHIVE_LAYER_CAP_BYTES"]
        else:
            os.environ["KUBEMETAL_ARCHIVE_LAYER_CAP_BYTES"] = old_layer_cap

    old_json_cap = os.environ.get("KUBEMETAL_ARCHIVE_JSON_CAP_BYTES")
    os.environ["KUBEMETAL_ARCHIVE_JSON_CAP_BYTES"] = "1024"
    try:
        oversized = Image("oversized")
        padded_manifest = json.loads(oversized.members()["manifest.json"])
        padded_manifest[0]["padding"] = "x" * 4096  # pushes manifest.json past the 1024-byte cap
        oversized_members = dict(oversized.members())
        oversized_members["manifest.json"] = json.dumps(padded_manifest).encode()
        reject("oversized manifest.json JSON member rejected by its declared size",
               write(work / "oversized-json.tar", oversized_members),
               digest(oversized.config), "JSON member exceeds size cap")
    finally:
        if old_json_cap is None:
            del os.environ["KUBEMETAL_ARCHIVE_JSON_CAP_BYTES"]
        else:
            os.environ["KUBEMETAL_ARCHIVE_JSON_CAP_BYTES"] = old_json_cap

    for label in passed:
        print(f"PASS check_archive: {label}")
    print(f"PASS check_archive fixture suite: {len(passed)} cases")


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="archive-identity-") as directory:
        main(Path(directory))
