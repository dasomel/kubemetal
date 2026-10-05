"""Tests for verify_support_bundle.py.

Fixtures are SYNTHETIC: built here to match the manifest the Rust service serializes
(src-tauri/src/services/support_bundle.rs). Set KUBEMETAL_REAL_BUNDLE to a bundle directory
produced by the real writer to also run the verifier against it.
Run: python3 -m unittest discover -s scripts/support -p 'test_*.py'
"""
import contextlib
import hashlib
import io
import json
import os
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import verify_support_bundle as v  # noqa: E402


def sha(b):
    return hashlib.sha256(b).hexdigest()


def build_bundle(root, extra_files=None, omitted=None):
    files = {
        "app_info.json": b'{"app_name": "KubeMetal"}',
        "health_summary.json": b'{"status":"healthy"}',
        "logs/a.log": b"line 1\nBearer [REDACTED]\n",
    }
    files.update(extra_files or {})
    entries = []
    for rel, data in sorted(files.items()):
        p = os.path.join(root, rel)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "wb") as f:
            f.write(data)
        entries.append({"path": rel, "sha256": sha(data), "bytes": len(data)})
    manifest = {
        "schema_version": 1,
        "redaction": {"mode": "best-effort", "review_before_sharing": True, "rules_version": 2},
        "created_at": "2026-10-06T01:02:03Z",
        "files": entries,
    }
    if omitted:
        manifest["omitted"] = omitted
    write_manifest(root, manifest)
    return manifest


def write_manifest(root, manifest):
    with open(os.path.join(root, "manifest.json"), "w") as f:
        json.dump(manifest, f, indent=2)


def read_manifest(root):
    with open(os.path.join(root, "manifest.json")) as f:
        return json.load(f)


def read_bytes(p):
    with open(p, "rb") as f:
        return f.read()


def write_bytes(p, b):
    with open(p, "wb") as f:
        f.write(b)


def run(root, *args):
    out = io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(out):
        code = v.main([root, *args])
    return code, out.getvalue()


class VerifyTest(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.mkdtemp(prefix="kubemetal-bundle-test-")
        self.addCleanup(shutil.rmtree, self.root, ignore_errors=True)

    def assertFails(self, cls, code=1, *args):
        got, out = run(self.root, *args)
        self.assertEqual(got, code, out)
        self.assertIn(f"FAIL[{cls}]", out)
        self.assertNotIn("RESULT: OK", out)
        return out

    def test_valid_bundle_passes_and_states_manifest_is_unauthenticated(self):
        build_bundle(self.root, omitted=[
            {"path": "logs/corrupt.bin", "omitted": "Binary content containing null bytes cannot be safely scanned"},
            {"path": "logs/", "omitted": "Failed to open log directory: nope"}])
        code, out = run(self.root)
        self.assertEqual(code, 0, out)
        self.assertIn("RESULT: OK", out)
        self.assertIn("unauthenticated", out)
        self.assertIn("mode=best-effort", out)

    def test_pinned_manifest_digest(self):
        build_bundle(self.root)
        good = sha(read_bytes(os.path.join(self.root, "manifest.json")))
        code, out = run(self.root, "--manifest-sha256", good)
        self.assertEqual(code, 0, out)
        self.assertIn("matched pinned digest", out)
        self.assertFails("manifest-hash", 1, "--manifest-sha256", "0" * 64)

    def test_one_byte_flipped(self):
        build_bundle(self.root)
        p = os.path.join(self.root, "logs/a.log")
        data = bytearray(read_bytes(p))
        data[0] ^= 0x01
        write_bytes(p, bytes(data))
        out = self.assertFails("changed")
        self.assertIn("logs/a.log", out)

    def test_size_changed_with_same_prefix(self):
        build_bundle(self.root)
        with open(os.path.join(self.root, "logs/a.log"), "ab") as f:
            f.write(b"x")
        self.assertFails("changed")

    def test_missing_file(self):
        build_bundle(self.root)
        os.remove(os.path.join(self.root, "logs/a.log"))
        self.assertFails("missing")

    def test_extra_file(self):
        build_bundle(self.root)
        with open(os.path.join(self.root, "logs/planted.log"), "w") as f:
            f.write("x")
        out = self.assertFails("extra")
        self.assertIn("logs/planted.log", out)

    def test_path_traversal_entries(self):
        for bad in ("../outside.txt", "/etc/passwd", "logs/../../x", "a//b", "./a", "a\\b"):
            with self.subTest(bad=bad):
                shutil.rmtree(self.root)
                os.makedirs(self.root)
                m = build_bundle(self.root)
                m["files"].append({"path": bad, "sha256": "0" * 64, "bytes": 0})
                write_manifest(self.root, m)
                self.assertFails("path-unsafe")

    def test_traversal_in_omitted_path(self):
        m = build_bundle(self.root)
        m["omitted"] = [{"path": "../x", "omitted": "reason"}]
        write_manifest(self.root, m)
        self.assertFails("path-unsafe")

    def test_symlinked_listed_file(self):
        build_bundle(self.root)
        outside = os.path.join(self.root, "..", os.path.basename(self.root) + "-outside")
        with open(outside, "wb") as f:
            f.write(b"line 1\nBearer [REDACTED]\n")  # same bytes: only the symlink rule can reject
        self.addCleanup(os.remove, outside)
        p = os.path.join(self.root, "logs/a.log")
        os.remove(p)
        os.symlink(outside, p)
        self.assertFails("symlink")

    def test_symlinked_directory_component_and_unlisted_symlink(self):
        build_bundle(self.root)
        os.symlink(self.root, os.path.join(self.root, "loop"))
        self.assertFails("symlink")

    def test_symlinked_manifest(self):
        build_bundle(self.root)
        real = os.path.join(self.root, "real-manifest")
        os.rename(os.path.join(self.root, "manifest.json"), real)
        os.symlink(real, os.path.join(self.root, "manifest.json"))
        self.assertFails("symlink")

    def test_symlinked_bundle_root(self):
        build_bundle(self.root)
        link = self.root + "-link"
        os.symlink(self.root, link)
        self.addCleanup(os.remove, link)
        got, out = run(link)
        self.assertEqual(got, 2, out)
        self.assertIn("NOT VERIFIED", out)

    def test_malformed_manifest_json(self):
        build_bundle(self.root)
        with open(os.path.join(self.root, "manifest.json"), "w") as f:
            f.write("{not json")
        self.assertFails("manifest-malformed")

    def test_manifest_schema_violations(self):
        cases = {
            "unknown top-level field": lambda m: m.update(extra=1),
            "missing created_at": lambda m: m.pop("created_at"),
            "bad file sha": lambda m: m["files"][0].update(sha256="XYZ"),
            "bytes is bool": lambda m: m["files"][0].update(bytes=True),
            "bytes negative": lambda m: m["files"][0].update(bytes=-1),
            "unknown file field": lambda m: m["files"][0].update(mode=1),
            "files not a list": lambda m: m.update(files={}),
            "duplicate path": lambda m: m["files"].append(dict(m["files"][0])),
            "lists manifest.json": lambda m: m["files"].append(
                {"path": "manifest.json", "sha256": "0" * 64, "bytes": 0}),
            "bad created_at": lambda m: m.update(created_at="yesterday"),
        }
        for name, mutate in cases.items():
            with self.subTest(name):
                shutil.rmtree(self.root)
                os.makedirs(self.root)
                m = build_bundle(self.root)
                mutate(m)
                write_manifest(self.root, m)
                got, out = run(self.root)
                self.assertEqual(got, 1, out)
                self.assertRegex(out, r"FAIL\[manifest-malformed\]")

    def test_duplicate_json_keys_rejected(self):
        build_bundle(self.root)
        mp = os.path.join(self.root, "manifest.json")
        text = read_bytes(mp).decode()
        write_bytes(mp, text.replace('"schema_version": 1,', '"schema_version": 1, "schema_version": 1,').encode())
        self.assertFails("manifest-malformed")

    def test_unsupported_schema_version(self):
        m = build_bundle(self.root)
        m["schema_version"] = 2
        write_manifest(self.root, m)
        self.assertFails("schema-version")

    def test_missing_manifest_is_exit_2(self):
        build_bundle(self.root)
        os.remove(os.path.join(self.root, "manifest.json"))
        got, out = run(self.root)
        self.assertEqual(got, 2, out)
        self.assertIn("NOT VERIFIED", out)

    def test_redaction_absent_or_malformed(self):
        m = build_bundle(self.root)
        del m["redaction"]
        write_manifest(self.root, m)
        self.assertFails("manifest-malformed")
        m["redaction"] = {"mode": "best-effort", "review_before_sharing": "yes", "rules_version": 2}
        write_manifest(self.root, m)
        self.assertFails("redaction-malformed")
        m["redaction"] = {"mode": "best-effort", "review_before_sharing": True}
        write_manifest(self.root, m)
        self.assertFails("manifest-malformed")

    def test_omitted_without_reason(self):
        for reason in ("", "   ", None, 3):
            with self.subTest(reason=reason):
                m = build_bundle(self.root)
                m["omitted"] = [{"path": "logs/x.log", "omitted": reason}]
                write_manifest(self.root, m)
                self.assertFails("omitted-no-reason")

    def test_omitted_missing_reason_field(self):
        m = build_bundle(self.root)
        m["omitted"] = [{"path": "logs/x.log"}]
        write_manifest(self.root, m)
        self.assertFails("manifest-malformed")

    def test_oversized_file_not_hashed(self):
        build_bundle(self.root)
        self.assertFails("size", 1, "--max-bytes", "5")

    def test_required_files_listed(self):
        m = build_bundle(self.root)
        m["files"] = [e for e in m["files"] if e["path"] != "health_summary.json"]
        write_manifest(self.root, m)
        self.assertFails("incomplete")

    @unittest.skipUnless(os.environ.get("KUBEMETAL_REAL_BUNDLE"), "no real bundle supplied")
    def test_real_bundle_from_app(self):
        code, out = run(os.environ["KUBEMETAL_REAL_BUNDLE"])
        self.assertEqual(code, 0, out)


if __name__ == "__main__":
    unittest.main()
