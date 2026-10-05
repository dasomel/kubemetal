#!/usr/bin/env python3
"""Offline verifier for support bundles written by src-tauri/src/services/support_bundle.rs.

A bundle is untrusted input after import, so this checks the manifest against the schema the
Rust side really serializes (SupportBundleManifest), recomputes every listed file's sha256,
and refuses traversal/symlinks. Python stdlib only; no network, no app, no daemon.

Exit codes: 0 = every check ran and passed, 1 = at least one problem (bundle not verified),
2 = usage error or the bundle/manifest could not be read at all.

The service writes NO hash of manifest.json itself and never lists manifest.json in `files`.
So the manifest is unauthenticated: editing it and the files together passes the file-hash
check. We say so on every run; `--manifest-sha256` checks it against a digest pinned out of
band (D22: never claim a check that was not made).
"""
import argparse
import hashlib
import json
import os
import re
import stat
import sys

MANIFEST_FILE = "manifest.json"
SUPPORTED_SCHEMA = 1  # SCHEMA_VERSION in support_bundle.rs
# MAX_SCANNABLE_BYTES in support_bundle.rs: the writer never emits a larger file, so a bigger
# one is not something the service produced and is refused rather than hashed.
DEFAULT_MAX_BYTES = 10 * 1024 * 1024
MANIFEST_MAX_BYTES = 16 * 1024 * 1024
KNOWN_RULES_VERSION = 2  # REDACTION_RULES_VERSION
REQUIRED_FILES = ("app_info.json", "health_summary.json")  # always written by the service
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
CREATED_RE = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")
U32_MAX = 2**32 - 1
U64_MAX = 2**64 - 1
READ_ERRORS = ("unreadable", "manifest-unreadable")


def _safe(text):
    """Make bundle-controlled text safe to print. A bundle is untrusted input after import: a path with a
    newline could forge a 'RESULT: OK' line, and ESC/CR/BEL/bidi characters could rewrite the terminal.
    str.isprintable() is False for control (Cc) and format (Cf, e.g. bidi overrides) characters, so
    everything else is shown as a visible \\uXXXX escape and nothing is dropped silently."""
    return "".join(c if c.isprintable() else (f"\\u{ord(c):04x}" if ord(c) <= 0xFFFF else f"\\U{ord(c):08x}")
                   for c in str(text))


class Report:
    def __init__(self):
        self.problems = []  # (class, message)
        self.notes = []

    def fail(self, cls, msg):
        self.problems.append((cls, msg))

    def note(self, msg):
        self.notes.append(msg)


def _no_dupes(pairs):
    out = {}
    for k, v in pairs:
        if k in out:
            raise ValueError(f"duplicate key {k!r}")
        out[k] = v
    return out


def _reject_const(name):
    raise ValueError(f"non-finite number {name}")


def _is_int(v, hi):
    return isinstance(v, int) and not isinstance(v, bool) and 0 <= v <= hi


def _check_keys(obj, required, optional, where, rep):
    if not isinstance(obj, dict):
        rep.fail("manifest-malformed", f"{where}: expected object, got {type(obj).__name__}")
        return False
    ok = True
    for k in required:
        if k not in obj:
            rep.fail("manifest-malformed", f"{where}: missing field {k!r}")
            ok = False
    for k in obj:
        if k not in required and k not in optional:
            rep.fail("manifest-malformed", f"{where}: unknown field {k!r}")
            ok = False
    return ok


def safe_rel_path(p, allow_dir_suffix=False):
    """Mirror the writer's rule (all components Normal) and be stricter on untrusted input."""
    if not isinstance(p, str) or not p:
        return "path must be a non-empty string"
    if "\x00" in p or "\\" in p:
        return "path contains NUL or backslash"
    q = p[:-1] if allow_dir_suffix and p.endswith("/") else p
    if q.startswith("/") or re.match(r"^[A-Za-z]:", q):
        return "absolute path"
    if any(s in ("", ".", "..") for s in q.split("/")):
        return "path traversal or empty/dot segment"
    return None


def parse_manifest(root, rep, manifest_sha256):
    mpath = os.path.join(root, MANIFEST_FILE)
    try:
        st = os.lstat(mpath)
    except OSError as e:
        rep.fail("manifest-unreadable", f"{MANIFEST_FILE}: {e.strerror}")
        return None
    if not stat.S_ISREG(st.st_mode):
        rep.fail("symlink" if stat.S_ISLNK(st.st_mode) else "manifest-malformed",
                 f"{MANIFEST_FILE} is not a regular file")
        return None
    if st.st_size > MANIFEST_MAX_BYTES:
        rep.fail("size", f"{MANIFEST_FILE} exceeds {MANIFEST_MAX_BYTES} bytes")
        return None
    try:
        fd = os.open(mpath, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
        with os.fdopen(fd, "rb") as f:
            raw = f.read(MANIFEST_MAX_BYTES + 1)
    except OSError as e:
        rep.fail("manifest-unreadable", f"{MANIFEST_FILE}: {e.strerror}")
        return None
    digest = hashlib.sha256(raw).hexdigest()
    rep.note(f"manifest.json sha256 = {digest}")
    if manifest_sha256 is None:
        rep.note("manifest self-hash: the support bundle service writes none, so manifest.json "
                 "itself is NOT authenticated (pass --manifest-sha256 with a digest recorded "
                 "out of band to check it)")
    elif digest != manifest_sha256.lower():
        rep.fail("manifest-hash", f"manifest.json sha256 {digest} != pinned {manifest_sha256.lower()}")
    else:
        rep.note("manifest sha256 matches the pinned digest")
    try:
        m = json.loads(raw.decode("utf-8"), object_pairs_hook=_no_dupes,
                       parse_constant=_reject_const)
    except (ValueError, UnicodeDecodeError) as e:
        rep.fail("manifest-malformed", f"{MANIFEST_FILE} is not valid JSON: {e}")
        return None
    return validate_schema(m, rep)


def validate_schema(m, rep):
    """Field names and optionality exactly as SupportBundleManifest et al. in the Rust service."""
    if not _check_keys(m, ("schema_version", "redaction", "created_at", "files"),
                       ("omitted",), "manifest", rep):
        return None
    ok = True
    sv = m["schema_version"]
    if not _is_int(sv, U32_MAX):
        rep.fail("manifest-malformed", "schema_version must be an unsigned 32-bit integer")
        ok = False
    elif sv != SUPPORTED_SCHEMA:
        rep.fail("schema-version", f"unsupported schema_version {sv} (this verifier knows {SUPPORTED_SCHEMA})")
        ok = False
    if not isinstance(m["created_at"], str) or not CREATED_RE.match(m["created_at"]):
        rep.fail("manifest-malformed", "created_at must be YYYY-MM-DDTHH:MM:SSZ")
        ok = False
    red = m["redaction"]
    if _check_keys(red, ("mode", "review_before_sharing", "rules_version"), (), "redaction", rep):
        red_ok = True
        if not isinstance(red["mode"], str) or not red["mode"]:
            rep.fail("redaction-malformed", "redaction.mode must be a non-empty string")
            red_ok = False
        if not isinstance(red["review_before_sharing"], bool):
            rep.fail("redaction-malformed", "redaction.review_before_sharing must be boolean")
            red_ok = False
        elif not red["review_before_sharing"]:
            rep.note("WARN redaction.review_before_sharing=false (the service always writes true)")
        if not _is_int(red["rules_version"], U32_MAX):
            rep.fail("redaction-malformed", "redaction.rules_version must be an unsigned 32-bit integer")
            red_ok = False
        elif red["rules_version"] != KNOWN_RULES_VERSION:
            rep.note(f"WARN redaction.rules_version={red['rules_version']}, verifier knows {KNOWN_RULES_VERSION}")
        if red_ok:
            rep.note(f"redaction: mode={red['mode']} rules_version={red['rules_version']} "
                     f"review_before_sharing={red['review_before_sharing']} (best-effort denylist; "
                     "its effectiveness is NOT verified here)")
        ok = ok and red_ok
    else:
        rep.fail("redaction-malformed", "redaction metadata absent or malformed")
        ok = False
    if not isinstance(m["files"], list):
        rep.fail("manifest-malformed", "files must be an array")
        return None
    seen = set()
    for i, e in enumerate(m["files"]):
        w = f"files[{i}]"
        if not _check_keys(e, ("path", "sha256", "bytes"), (), w, rep):
            ok = False
            continue
        err = safe_rel_path(e["path"])
        if err:
            rep.fail("path-unsafe", f"{w}: {err}: {e['path']!r}")
            ok = False
        elif e["path"] == MANIFEST_FILE:
            rep.fail("manifest-malformed", f"{w}: manifest.json cannot list itself")
            ok = False
        elif e["path"] in seen:
            rep.fail("manifest-malformed", f"{w}: duplicate path {e['path']!r}")
            ok = False
        seen.add(e["path"])
        if not isinstance(e["sha256"], str) or not SHA_RE.match(e["sha256"]):
            rep.fail("manifest-malformed", f"{w}: sha256 must be 64 lowercase hex chars")
            ok = False
        if not _is_int(e["bytes"], U64_MAX):
            rep.fail("manifest-malformed", f"{w}: bytes must be an unsigned integer")
            ok = False
    for req in REQUIRED_FILES:
        if req not in seen:
            rep.fail("incomplete", f"required file {req} is not listed (the service always writes it)")
            ok = False
    omitted = m.get("omitted", [])
    if not isinstance(omitted, list):
        rep.fail("manifest-malformed", "omitted must be an array")
        return None
    for i, e in enumerate(omitted):
        w = f"omitted[{i}]"
        if not _check_keys(e, ("path", "omitted"), (), w, rep):
            ok = False
            continue
        err = safe_rel_path(e["path"], allow_dir_suffix=True)
        if err:
            rep.fail("path-unsafe", f"{w}: {err}: {e['path']!r}")
            ok = False
        if not isinstance(e["omitted"], str) or not e["omitted"].strip():
            rep.fail("omitted-no-reason", f"{w} ({e.get('path')!r}): omitted entry carries no reason")
            ok = False
        if e["path"] in seen:
            rep.fail("manifest-malformed", f"{w}: {e['path']!r} is both listed and omitted")
            ok = False
    return m if ok else None


def hash_file(root, rel, size_cap, rep):
    """Return (sha256, size) or None after recording why not. Never follows symlinks."""
    cur = root
    parts = rel.split("/")
    for i, part in enumerate(parts):
        cur = os.path.join(cur, part)
        try:
            st = os.lstat(cur)
        except FileNotFoundError:
            rep.fail("missing", f"{rel}: listed in manifest but not present")
            return None
        except OSError as e:
            rep.fail("unreadable", f"{rel}: {e.strerror}")
            return None
        if stat.S_ISLNK(st.st_mode):
            rep.fail("symlink", f"{rel}: path component {part!r} is a symlink")
            return None
        last = i == len(parts) - 1
        if last and not stat.S_ISREG(st.st_mode):
            rep.fail("not-regular", f"{rel}: not a regular file")
            return None
        if not last and not stat.S_ISDIR(st.st_mode):
            rep.fail("not-regular", f"{rel}: path component {part!r} is not a directory")
            return None
    if st.st_size > size_cap:
        rep.fail("size", f"{rel}: {st.st_size} bytes exceeds cap {size_cap}; not read")
        return None
    try:
        fd = os.open(cur, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
        with os.fdopen(fd, "rb") as f:
            data = f.read(size_cap + 1)  # bounded even if the file grew after lstat
    except OSError as e:
        rep.fail("unreadable", f"{rel}: {e.strerror}")
        return None
    if len(data) > size_cap:
        rep.fail("size", f"{rel}: exceeds cap {size_cap} while reading")
        return None
    return hashlib.sha256(data).hexdigest(), len(data)


def walk_tree(root, rep):
    """All non-directory entries relative to root, without following links. Symlinks fail."""
    found = []
    stack = [""]
    while stack:
        rel = stack.pop()
        try:
            with os.scandir(os.path.join(root, rel) if rel else root) as it:
                entries = list(it)
        except OSError as e:
            rep.fail("unreadable", f"{rel or '.'}: {e.strerror}")
            continue
        for ent in entries:
            r = f"{rel}/{ent.name}" if rel else ent.name
            if ent.is_symlink():
                rep.fail("symlink", f"{r}: symlink inside bundle")
            elif ent.is_dir(follow_symlinks=False):
                stack.append(r)
            else:
                found.append(r)
    return found


def verify(root, size_cap=DEFAULT_MAX_BYTES, manifest_sha256=None):
    rep = Report()
    try:
        rst = os.lstat(root)
    except OSError as e:
        rep.fail("unreadable", f"bundle dir {root}: {e.strerror}")
        return rep, None
    if stat.S_ISLNK(rst.st_mode) or not stat.S_ISDIR(rst.st_mode):
        rep.fail("unreadable", f"bundle dir {root} is a symlink or not a directory")
        return rep, None
    manifest = parse_manifest(root, rep, manifest_sha256)
    if manifest is None:
        return rep, None
    listed = {}
    for e in manifest["files"]:
        listed[e["path"]] = e
        res = hash_file(root, e["path"], size_cap, rep)
        if res is None:
            continue
        digest, size = res
        if digest != e["sha256"]:
            rep.fail("changed", f"{e['path']}: sha256 {digest} != manifest {e['sha256']}")
        elif size != e["bytes"]:
            rep.fail("changed", f"{e['path']}: {size} bytes != manifest {e['bytes']}")
    for r in sorted(walk_tree(root, rep)):
        if r != MANIFEST_FILE and r not in listed:
            rep.fail("extra", f"{r}: present in bundle but not in manifest")
    rep.note(f"checked {len(listed)} file(s); {len(manifest.get('omitted', []))} omitted "
             "entr(ies), each with a reason")
    return rep, manifest


def main(argv=None):
    ap = argparse.ArgumentParser(description="Offline verification of a KubeMetal support bundle.")
    ap.add_argument("bundle_dir")
    ap.add_argument("--manifest-sha256", help="digest of manifest.json recorded out of band")
    ap.add_argument("--max-bytes", type=int, default=DEFAULT_MAX_BYTES,
                    help=f"per-file read cap (default {DEFAULT_MAX_BYTES})")
    try:
        args = ap.parse_args(argv)
    except SystemExit as e:
        return 2 if e.code else 0
    if args.max_bytes <= 0:
        print("usage: --max-bytes must be positive", file=sys.stderr)
        return 2
    pin = args.manifest_sha256
    if pin is not None and not SHA_RE.match(pin.lower()):
        print("usage: --manifest-sha256 must be 64 hex chars", file=sys.stderr)
        return 2
    rep, manifest = verify(args.bundle_dir, args.max_bytes, pin)
    for n in rep.notes:
        print(f"note: {_safe(n)}")
    for cls, msg in rep.problems:
        print(f"FAIL[{_safe(cls)}] {_safe(msg)}")
    if manifest is None and rep.problems and all(c in READ_ERRORS for c, _ in rep.problems):
        print("RESULT: NOT VERIFIED (bundle could not be read)")
        return 2
    if rep.problems:
        print(f"RESULT: FAILED ({len(rep.problems)} problem(s))")
        return 1
    scope = "manifest.json matched pinned digest" if pin else \
        "manifest.json unauthenticated, the service writes no self-hash"
    print(f"RESULT: OK (file hashes verified; {scope})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
