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

    def __init__(self, tag, platforms=(("linux", "arm64", None),), compress=True):
        self.plain = [layer(f"{tag}-base"), layer(f"{tag}-app")]
        self.stored = [gzip.compress(item, mtime=0) if compress else item for item in self.plain]
        self.config = json.dumps({"architecture": "arm64", "os": "linux", "tag": tag,
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

    for label in passed:
        print(f"PASS check_archive: {label}")
    print(f"PASS check_archive fixture suite: {len(passed)} cases")


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="archive-identity-") as directory:
        main(Path(directory))
