#!/usr/bin/env python3
"""D43 divergence gate. Static/read-only: never launches a VM or kubectl."""
import os
from pathlib import Path
import re
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[2]


def assert_consumers(root):
    profile = (root / 'scripts/colima-profile.txt').read_text().strip()
    assert profile and profile != 'default', 'managed profile must be named'
    process = (root / 'src-tauri/src/services/process.rs').read_text()
    assert 'include_str!("../../../scripts/colima-profile.txt")' in process
    assert 'format!("colima-{}", colima_profile())' in process
    assert 'command.args(["--profile", colima_profile()])' in process
    lifecycle = (root / 'src-tauri/src/commands/colima.rs').read_text()
    assert lifecycle.count('colima_command()?') == 3
    assert 'external_command("colima")' not in lifecycle
    make = (root / 'Makefile').read_text()
    assert 'COLIMA_PROFILE ?= $(shell cat scripts/colima-profile.txt)' in make
    assert 'COLIMA_CONTEXT := colima-$(COLIMA_PROFILE)' in make
    assert make.count('colima --profile $(COLIMA_PROFILE)') == 3
    hook = (root / 'src/hooks/useDeployTarget.ts').read_text()
    assert "invoke<string>('get_managed_colima_context')" in hook
    for path in (root / 'src').rglob('*'):
        if path.suffix in {'.ts', '.tsx'}:
            assert not re.search(r'''["']colima(?:-kubemetal)?["']''', path.read_text()), path
    shell = [
        'scripts/airgap/install_from_airgap.sh', 'scripts/airgap/verify_offline_images.sh',
        'scripts/k8s/remote-reader/setup-remote-reader.sh',
        'scripts/k8s/remote-reader/teardown-remote-reader.sh',
        'scripts/mlx/verify_local_inference_bridge.sh',
    ]
    for path in shell:
        text = (root / path).read_text()
        assert '/colima-profile.sh"' in text, path
        assert not re.search(r':-colima\b|\bcolima (?:status|ssh|start|stop)\b', text), path
    for path in (root / 'scripts/e2e').glob('0[34]*.py'):
        text = path.read_text()
        assert 'colima-profile.txt' in text and 'f"colima-{PROFILE}"' in text
    for path in (root / 'src-tauri/src').rglob('*.rs'):
        text = path.read_text().split('#[cfg(test)]')[0]
        # Binary names and health component ids are legitimate; context fallbacks are not.
        assert not re.search(r'(?:CONTEXT[^\n]*=|unwrap_or_else[^\n]*|target_context:)[^\n]*"colima"', text), path


class ColimaProfileTests(unittest.TestCase):
    def test_consumers_agree(self):
        assert_consumers(ROOT)

    def test_make_defaults_and_override_without_executing_recipes(self):
        for override in (None, 'isolated-test'):
            env = dict(os.environ)
            for key in ('COLIMA_PROFILE', 'CONTEXT', 'NAMESPACE'):
                env.pop(key, None)
            if override:
                env['COLIMA_PROFILE'] = override
            output = subprocess.check_output(['make', '-n', 'status'], cwd=ROOT, env=env, text=True)
            profile = override or (ROOT / 'scripts/colima-profile.txt').read_text().strip()
            self.assertIn(f'colima --profile {profile} status --json', output)
            self.assertIn(f'kubectl --context colima-{profile}', output)

    def test_shell_default_and_override_derive_context(self):
        for override in (None, 'isolated-test'):
            env = dict(os.environ)
            env.pop('COLIMA_PROFILE', None)
            if override:
                env['COLIMA_PROFILE'] = override
            output = subprocess.check_output([
                'bash', '-c', 'source scripts/colima-profile.sh; printf "%s\\n%s" "$COLIMA_PROFILE" "$COLIMA_CONTEXT"',
            ], cwd=ROOT, env=env, text=True)
            profile = override or (ROOT / 'scripts/colima-profile.txt').read_text().strip()
            self.assertEqual(output, f'{profile}\ncolima-{profile}')

    def test_negative_control_rejects_hardcoded_make_context(self):
        from unittest.mock import patch
        original = Path.read_text
        def drift(path, *args, **kwargs):
            text = original(path, *args, **kwargs)
            return text.replace('COLIMA_CONTEXT := colima-$(COLIMA_PROFILE)', 'COLIMA_CONTEXT := colima') if path.name == 'Makefile' else text
        with patch.object(Path, 'read_text', drift):
            with self.assertRaises(AssertionError):
                assert_consumers(ROOT)


if __name__ == '__main__':
    unittest.main()
