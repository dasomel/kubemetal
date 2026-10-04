import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from check_tauri_versions import PAIRS, check

ROOT = Path(__file__).resolve().parents[2]


def load(name):
  spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
  module = importlib.util.module_from_spec(spec)
  spec.loader.exec_module(module)
  return module


class CouplingTests(unittest.TestCase):
  def setUp(self):
    self.cargo = ''.join(f'[[package]]\nname = "{crate}"\nversion = "2.1.2"\n' for crate, _, _ in PAIRS)
    self.pnpm = ''.join(f"  '@tauri-apps/{pkg}@2.1.2':\n" for _, pkg, _ in PAIRS)

  def test_synced(self):
    self.assertEqual(check(self.cargo, self.pnpm), [])

  def test_core_patch_allowed(self):
    self.assertEqual(check(self.cargo, self.pnpm.replace('api@2.1.2', 'api@2.1.9')), [])

  def test_each_pair_rejects_minor_and_major(self):
    for _, pkg, _ in PAIRS:
      for version in ('2.2.2', '3.1.2'):
        with self.subTest(pkg=pkg, version=version):
          self.assertTrue(check(self.cargo, self.pnpm.replace(f'{pkg}@2.1.2', f'{pkg}@{version}')))

  def test_plugins_reject_patch(self):
    for pkg in ('plugin-dialog', 'plugin-opener'):
      self.assertTrue(check(self.cargo, self.pnpm.replace(f'{pkg}@2.1.2', f'{pkg}@2.1.3')))

  def test_missing_malformed_duplicate_and_prerelease(self):
    for text in ('', self.pnpm.replace('api@2.1.2', 'api@garbage'),
                 self.pnpm + "  '@tauri-apps/api@2.2.0':\n",
                 self.pnpm.replace('api@2.1.2', 'api@2.1.2-beta.1')):
      self.assertTrue(check(self.cargo, text))
    self.assertTrue(check('', self.pnpm))
    self.assertTrue(check(self.cargo + '[[package]]\nname = "tauri"\nversion = "2.2.0"\n', self.pnpm))


class TracePolicyTests(unittest.TestCase):
  def test_requirement_cli_lock_only_and_mixed(self):
    with tempfile.TemporaryDirectory() as directory:
      changed = Path(directory) / 'changed.txt'
      for paths, code in ((['src-tauri/Cargo.lock'], 0),
                          (['src-tauri/Cargo.lock', 'package.json', 'pnpm-lock.yaml'], 0),
                          (['src-tauri/Cargo.lock', 'src-tauri/Cargo.toml'], 1)):
        changed.write_text('\n'.join(paths) + '\n')
        result = subprocess.run([sys.executable, str(ROOT / 'scripts/ci/check-agent-trace-requirement.py'),
                                 '--policy', str(ROOT / '.agents/evals/risk-policy.json'),
                                 '--changed-files', str(changed)], capture_output=True, text=True)
        self.assertEqual(result.returncode, code, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report['risk'], 'high')
        self.assertEqual(report['traceRequired'], bool(code))


  def test_lockfile_high_risk_set_and_evidence_consistency(self):
    requirement = load('check-agent-trace-requirement')
    evidence = load('check-agent-trace-evidence')
    policy = json.loads((ROOT / '.agents/evals/risk-policy.json').read_text())
    lock = 'src-tauri/Cargo.lock'
    cases = (([lock], True), ([], False),
             (['src-tauri/Cargo.toml'], False),
             ([lock, 'src-tauri/Cargo.toml'], False),
             ([lock, 'src-tauri/src/lib.rs'], False),
             ([lock, '.github/workflows/ci.yml'], False),
             ([lock, '.agents/evals/risk-policy.json'], False),
             ([lock, 'scripts/ci/check_versions.py'], False),
             ([lock, 'pnpm-lock.yaml', 'package.json'], True),
             ([lock, 'README.md'], True),
             (['pnpm-lock.yaml'], False))
    for paths, exempt in cases:
      with self.subTest(paths=paths):
        has_high = any(requirement.classify([p], policy)[0] == 'high' for p in paths)
        self.assertEqual(requirement.trace_exempt(paths, policy), exempt)
        self.assertEqual(bool(evidence.high(paths, policy)), has_high and not exempt)
    # Low-risk-only change: nothing required, nothing exempt.
    self.assertEqual(requirement.classify(['pnpm-lock.yaml'], policy)[0], 'low')
    self.assertEqual(evidence.high(['pnpm-lock.yaml'], policy), [])
    self.assertEqual(requirement.classify(['src-tauri/Cargo.lock'], policy)[0], 'high')
    policy.pop('traceExemptHighRiskFileSets')
    self.assertFalse(requirement.trace_exempt(['src-tauri/Cargo.lock'], policy))


if __name__ == '__main__':
  unittest.main()
