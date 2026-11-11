"""Run the real shell/classifier with disposable CPU-only fake tools."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
MATRIX = ROOT / 'scripts/cumetal/matrix'
SPEC = importlib.util.spec_from_file_location('matrix_evidence', MATRIX / 'evidence.py')
evidence_module = importlib.util.module_from_spec(SPEC)
if SPEC.origin and Path(SPEC.origin).exists():
    SPEC.loader.exec_module(evidence_module)


class EvidenceTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.scripts = self.root / 'matrix'
        shutil.copytree(MATRIX, self.scripts)
        self.tool = self.root / 'tool'
        self.tool.write_text('''#!/usr/bin/env python3
import os, pathlib, sys
if '--version' in sys.argv or 'doctor' in sys.argv:
    print('fake tool full diagnostics'); sys.exit(0)
if os.environ.get('CASE') == 'compile':
    print('full diagnostic ' * 100); sys.exit(2)
if os.environ.get('CASE') == 'compile_timeout': sys.exit(142)
out = pathlib.Path(sys.argv[sys.argv.index('-o') + 1])
out.write_text(''' + repr('''#!/bin/bash
if [ "$CASE" = run ]; then echo 'run failure'; exit 20; fi
if [ "$CASE" = timeout ]; then exit 142; fi
echo 'successful stdout retained'
echo 'CUMETAL_PROVENANCE event=kernel_launch device=apple_gpu source=jit semantic_quality=exact launch_success=true'
echo 'MATRIX_RESULT status=PASS digest=abc'
''') + ''')
out.chmod(0o700)
''')
        self.tool.chmod(0o700)
        script = (self.scripts / 'run_matrix.sh').read_text()
        for binary in ('/opt/homebrew/opt/llvm/bin/clang++', '/opt/homebrew/bin/cumetalc',
                       '/opt/homebrew/bin/cumetal', '/usr/bin/xcrun'):
            script = script.replace(binary, str(self.tool))
        script = script.replace('PY=/usr/bin/python3', f'PY={sys.executable}')
        (self.scripts / 'run_matrix.sh').write_text(script)
        self.output = self.root / 'out.json'

    def run_matrix(self, case='ok', probes=('launch_1d',)):
        return subprocess.run(['bash', str(self.scripts / 'run_matrix.sh'), str(self.output), *probes],
                              env={**os.environ, 'CASE': case}, capture_output=True, text=True)

    def reclassify(self, evidence, target):
        return subprocess.run([sys.executable, '-I', '-B', str(MATRIX / 'classify.py'),
                               str(evidence), str(target), '3'], env={'PATH': '/nonexistent'},
                              capture_output=True, text=True)

    def test_outcomes_and_offline_reclassification(self):
        for case, expected in (('ok', 'supported'), ('compile', 'unavailable'),
                               ('compile_timeout', 'failed'), ('run', 'failed'), ('timeout', 'failed')):
            with self.subTest(case=case):
                self.output = self.root / f'{case}.json'
                proc = self.run_matrix(case)
                self.assertEqual(proc.returncode, 0, proc.stderr)
                doc = json.loads(self.output.read_text())
                self.assertIn('evidence', doc)
                evidence = self.output.parent / doc['evidence']['directory']
                self.assertEqual(doc['summary'], {expected: 1})
                self.assertTrue((evidence / 'manifest/doctor.log').is_file())
                self.assertTrue(doc['evidence']['files']['manifest/sources/launch_1d.cu'])
                self.assertFalse(list(evidence.rglob('*.bin')) + list(evidence.rglob('*.o')))
                replay = self.root / f'{case}-replay.json'
                proc = self.reclassify(evidence, replay)
                self.assertEqual(proc.returncode, 0, proc.stderr)
                self.assertEqual(json.loads(replay.read_text())['results'], doc['results'])
                log = evidence / 'launch_1d/aot.compile.log'
                if case == 'compile':
                    self.assertGreater(len(log.read_text()), 400)
                if case == 'ok':
                    self.assertIn('successful stdout retained', (evidence / 'launch_1d/aot.run1.out').read_text())
                log.write_text('tampered')
                proc = self.reclassify(evidence, self.root / f'{case}-tampered.json')
                self.assertNotEqual(proc.returncode, 0)
                self.assertIn('hash', proc.stderr.lower())

    def test_destination_collision(self):
        for suffix in ('.json', '.md'):
            with self.subTest(suffix=suffix):
                path = self.output.with_suffix(suffix)
                path.write_text('keep')
                proc = self.run_matrix()
                self.assertNotEqual(proc.returncode, 0)
                self.assertEqual(path.read_text(), 'keep')
                self.assertNotIn('wrote ', proc.stderr)
                path.unlink()

    def test_retention_failure_and_partial_execution(self):
        self.output = self.root / 'absent/out.json'
        proc = self.run_matrix()
        self.assertNotEqual(proc.returncode, 0)
        self.assertNotIn('wrote ', proc.stderr)
        self.output = self.root / 'partial.json'
        proc = self.run_matrix(probes=('launch_1d', 'launch_1d'))
        self.assertNotEqual(proc.returncode, 0)
        self.assertFalse(self.output.exists())

    def test_seal_failure_does_not_publish_success(self):
        (self.scripts / 'evidence.py').write_text('raise OSError("injected preservation failure")\n')
        proc = self.run_matrix()
        self.assertNotEqual(proc.returncode, 0)
        self.assertFalse(self.output.exists())
        self.assertNotIn('wrote ', proc.stderr)

    def test_partial_attempt_is_sealed_and_refuses_replay(self):
        proc = self.run_matrix(probes=('launch_1d', 'launch_1d'))
        self.assertNotEqual(proc.returncode, 0)
        evidence = next(self.root.glob('*.evidence.*'))
        self.assertFalse(json.loads((evidence / 'manifest/evidence.json').read_text())['complete'])
        proc = self.reclassify(evidence, self.root / 'replay.json')
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn('incomplete', proc.stderr)

    def test_inventory_rejects_changes_and_invalid_entries(self):
        raw = self.root / 'raw'
        (raw / 'manifest').mkdir(parents=True)
        log = raw / 'manifest/log'
        log.write_text('raw')
        evidence_module.seal(raw, 3, True)
        self.assertTrue(evidence_module.verify(raw, 3)['complete'])
        for repeats in (2, 0):
            with self.assertRaisesRegex(ValueError, 'repeats'):
                evidence_module.verify(raw, repeats)
        log.unlink()
        with self.assertRaisesRegex(ValueError, 'hash'):
            evidence_module.verify(raw, 3)
        log.write_text('raw')
        extra = raw / 'extra'
        extra.write_text('unlisted')
        with self.assertRaisesRegex(ValueError, 'hash'):
            evidence_module.verify(raw, 3)
        extra.unlink()
        for name in ('tool.bin', 'tool.o'):
            product = raw / name
            product.write_bytes(b'binary')
            with self.assertRaisesRegex(ValueError, 'build product'):
                evidence_module.verify(raw, 3)
            product.unlink()
        extra.symlink_to(log)
        with self.assertRaisesRegex(ValueError, 'symlink'):
            evidence_module.verify(raw, 3)

    def test_missing_index_and_invalid_probe_are_rejected(self):
        self.assertEqual(self.run_matrix().returncode, 0)
        evidence = next(self.root.glob('*.evidence.*'))
        (evidence / 'manifest/evidence.json').unlink()
        self.assertNotEqual(self.reclassify(evidence, self.root / 'replay.json').returncode, 0)
        self.output = self.root / 'invalid.json'
        self.assertNotEqual(self.run_matrix(probes=('../escape',)).returncode, 0)


if __name__ == '__main__':
    unittest.main()
