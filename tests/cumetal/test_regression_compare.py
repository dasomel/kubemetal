"""CPU-only retained evidence fixtures; no CuMetal executables or GPU work."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / 'scripts/cumetal/regression/compare.py'
SPEC = importlib.util.spec_from_file_location('regression_compare', SCRIPT)
compare = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(compare)


class RegressionTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def fixture(self, name, aot='supported', jit='supported', host=None,
                source='// fixture', repeats=1, env='', probes=('add',), version='v1'):
        root = self.root / name
        manifest = root / 'manifest'
        sources = manifest / 'sources'
        sources.mkdir(parents=True)
        identity = dict.fromkeys(compare.HOST_FIELDS + compare.VERSIONS, 'fixture')
        identity.update(schema='cumetal-host/1', errors={}, cumetal_version=version)
        identity.update(host or {})
        (manifest / 'host.json').write_text(json.dumps(identity))
        (manifest / 'env.txt').write_text('prefix=/fixture\ndoctor_rc=0\ndate_utc=' + name + '\n' + env)
        for probe in probes:
            (sources / f'{probe}.cu').write_text(source)
            directory = root / probe
            directory.mkdir()
            (directory / 'lib').write_text('')
            for mode, state in (('aot', aot), ('jit', jit)):
                if state == 'unverified':
                    continue
                (directory / f'{mode}.compile.rc').write_text('2' if state == 'unavailable' else '0')
                (directory / f'{mode}.compile.log').write_text('unsupported API' if state == 'unavailable' else '')
                if state == 'unavailable':
                    continue
                for i in range(1, repeats + 1):
                    status = 'MISMATCH' if state == 'failed' else 'PASS'
                    output = f'MATRIX_RESULT status={status} digest=abc max_error=0.125 finite=true\n'
                    if state != 'partial':
                        output += ('CUMETAL_PROVENANCE event=kernel_launch device=apple_gpu '
                                   'source=jit semantic_quality=exact launch_success=true\n')
                    (directory / f'{mode}.run{i}.rc').write_text('1' if state == 'failed' else '0')
                    (directory / f'{mode}.run{i}.out').write_text(output)
        (manifest / 'sha256.txt').write_text(''.join(
            f'{compare.evidence.file_sha256(path)}  ./{path.name}\n' for path in sorted(sources.iterdir())))
        compare.evidence.seal(root, repeats, True)
        result = self.root / f'{name}.json'
        compare.classify.main(str(root), str(result), repeats, initial=True)
        return result, root

    def invoke(self, left, right, name='report'):
        target = self.root / f'{name}.json'
        proc = subprocess.run([sys.executable, '-I', '-B', str(SCRIPT), str(left), str(right), str(target)],
                              env={'PATH': '/nonexistent'}, capture_output=True, text=True)
        return proc, target

    def test_same_results_and_version_upgrade(self):
        left, _ = self.fixture('old')
        right, _ = self.fixture('new', version='v2')
        proc, target = self.invoke(left, right)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        doc = json.loads(target.read_text())
        self.assertEqual(doc['verdict'], 'no_regression')
        self.assertEqual(doc['summary'], {'unchanged': 2})
        self.assertEqual(doc['versions']['cumetal_version'], {'before': 'v1', 'after': 'v2'})
        self.assertIn('no_regression', target.with_suffix('.md').read_text())

    def test_markdown_failure_leaves_no_partial_report_and_can_retry(self):
        left, _ = self.fixture('old')
        right, _ = self.fixture('new')
        target = self.root / 'retry.json'
        with mock.patch.object(compare, 'compare', return_value={'verdict': 'no_regression'}), \
                mock.patch.object(compare, 'markdown', side_effect=OSError('injected markdown failure')):
            with mock.patch.object(sys, 'argv', ['compare.py', str(left), str(right), str(target)]):
                self.assertEqual(compare.main(), 2)
        self.assertFalse(target.exists())
        self.assertFalse(target.with_suffix('.md').exists())
        with mock.patch.object(compare, 'compare', return_value={'verdict': 'no_regression'}), \
                mock.patch.object(compare, 'markdown', return_value='# retry\n'):
            with mock.patch.object(sys, 'argv', ['compare.py', str(left), str(right), str(target)]):
                self.assertEqual(compare.main(), 0)
        self.assertTrue(target.exists())
        self.assertTrue(target.with_suffix('.md').exists())

    def test_supported_to_failed_or_unavailable_and_improvement(self):
        left, _ = self.fixture('old')
        for state in ('failed', 'unavailable'):
            with self.subTest(state=state):
                right, _ = self.fixture(state, aot=state)
                proc, target = self.invoke(left, right, state + '-report')
                self.assertEqual(proc.returncode, 1, proc.stderr)
                doc = json.loads(target.read_text())
                self.assertEqual(doc['summary'], {'regression': 1, 'unchanged': 1})
                text = target.with_suffix('.md').read_text()
                if state == 'failed':
                    self.assertIn('numeric mismatch', text)
                    self.assertIn('max_error=0.125', text)
                    self.assertIn('MISMATCH', doc['results'][0]['candidate_evidence']['parity'])
                else:
                    self.assertIn('unsupported API', text)
                reverse = compare.compare(right, left)
                self.assertEqual(reverse['summary'], {'improvement': 1, 'unchanged': 1})

    def test_partial_is_not_a_clean_supported_baseline(self):
        left, _ = self.fixture('old')
        right, _ = self.fixture('new', jit='partial')
        proc, target = self.invoke(left, right)
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertEqual(json.loads(target.read_text())['verdict'], 'incomparable')

    def test_unverified_never_reports_clean_comparison(self):
        left, _ = self.fixture('old', aot='unverified')
        right, _ = self.fixture('new')
        proc, target = self.invoke(left, right)
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertEqual(json.loads(target.read_text())['verdict'], 'incomparable')

    def test_other_changes_and_unchanged_failure_preserve_diagnostics(self):
        left, _ = self.fixture('old', aot='failed', jit='unavailable')
        right, _ = self.fixture('new', aot='failed', jit='partial')
        doc = compare.compare(left, right)
        self.assertEqual(doc['summary'], {'unchanged': 1, 'changed': 1})
        self.assertIn('numeric mismatch', compare.markdown(doc))

    def test_missing_duplicate_empty_and_edited_rows_rejected(self):
        left, _ = self.fixture('old')
        right, _ = self.fixture('new')
        original = json.loads(right.read_text())
        for case in ('missing', 'duplicate', 'empty', 'edited'):
            with self.subTest(case=case):
                doc = json.loads(json.dumps(original))
                if case in ('missing', 'empty'):
                    doc['results'] = []
                elif case == 'duplicate':
                    doc['results'].append(doc['results'][0])
                else:
                    doc['results'][0]['aot']['class'] = 'failed'
                right.write_text(json.dumps(doc))
                proc, target = self.invoke(left, right)
                self.assertEqual(proc.returncode, 2)
                self.assertFalse(target.exists())
        right.write_text(json.dumps(original))
        different, _ = self.fixture('different', probes=('other',))
        with self.assertRaisesRegex(ValueError, 'probe set'):
            compare.compare(left, different)

    def test_digest_tampering_and_missing_evidence(self):
        left, _ = self.fixture('old')
        for case in ('log', 'reseal', 'index', 'missing', 'partial'):
            with self.subTest(case=case):
                right, root = self.fixture(case)
                index = root / compare.evidence.INDEX
                if case in ('log', 'reseal'):
                    (root / 'add/aot.run1.out').write_text('tampered')
                    if case == 'reseal':
                        index.unlink()
                        compare.evidence.seal(root, 1, True)
                elif case == 'missing':
                    index.unlink()
                else:
                    doc = json.loads(index.read_text())
                    if case == 'partial':
                        doc['complete'] = False
                    else:
                        doc['files']['add/aot.run1.out'] = '0' * 64
                    index.write_text(json.dumps(doc))
                proc, target = self.invoke(left, right)
                self.assertEqual(proc.returncode, 2)
                self.assertFalse(target.exists())

    def test_host_source_and_conditions_must_match(self):
        left, _ = self.fixture('old')
        cases = ({'host': {'soc': 'different'}}, {'source': '// changed'},
                 {'repeats': 2}, {'env': 'timeout=2\n'},
                 {'host': {'soc': None}}, {'host': {'schema': 'unknown'}},
                 {'env': 'doctor_rc=1\n'}, {'env': 'prefix=\n'}, {'host': {'errors': {'gpu_cores': 'missing'}}})
        for i, options in enumerate(cases):
            with self.subTest(options=options):
                right, _ = self.fixture(f'new{i}', **options)
                proc, target = self.invoke(left, right)
                self.assertEqual(proc.returncode, 2, proc.stderr)
                self.assertFalse(target.exists())

    def test_resealed_invalid_sources_states_and_library_conditions(self):
        left, _ = self.fixture('old')
        for case in ('source_digest', 'source_missing', 'state_missing', 'library'):
            with self.subTest(case=case):
                right, root = self.fixture(case)
                if case == 'source_digest':
                    (root / 'manifest/sha256.txt').write_text('0' * 64 + '  ./add.cu\n')
                elif case == 'source_missing':
                    (root / 'manifest/sources/add.cu').unlink()
                    (root / 'manifest/sha256.txt').write_text('')
                elif case == 'state_missing':
                    (root / 'add/missing').write_text('missing')
                else:
                    (root / 'add/lib').write_text('different')
                (root / compare.evidence.INDEX).unlink()
                compare.evidence.seal(root, 1, True)
                right.unlink()
                right.with_suffix('.md').unlink()
                compare.classify.main(str(root), str(right), 1, initial=True)
                proc, target = self.invoke(left, right)
                self.assertEqual(proc.returncode, 2, proc.stderr)
                self.assertFalse(target.exists())

    def test_regression_takes_priority_over_incomparable(self):
        left, _ = self.fixture('old')
        right, _ = self.fixture('new', aot='failed', jit='unverified')
        doc = compare.compare(left, right)
        self.assertEqual(doc['verdict'], 'regression')
        self.assertEqual(doc['summary'], {'regression': 1, 'incomparable': 1})

    def test_invalid_schema_repeats_index_and_destination(self):
        left, _ = self.fixture('old')
        right, _ = self.fixture('new')
        original = json.loads(right.read_text())
        for field, value in (('schema', 'unknown'), ('repeats', 0), ('repeats', True),
                             ('evidence', {'index': 'other'}), ('manifest', [])):
            with self.subTest(field=field, value=value):
                doc = {**original, field: value}
                right.write_text(json.dumps(doc))
                proc, target = self.invoke(left, right)
                self.assertEqual(proc.returncode, 2)
                self.assertFalse(target.exists())
        right.write_text(json.dumps(original))
        for suffix in ('.json', '.md'):
            target = self.root / ('report' + suffix)
            target.write_text('keep')
            proc, _ = self.invoke(left, right)
            self.assertEqual(proc.returncode, 2)
            self.assertEqual(target.read_text(), 'keep')
            target.unlink()
        proc = subprocess.run([sys.executable, '-B', str(SCRIPT), str(left), str(right),
                               str(self.root / 'bad.txt')], capture_output=True, text=True)
        self.assertEqual(proc.returncode, 2)
        self.assertIn('output must end in .json', proc.stderr)
        self.assertIn('&#124;', compare.markdown({'verdict': 'incomparable', 'results': [
            {'probe': 'a|b', 'mode': 'jit', 'before': 'failed', 'after': 'failed',
             'change': 'unchanged', 'candidate_evidence': {'error': 'a\nb'}}]}))


if __name__ == '__main__':
    unittest.main()
