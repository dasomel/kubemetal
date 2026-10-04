#!/usr/bin/env python3
"""D43 read-only divergence gate; no VM commands are executed."""
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
INVALID = ("", " ", "default", "colima", "colima-foo", "--foo", "../x", "A", "a b")


def rust_violations(source, process=False):
    # Preserve string tokens while removing comments (including multi-line comments).
    source = re.sub(r'("(?:\\.|[^"\\])*")|//[^\n]*|/\*.*?\*/',
                    lambda m: m.group(1) or "", source, flags=re.S)
    if process:
        # The only allowed binary definition site is this function body.
        source = re.sub(r'pub fn colima_command\(\).*?\n}', '', source, flags=re.S)
    problems = []
    if re.search(r'(?:external_command|Command::new)\s*\(\s*"colima"\s*\)', source):
        problems.append("bare colima command")
    if re.search(r'(?:args\s*\(|arg\s*\(|CONTEXT\w*\s*[:=]|target_context\s*:|unwrap_or_else\s*\()[^;]*?"colima"', source, re.S):
        problems.append("hardcoded context")
    return problems


def script_violations(source):
    source = re.sub(r'(?m)^\s*#.*$', '', source)
    source = source.replace('\\\n', ' ')
    problems = []
    # Scan shell/Make command segments and Python argument lists.
    for match in re.finditer(r'\bcolima\b([^\n;|&]*)', source):
        tail = match.group(1)
        if re.search(r'\b(start|stop|status|ssh|delete|list|nerdctl)\b', tail):
            if not re.search(r'--profile(?:\s|["\x27,])', tail):
                problems.append("colima invocation without profile")
    if re.search(r'(?:--context|--kube-context)[\s,]*["\x27]colima["\x27]|:-colima\b', source):
        problems.append("hardcoded context")
    if re.search(r'(?m)\b(?:COLIMA_CONTEXT|CONTEXT)\s*:?=\s*["\x27]?colima(?:["\x27]|\s*$)', source):
        problems.append("hardcoded context assignment")
    return problems


def assert_consumers(root):
    process = root / "src-tauri/src/services/process.rs"
    assert 'include_str!("../../../scripts/colima-profile.txt")' in process.read_text()
    assert '.env("DOCKER_CONTEXT", colima_context())' in (root / "src-tauri/src/commands/colima.rs").read_text()
    for path in (root / "src-tauri/src").rglob("*.rs"):
        assert not rust_violations(path.read_text(), path == process), path
    for path in [root / "Makefile", *(root / "scripts").rglob("*.sh"),
                 *(root / "scripts").rglob("*.py")]:
        # This guard intentionally embeds forbidden sources as negative controls.
        if path.resolve() == Path(__file__).resolve():
            continue
        assert not script_violations(path.read_text()), path
    for path in (root / "src").rglob("*"):
        if path.suffix in {".ts", ".tsx"}:
            assert not re.search(r'["\x27]colima(?:-kubemetal)?["\x27]', path.read_text()), path


class ColimaProfileTests(unittest.TestCase):
    def test_consumers_agree(self):
        assert_consumers(ROOT)

    def shell(self, value=None, script=None):
        env = dict(os.environ)
        env.pop("KUBEMETAL_COLIMA_PROFILE", None)
        if value is not None:
            env["KUBEMETAL_COLIMA_PROFILE"] = value
        return subprocess.run(
            ["bash", "-c", 'source "$1"; printf "%s\\n%s" "$KUBEMETAL_COLIMA_PROFILE" "$COLIMA_CONTEXT"',
             "profile", str(script or ROOT / "scripts/colima-profile.sh")],
            env=env, text=True, capture_output=True)

    def test_shell_accept_reject(self):
        default = (ROOT / "scripts/colima-profile.txt").read_text().strip()
        for value in (None, "kubemetal", "my-vm2", "  my-vm2 \n"):
            result = self.shell(value)
            self.assertEqual(result.returncode, 0, result.stderr)
            profile = default if value is None else value.strip()
            self.assertEqual(result.stdout, f"{profile}\ncolima-{profile}")
        for value in INVALID:
            result = self.shell(value)
            self.assertNotEqual(result.returncode, 0, value)
            self.assertIn("Invalid KUBEMETAL_COLIMA_PROFILE", result.stderr)

    def test_shell_missing_empty_source(self):
        with tempfile.TemporaryDirectory() as tmp:
            script = Path(tmp) / "colima-profile.sh"
            script.write_text((ROOT / "scripts/colima-profile.sh").read_text())
            self.assertNotEqual(self.shell("my-vm2", script).returncode, 0)
            (Path(tmp) / "colima-profile.txt").write_text(" \n")
            self.assertNotEqual(self.shell("my-vm2", script).returncode, 0)

    def test_make_defaults_override_rejections(self):
        for value in (None, "kubemetal", "my-vm2", "  my-vm2 ", *INVALID):
            env = dict(os.environ)
            env.pop("KUBEMETAL_COLIMA_PROFILE", None)
            if value is not None:
                env["KUBEMETAL_COLIMA_PROFILE"] = value
            result = subprocess.run(["make", "-n", "status"], cwd=ROOT, env=env,
                                    text=True, capture_output=True)
            if value in INVALID:
                self.assertNotEqual(result.returncode, 0, value)
            else:
                self.assertEqual(result.returncode, 0, result.stderr)
                profile = (value or (ROOT / "scripts/colima-profile.txt").read_text()).strip()
                self.assertIn(f"colima --profile {profile} status --json", result.stdout)
                self.assertIn(f"kubectl --context colima-{profile}", result.stdout)

    def test_negative_controls(self):
        for source in ('.args(["--context",\n "colima", "get"])',
                       'external_command(\n"colima")', 'Command::new("colima")'):
            self.assertTrue(rust_violations(source), source)
        self.assertTrue(rust_violations('fn other() { external_command("colima"); }', True))
        good = '\n'.join(['\tcolima --profile $(KUBEMETAL_COLIMA_PROFILE) start'] * 3)
        self.assertFalse(script_violations(good))
        self.assertTrue(script_violations("COLIMA_CONTEXT := colima"))
        self.assertTrue(script_violations(good + '\n\tcolima start'))
        self.assertTrue(script_violations('["colima", "ssh", "--", "true"]'))
        self.assertFalse(rust_violations('// external_command("colima")\n/* --context "colima" */'))


if __name__ == "__main__":
    unittest.main()
