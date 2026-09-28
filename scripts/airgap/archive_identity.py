"""Bind a `docker save` archive to its digests.lock image ID (#98).

syft's `docker-archive:` source reads the legacy manifest.json Config/Layers, so the
lock digest must be linked to exactly those bytes. Every digest used here is the
sha256 of bytes re-read from the archive; no digest string in the archive is trusted.

Resource-exhaustion caps (a hostile archive must fail closed, never allocate
unbounded memory) are configurable via env var and read fresh on every call:
  KUBEMETAL_ARCHIVE_LAYER_CAP_BYTES  - max decompressed bytes per gzip layer
                                        (default 8 GiB); a compressed layer is
                                        rejected the moment this is crossed,
                                        never after materializing it in full.
  KUBEMETAL_ARCHIVE_JSON_CAP_BYTES   - max size of any whole-member JSON read
                                        (manifest.json, image config, and the
                                        containerd-store index/manifest blobs;
                                        default 16 MiB).
  KUBEMETAL_ARCHIVE_MEMBER_CAP       - max docker-save tar member count
                                        (default 100000).
"""

import hashlib
import json
import os
from pathlib import PurePosixPath
import re
import tarfile
import zlib

DIGEST = re.compile(r"sha256:[0-9a-f]{64}")
# Colima's K3s VM is linux/arm64 (D23); `docker save` on this host keeps only that
# platform's manifest out of a multi-platform list.
PLATFORM = {"os": "linux", "architecture": "arm64"}
CHUNK = 1024 * 1024
DEFAULT_LAYER_CAP_BYTES = 8 * 1024 ** 3
DEFAULT_JSON_CAP_BYTES = 16 * 1024 * 1024
DEFAULT_MEMBER_CAP = 100_000


def require(condition, message):
    if not condition:
        raise ValueError(message)


def _cap(env_name, default):
    raw = os.environ.get(env_name)
    if raw is None:
        return default
    require(raw.isdigit() and int(raw) > 0, f"{env_name} must be a positive integer")
    return int(raw)


def _stream_digests(fileobj):
    """Return (sha256 of stored bytes, sha256 of the uncompressed tar stream).

    Decompression is bounded on two axes so a hostile gzip layer (a "gzip bomb":
    small compressed input, huge expansion) cannot exhaust memory before the
    diff_id check downstream ever runs: each zlib.decompress call is capped to
    CHUNK output bytes via max_length, and the running total of decompressed
    bytes is capped at KUBEMETAL_ARCHIVE_LAYER_CAP_BYTES - the layer is rejected
    the instant either bound is crossed, never after materializing the full
    output.
    """
    cap = _cap("KUBEMETAL_ARCHIVE_LAYER_CAP_BYTES", DEFAULT_LAYER_CAP_BYTES)
    stored, plain = hashlib.sha256(), hashlib.sha256()
    plain_total = 0
    first = fileobj.read(CHUNK)
    gzipped = first[:2] == b"\x1f\x8b"
    require(first[:4] != b"\x28\xb5\x2f\xfd", "zstd layers are not supported by the stdlib check")
    inflater = zlib.decompressobj(16 + zlib.MAX_WBITS) if gzipped else None
    chunk = first
    while chunk:
        stored.update(chunk)
        if inflater is None:
            plain_total += len(chunk)
            require(plain_total <= cap, "layer exceeds decompressed size cap")
            plain.update(chunk)
        else:
            data = chunk
            while data:
                if inflater.eof:  # concatenated gzip members
                    inflater = zlib.decompressobj(16 + zlib.MAX_WBITS)
                try:
                    piece = inflater.decompress(data, CHUNK)
                except zlib.error as error:
                    raise ValueError(f"corrupt gzip layer: {error}") from error
                plain_total += len(piece)
                require(plain_total <= cap, "layer exceeds decompressed size cap")
                plain.update(piece)
                # decompress(data, max_length) may cap output before all of `data`
                # is consumed; re-feed only what's left (unconsumed_tail), or once
                # eof, any concatenated-member bytes past the stream end
                # (unused_data) - never the original chunk, or already-consumed
                # input would be redone.
                if inflater.unconsumed_tail:
                    data = inflater.unconsumed_tail
                elif inflater.eof:
                    data = inflater.unused_data
                else:
                    data = b""
        chunk = fileobj.read(CHUNK)
    require(inflater is None or inflater.eof, "truncated gzip layer")
    return "sha256:" + stored.hexdigest(), "sha256:" + plain.hexdigest()


def _platform_manifest(index):
    manifests = index.get("manifests")
    require(isinstance(manifests, list), "invalid manifest list")
    matches = [item for item in manifests if isinstance(item, dict)
               and isinstance(item.get("platform"), dict)
               and all(item["platform"].get(key) == value for key, value in PLATFORM.items())
               and item["platform"].get("variant") in (None, "v8")]
    require(len(matches) == 1, f"expected exactly one linux/arm64 manifest, found {len(matches)}")
    digest = matches[0].get("digest")
    require(isinstance(digest, str) and DIGEST.fullmatch(digest), "invalid platform manifest digest")
    return digest


def check_archive(path, expected):
    require(isinstance(expected, str) and DIGEST.fullmatch(expected), "invalid locked image ID")
    member_cap = _cap("KUBEMETAL_ARCHIVE_MEMBER_CAP", DEFAULT_MEMBER_CAP)
    json_cap = _cap("KUBEMETAL_ARCHIVE_JSON_CAP_BYTES", DEFAULT_JSON_CAP_BYTES)
    # Read only regular members; never extract potentially hostile archive paths.
    with tarfile.open(path, "r:*") as archive:
        members = {}
        count = 0
        # Iterate lazily (not getmembers(), which loads the whole member index
        # up front) so a member-count bomb is rejected as soon as it's seen.
        for item in archive:
            count += 1
            require(count <= member_cap, f"docker-save archive exceeds member count cap ({member_cap})")
            name = item.name
            require(not name.startswith("/") and "\\" not in name
                    and PurePosixPath(name).as_posix() == name
                    and not {".", ".."} & set(PurePosixPath(name).parts),
                    f"unsafe docker-save member name: {name!r}")
            # Links could make syft read bytes other than the ones hashed here.
            require(item.isfile() or item.isdir(), f"non-regular docker-save member: {name}")
            require(name not in members, f"duplicate docker-save member: {name}")
            members[name] = item

        def member(name):
            require(isinstance(name, str) and name in members and members[name].isfile(),
                    f"invalid docker-save member: {name}")
            return archive.extractfile(members[name])

        def read_json(name):
            # manifest.json / image config / (containerd-store) index and platform-
            # manifest blobs are the only whole-member reads; cap them by the
            # member's declared size so an oversized JSON document is rejected
            # before being read fully into memory.
            info = members.get(name) if isinstance(name, str) else None
            require(info is not None and info.isfile(), f"invalid docker-save member: {name}")
            require(info.size <= json_cap,
                    f"docker-save JSON member exceeds size cap ({json_cap} bytes): {name}")
            with archive.extractfile(info) as source:
                return source.read()

        def blob(digest):
            data = read_json("blobs/" + digest.replace(":", "/"))
            require("sha256:" + hashlib.sha256(data).hexdigest() == digest,
                    f"blob does not hash to its name: {digest}")
            return data

        manifest = json.loads(read_json("manifest.json"))
        require(isinstance(manifest, list) and len(manifest) == 1 and isinstance(manifest[0], dict),
                "SBOM requires a single-image docker-save archive")
        config_bytes = read_json(manifest[0].get("Config"))
        config_digest = "sha256:" + hashlib.sha256(config_bytes).hexdigest()
        config = json.loads(config_bytes)
        require(isinstance(config, dict), "invalid image config JSON")
        # #98 critic review: enforced only on the manifest-list path before this -
        # an amd64 image locked by config digest (classic) or a direct single-
        # manifest digest (no list) skipped this entirely. `config` is the same
        # object every binding path below re-hashes and hands to syft, so
        # checking it here covers all three paths in one place.
        require(config.get("os") == PLATFORM["os"] and config.get("architecture") == PLATFORM["architecture"]
                and config.get("variant") in (None, "v8"),
                f"image config is not {PLATFORM['os']}/{PLATFORM['architecture']}: "
                f"os={config.get('os')!r} architecture={config.get('architecture')!r} "
                f"variant={config.get('variant')!r}")
        layers = manifest[0].get("Layers")
        rootfs = config.get("rootfs")
        diff_ids = rootfs.get("diff_ids") if isinstance(rootfs, dict) else None
        require(isinstance(layers, list) and isinstance(diff_ids, list) and len(layers) == len(diff_ids),
                "manifest.json Layers do not match config rootfs.diff_ids")

        stored_digests = []
        for index, (name, diff_id) in enumerate(zip(layers, diff_ids)):
            with member(name) as source:
                stored, plain = _stream_digests(source)
            require(plain == diff_id, f"layer {index} does not match config diff_id: {name}")
            stored_digests.append(stored)

        if expected == config_digest:
            # Classic dockerd store: .Id is the config digest, which content-addresses
            # the diff_ids just checked against every layer syft will read.
            return

        # containerd image store (Docker Desktop 4.34+ default): .Id is the pulled
        # manifest(-list) digest. Walk lock digest -> list -> linux/arm64 manifest ->
        # the exact Config/Layers bytes manifest.json hands to syft. The index.json
        # file is never consulted: the chain starts from the lock, not the archive.
        require("blobs/" + expected.replace(":", "/") in members,
                f"archive config digest mismatch and no locked manifest blob: "
                f"lock={expected}, config={config_digest}")
        top = json.loads(blob(expected))
        require(isinstance(top, dict), "invalid locked manifest")
        if "manifests" in top:
            image = json.loads(blob(_platform_manifest(top)))
        else:
            image = top
        require(isinstance(image, dict) and "manifests" not in image
                and isinstance(image.get("config"), dict) and isinstance(image.get("layers"), list),
                "locked digest does not resolve to an image manifest")
        require(image["config"].get("digest") == config_digest,
                f"manifest.json Config is not the locked image's config: lock={expected}")
        require([item.get("digest") if isinstance(item, dict) else None for item in image["layers"]]
                == stored_digests,
                f"manifest.json Layers are not the locked image's layers: lock={expected}")
