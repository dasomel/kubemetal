"""Synthetic on-disk evidence; no CuMetal, GPU or subprocess mocks required."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / 'scripts/cumetal/matrix/classify.py'
SPEC = importlib.util.spec_from_file_location('matrix_classifier', SCRIPT)
classifier = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(classifier)

GPU = ('CUMETAL_PROVENANCE event=kernel_launch device=apple_gpu source=jit '
       'semantic_quality=exact launch_success=true device_name="Apple GPU"\n')
RESULT = 'MATRIX_RESULT probe=launch_1d status=PASS digest=abc\n'


class MatrixClassifierTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.work = Path(self.tmp.name)
        self.probe = self.work / 'launch_1d'
        self.probe.mkdir()
        for path in ('aot', 'jit'):
            self.write(f'{path}.compile.rc', '0')
            for run in range(1, 4):
                self.write(f'{path}.run{run}.rc', '0')
                self.write(f'{path}.run{run}.out', GPU + RESULT)

    def write(self, name, value):
        (self.probe / name).write_text(value)

    def classify(self, kernel=True):
        return classifier.classify_path(str(self.probe), 'jit', kernel)

    def test_every_run_retains_provenance(self):
        for kernel in (True, False):
            with self.subTest(kernel=kernel):
                result = self.classify(kernel)
                self.assertEqual(result['class'], 'supported')
                for run in result['runs']:
                    self.assertEqual(run['provenance']['devices'], ['apple_gpu'])
                self.assertEqual(result['provenance']['kernel_launches'], 3)

    def test_gpu_evidence_in_only_one_run_is_partial(self):
        for kernel in (True, False):
            for present in range(1, 4):
                with self.subTest(kernel=kernel, present=present):
                    for run in range(1, 4):
                        self.write(f'jit.run{run}.out', (GPU if run == present else '') + RESULT)
                    self.assertEqual(self.classify(kernel)['class'], 'partial')

    def test_each_run_requires_valid_gpu_launches(self):
        mutations = ('', GPU.replace('apple_gpu', 'cpu'),
                     GPU.replace('true', 'false'), GPU.replace('exact', 'emulated'),
                     GPU + GPU.replace('apple_gpu', 'cpu'),
                     GPU.replace(' launch_success=true', ''),
                     GPU + 'CUMETAL_PROVENANCE event=kernel_launch\n',
                     GPU.replace(' source=jit', ''),
                     GPU.replace('source=jit', 'source=""'),
                     GPU.rstrip() + ' unsupported_reason="unsupported op"\n')
        for kernel in (True, False):
            for evidence in mutations:
                with self.subTest(kernel=kernel, evidence=evidence):
                    self.write('jit.run2.out', evidence + RESULT)
                    self.assertEqual(self.classify(kernel)['class'], 'partial')

    def test_library_without_provenance_is_partial(self):
        for run in range(1, 4):
            self.write(f'jit.run{run}.out', RESULT)
        self.assertEqual(self.classify(False)['class'], 'partial')

    def test_failed_execution_and_parity(self):
        cases = ((142, GPU + RESULT, 'failed'), (1, GPU + RESULT, 'failed'),
                 (0, GPU + RESULT.replace('PASS', 'MISMATCH'), 'failed'),
                 (0, GPU + 'MATRIX_RESULT status=MISMATCH\n', 'failed'),
                 (20, 'unsupported API', 'unavailable'), (20, 'API error', 'failed'),
                 (139, GPU + RESULT, 'failed'), (0, GPU, 'failed'),
                 (0, GPU + 'MATRIX_RESULT status=PASS\n', 'failed'))
        for rc, output, expected in cases:
            with self.subTest(rc=rc, output=output):
                self.write('jit.run2.rc', str(rc))
                self.write('jit.run2.out', output)
                self.assertEqual(self.classify()['class'], expected)

    def test_missing_run_is_failed(self):
        for suffix in ('rc', 'out'):
            with self.subTest(suffix=suffix):
                file = self.probe / f'jit.run2.{suffix}'
                content = file.read_text()
                file.unlink()
                self.assertEqual(self.classify()['class'], 'failed')
                file.write_text(content)

    def test_compile_outcomes(self):
        self.write('jit.compile.log', 'line 1\nline 2 error: compile failed\n')
        for rc, expected, compile_state in (('142', 'failed', 'timeout'),
                                            ('2', 'unavailable', 'refused'),
                                            ('', 'unverified', 'not_attempted')):
            with self.subTest(rc=rc):
                self.write('jit.compile.rc', rc)
                result = self.classify()
                self.assertEqual(result['class'], expected)
                self.assertEqual(result['compile'], compile_state)
                if compile_state != 'not_attempted':
                    self.assertIn('compile failed', result['compile_error'])

    def test_tolerance_stability(self):
        for digest, expected in (('abc', 'bit_identical'), ('def', 'within_tolerance'),
                                 ('na', 'tolerance_only')):
            with self.subTest(digest=digest):
                self.write('jit.run2.out', GPU + RESULT.replace('abc', digest))
                result = self.classify()
                self.assertEqual(result['class'], 'supported')
                self.assertEqual(result['stability'], expected)

    def test_overall_resolution_branches(self):
        # Verify resolution rules between AOT and JIT paths
        rules = [
            (('supported', 'supported'), 'supported'),
            (('failed', 'supported'), 'failed'),
            (('supported', 'failed'), 'failed'),
            (('unavailable', 'unavailable'), 'unavailable'),
            (('supported', 'unavailable'), 'partial'),
            (('supported', 'partial'), 'partial'),
            (('unverified', 'supported'), 'partial'),
            (('unverified', 'unavailable'), 'unverified'),
            (('unverified', 'unverified'), 'unverified'),
        ]
        manifest = self.work / 'manifest'
        manifest.mkdir(exist_ok=True)
        (manifest / 'env.txt').write_text('cumetal_version=fixture\n')
        (manifest / 'sha256.txt').write_text('abc  ./classify.py\n')
        out_json = self.work / 'matrix_branches.json'

        for (aot_class, jit_class), expected_overall in rules:
            with self.subTest(aot=aot_class, jit=jit_class):
                # Write probe files matching classes
                if aot_class == 'failed':
                    self.write('aot.run1.rc', '1')
                elif aot_class == 'unavailable':
                    self.write('aot.compile.rc', '2')
                elif aot_class == 'unverified':
                    self.write('aot.compile.rc', '')
                else:
                    self.write('aot.compile.rc', '0')
                    self.write('aot.run1.rc', '0')

                if jit_class == 'failed':
                    self.write('jit.run1.rc', '1')
                elif jit_class == 'unavailable':
                    self.write('jit.compile.rc', '2')
                elif jit_class == 'unverified':
                    self.write('jit.compile.rc', '')
                elif jit_class == 'partial':
                    self.write('jit.compile.rc', '0')
                    self.write('jit.run1.rc', '0')
                    self.write('jit.run2.out', RESULT)
                else:
                    self.write('jit.compile.rc', '0')
                    self.write('jit.run1.rc', '0')
                    self.write('jit.run2.out', GPU + RESULT)

                proc = subprocess.run([sys.executable, '-I', '-B', str(SCRIPT), str(self.work),
                                       str(out_json), '3'], capture_output=True, text=True)
                self.assertEqual(proc.returncode, 0, proc.stderr)
                doc = json.loads(out_json.read_text())
                row = next(r for r in doc['results'] if r['probe'] == 'launch_1d')
                self.assertEqual(row['overall'], expected_overall)

    def test_cli_reports_both_paths_and_manifest(self):
        manifest = self.work / 'manifest'
        manifest.mkdir()
        (manifest / 'env.txt').write_text('cumetal_version=fixture\n')
        (manifest / 'sha256.txt').write_text('abc  ./classify.py\n')
        self.write('jit.run3.out', RESULT)
        missing = self.work / 'missing_probe'
        missing.mkdir()
        (missing / 'missing').touch()
        output = self.work / 'matrix.json'
        proc = subprocess.run([sys.executable, '-I', '-B', str(SCRIPT), str(self.work),
                               str(output), '3'], capture_output=True, text=True)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        doc = json.loads(output.read_text())
        self.assertEqual(doc['manifest']['probe_sha256']['classify.py'], 'abc')
        row = doc['results'][0]
        self.assertEqual(row['aot']['class'], 'supported')
        self.assertEqual(row['jit']['class'], 'partial')
        self.assertEqual(row['overall'], 'partial')
        self.assertEqual(doc['summary'], {'partial': 1, 'unverified': 1})
        self.assertIn('partial', output.with_suffix('.md').read_text())


if __name__ == '__main__':
    unittest.main()
