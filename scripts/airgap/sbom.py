"""Local SBOM evidence: strict lock coverage, archive identity, hashes and licenses."""

from collections import Counter
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import shutil
import sys
import tarfile


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read_lock(path):
    images = {}
    for line in Path(path).read_text().splitlines():
        if not line.strip():
            continue
        fields = line.split()
        require(len(fields) == 3, "digests.lock: expected three fields")
        ref, repo, image_id = fields
        require(re.fullmatch(r"[\w./:@-]+", ref) and ":" in ref,
                "digests.lock: invalid image ref")
        require(repo == "unverified" or re.fullmatch(r"[\w./:-]+@sha256:[0-9a-f]{64}", repo),
                "digests.lock: invalid RepoDigest")
        require(re.fullmatch(r"sha256:[0-9a-f]{64}", image_id), "digests.lock: invalid image ID")
        require(ref not in images, f"digests.lock: duplicate image {ref}")
        images[ref] = {"image_ref": ref, "repo_digest": repo, "image_id": image_id}
    require(images, "digests.lock: no images")
    return images


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def read_json(path):
    require(path.is_file() and path.stat().st_size, f"missing or empty SBOM evidence: {path}")
    return json.loads(path.read_text())


def write_json(path, data):
    path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n")


def sbom_file(directory, relative):
    require(isinstance(relative, str), "invalid SBOM path")
    parts = PurePosixPath(relative).parts
    require(len(parts) == 2 and parts[0] == "sbom" and parts[1].endswith(".spdx.json")
            and "\\" not in relative, f"invalid SBOM path: {relative}")
    path = directory / parts[1]
    require(not directory.is_symlink() and not path.is_symlink(), "symlink SBOM evidence rejected")
    return path


def packages(path):
    document = read_json(path)
    require(isinstance(document, dict) and document.get("spdxVersion") in ("SPDX-2.2", "SPDX-2.3")
            and document.get("SPDXID") == "SPDXRef-DOCUMENT"
            and isinstance(document.get("packages"), list), f"invalid SPDX JSON SBOM: {path}")
    for package in document["packages"]:
        require(isinstance(package, dict) and isinstance(package.get("name"), str)
                and package["name"], f"invalid SPDX package: {path}")
    return document["packages"]


def check_archive(path, expected):
    # Read only regular members; never extract potentially hostile archive paths.
    with tarfile.open(path, "r:*") as archive:
        def member_bytes(name):
            matches = [item for item in archive.getmembers() if item.name == name]
            require(len(matches) == 1 and matches[0].isfile(), f"invalid docker-save member: {name}")
            with archive.extractfile(matches[0]) as member:
                return member.read()

        manifest = json.loads(member_bytes("manifest.json"))
        require(isinstance(manifest, list) and len(manifest) == 1,
                "SBOM requires a single-image docker-save archive")
        config = member_bytes(manifest[0]["Config"])
        actual = "sha256:" + hashlib.sha256(config).hexdigest()
        require(actual == expected, f"archive config digest mismatch: lock={expected}, archive={actual}")


def license_summary(entries, directory):
    counts = Counter()
    for entry in entries:
        for package in packages(sbom_file(directory, entry["sbom"])):
            concluded = package.get("licenseConcluded")
            license_id = concluded if concluded and concluded != "NOASSERTION" else package.get("licenseDeclared")
            license_id = license_id or "NOASSERTION"
            require(isinstance(license_id, str), "invalid SPDX license expression")
            counts[license_id] += 1
    return {
        "informational_only": True,
        "counting": "One license expression per package per image; concluded, then declared, then NOASSERTION.",
        "package_count": sum(counts.values()),
        "licenses": [{"license": name, "count": count,
                      "gpl_family": bool(re.search(r"(?:^|[^a-z0-9])(?:a|l)?gpl(?:[^a-z0-9]|$)", name, re.I))}
                     for name, count in sorted(counts.items())],
    }


def assemble(lock, work):
    images = read_lock(lock)
    seen = set()
    for line in (work / "paths").read_text().splitlines():
        ref, relative = line.split("\t")
        require(ref in images and ref not in seen, "SBOM image coverage mismatch")
        seen.add(ref)
        path = sbom_file(work / "sbom", relative)
        packages(path)
        images[ref].update(sbom=relative, sbom_sha256=sha256(path))
    require(seen == images.keys(), "missing image SBOM")
    entries = list(images.values())
    write_json(work / "sbom/manifest.json", {"schema_version": 1, "images": entries})
    write_json(work / "sbom/licenses.json", license_summary(entries, work / "sbom"))


def verify(lock, directory):
    images = read_lock(lock)
    require(not directory.is_symlink() and not (directory / "manifest.json").is_symlink(),
            "symlink SBOM manifest rejected")
    manifest = read_json(directory / "manifest.json")
    require(isinstance(manifest, dict) and manifest.get("schema_version") == 1
            and isinstance(manifest.get("images"), list), "invalid SBOM manifest schema")
    seen, files = set(), set()
    for entry in manifest["images"]:
        require(isinstance(entry, dict), "invalid SBOM entry")
        ref = entry.get("image_ref")
        require(isinstance(ref, str) and ref in images and ref not in seen, "SBOM image coverage mismatch")
        require(all(entry.get(key) == value for key, value in images[ref].items()),
                f"SBOM digest differs from digests.lock: {ref}")
        path = sbom_file(directory, entry.get("sbom"))
        require(path not in files, "duplicate SBOM path")
        packages(path)
        require(entry.get("sbom_sha256") == sha256(path), f"SBOM sha256 mismatch: {ref}")
        seen.add(ref)
        files.add(path)
    require(seen == images.keys(), "missing image SBOM")
    print(f"SBOM 검증 통과: {len(seen)}개 이미지 (digest lock + SPDX JSON + sha256)")


def publish(bundle, work):
    destination = bundle / "sbom"
    require(not destination.is_symlink() and (not destination.exists() or destination.is_dir()),
            "invalid SBOM destination")
    checksum_manifest = bundle / "manifest.sha256"
    updated = work / "manifest.sha256"
    if checksum_manifest.exists():
        # D-b (#98): refreshing evidence must not re-baseline unrelated assets.
        # Cost: preserve the transport manifest's old asset hashes verbatim.
        # Escape hatch: recollect assets via the existing downloader when needed.
        retained = []
        for line in checksum_manifest.read_text().splitlines(keepends=True):
            match = re.fullmatch(r"[0-9a-f]{64} [ *](.+)\n?", line)
            require(match, "invalid manifest.sha256 entry")
            parts = PurePosixPath(match[1].rstrip("\n")).parts
            if not parts or parts[0] != "sbom":
                retained.append(line.rstrip("\n") + "\n")
        for path in sorted((work / "sbom").iterdir()):
            retained.append(f"{sha256(path)}  ./sbom/{path.name}\n")
        updated.write_text("".join(retained))
    backup = work / "previous"
    if destination.exists():
        destination.rename(backup)
    try:
        (work / "sbom").rename(destination)
        if updated.exists():
            updated.replace(checksum_manifest)
    except OSError:
        if destination.exists():
            shutil.rmtree(destination)
        if backup.exists():
            backup.rename(destination)
        raise


if __name__ == "__main__":
    try:
        action = sys.argv[1]
        if action == "lock":
            for image in read_lock(sys.argv[2]).values():
                print(image["image_ref"], image["repo_digest"], image["image_id"])
        elif action == "archive":
            check_archive(sys.argv[2], sys.argv[3])
        elif action == "assemble":
            assemble(sys.argv[2], Path(sys.argv[3]))
        elif action == "verify":
            verify(sys.argv[2], Path(sys.argv[3]))
        elif action == "publish":
            publish(Path(sys.argv[2]), Path(sys.argv[3]))
        else:
            raise ValueError(f"unknown SBOM action: {action}")
    except (OSError, ValueError, KeyError, TypeError, tarfile.TarError) as error:
        print(f"SBOM 오류: {error}", file=sys.stderr)
        sys.exit(1)
