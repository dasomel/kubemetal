"""Host probe fixtures exercise real macOS formats without executing GPU work."""
import importlib.util
import json
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('host_manifest', ROOT / 'scripts/cumetal/matrix/host_manifest.py')
host = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(host)


def probe(stdout='', rc=0, error=None):
    return {'argv': ['fixture'], 'stdout': stdout, 'stderr': '', 'returncode': rc, 'error': error}


def fixtures():
    return {
        'mac_model': probe('Mac16,7\n'), 'soc': probe('Apple M4 Pro\n'),
        'ram_bytes': probe('51539607552\n'),
        'gpu_cores': probe(json.dumps({'SPDisplaysDataType': [{'_name': 'Apple M4 Pro', 'sppci_cores': '20'}]})),
        'macos': probe('ProductName:\t\tmacOS\nProductVersion:\t\t27.0.1\nBuildVersion:\t\t26A434\n'),
        'xcode_version': probe('Xcode 26.0\nBuild version 17A324\n'),
        'cumetal_version': probe('CuMetal 0.7.1\n'),
        'llvm_clang': probe('Homebrew clang version 23.1.5\nTarget: arm64-apple-darwin\n'),
        'xcrun_clang': probe('Apple clang version 17.0.0 (clang-1700.0.13.3)\n'),
        'metal_version': probe('Apple metal version 32023.404 (metalfe-32023.404)\n'),
    }


class HostManifestTests(unittest.TestCase):
    def test_real_formats_and_unicode(self):
        raw = fixtures()
        raw['soc']['stdout'] = 'Apple M4 Pro — 테스트\n'
        result = host.interpret(raw)
        self.assertEqual(result['mac_model'], 'Mac16,7')
        self.assertEqual(result['soc'], 'Apple M4 Pro — 테스트')
        self.assertEqual(result['ram_bytes'], 51539607552)
        self.assertEqual(result['gpu_cores'], 20)
        self.assertEqual(result['product_version'], '27.0.1')
        self.assertEqual(result['build'], '26A434')
        self.assertEqual(result['macos'], raw['macos']['stdout'].strip())
        self.assertEqual(result['xcode_version'], 'Xcode 26.0\nBuild version 17A324')
        self.assertFalse(result['errors'])
        self.assertEqual(result['probes'], raw)

    def test_display_serials_and_uuids_are_excluded(self):
        raw = fixtures()
        raw['gpu_cores']['stdout'] = json.dumps({'SPDisplaysDataType': [{
            'sppci_cores': '20', 'serial_number': 'PRIVATE_SERIAL',
            'nested': {'display_UUID': 'PRIVATE_UUID'}}]})
        responses = iter(raw.values())
        result = host.collect('/tool', '/clang', runner=lambda argv: next(responses))
        self.assertEqual(result['gpu_cores'], 20)
        self.assertNotIn('PRIVATE_', json.dumps(result))

    def test_clt_is_never_xcode(self):
        for output, rc in (('Apple clang version 17.0.0', 0),
                           ('xcode-select: error: tool requires Xcode, active developer directory is CommandLineTools', 1)):
            raw = fixtures()
            raw['xcode_version'] = probe(output, rc)
            result = host.interpret(raw)
            self.assertIsNone(result['xcode_version'])
            self.assertIn('xcode_version', result['errors'])
            self.assertEqual(result['probes']['xcode_version']['stdout'], output)
            self.assertIsNotNone(result['xcrun_clang'])

    def test_missing_timeout_failed_and_empty_fields(self):
        for replacement in (None, probe('', None, 'timeout after 5s'),
                            probe('error text', 1), probe(''), probe('   ')):
            for field in fixtures():
                with self.subTest(field=field, replacement=replacement):
                    raw = fixtures()
                    if replacement is None:
                        del raw[field]
                    else:
                        raw[field] = replacement
                    result = host.interpret(raw)
                    self.assertIsNone(result[field])
                    self.assertTrue(result['errors'][field])

    def test_malformed_gpu_and_ram(self):
        for value in ('0', '-1', 'twenty', '20 cores', '1.5', True, None, 0):
            raw = fixtures()
            raw['gpu_cores'] = probe(json.dumps({'SPDisplaysDataType': [{'sppci_cores': value}]}))
            result = host.interpret(raw)
            self.assertIsNone(result['gpu_cores'], value)
            self.assertIn('gpu_cores', result['errors'])
        for value in ('0', '-1', '48 GB', '1.5', 'unknown'):
            raw = fixtures()
            raw['ram_bytes'] = probe(value)
            self.assertIsNone(host.interpret(raw)['ram_bytes'])
        for value in ('{}', 'not json', '{"SPDisplaysDataType": []}',
                      '{"SPDisplaysDataType": [{}, {}]}', '{"SPDisplaysDataType": [null]}', '{"SPDisplaysDataType": [{}]}'):
            raw = fixtures()
            raw['gpu_cores'] = probe(value)
            self.assertIsNone(host.interpret(raw)['gpu_cores'])

    def test_macos_requires_version_and_build_and_numeric_gpu_is_valid(self):
        for output in ('ProductName: macOS', 'ProductVersion: 27.0.1',
                       'ProductVersion: \nBuildVersion: 26A434',
                       'ProductVersion: 27.0.1\nBuildVersion: '):
            raw = fixtures()
            raw['macos'] = probe(output)
            self.assertIsNone(host.interpret(raw)['macos'])
        raw = fixtures()
        raw['gpu_cores'] = probe('{"SPDisplaysDataType": [{"sppci_cores": 20}]}')
        self.assertEqual(host.interpret(raw)['gpu_cores'], 20)

    def test_bounded_runner_keeps_raw_status(self):
        with patch.object(host.subprocess, 'run', return_value=subprocess.CompletedProcess(['fake'], 2, '원문', 'diagnostic')) as run:
            result = host.run_probe(['fake'], timeout=0.1)
            self.assertEqual(result['returncode'], 2)
            self.assertEqual(result['stdout'], '원문')
            self.assertEqual(result['stderr'], 'diagnostic')
            self.assertTrue(result['error'])
            self.assertEqual(run.call_args.kwargs['timeout'], 0.1)
            self.assertEqual(run.call_args.kwargs['env'],
                             {'PATH': '/usr/bin:/bin:/usr/sbin:/sbin', 'LC_ALL': 'C'})
            self.assertFalse(run.call_args.kwargs.get('shell', False))
        with patch.object(host.subprocess, 'run', side_effect=FileNotFoundError('missing')):
            result = host.run_probe(['missing'])
            self.assertIsNone(result['returncode'])
            self.assertIn('missing', result['error'])
        for stdout, stderr, expected in ((b'partial', b'err', 'partial'),
                                          ('부분', 'err', '부분'), (None, None, '')):
            with patch.object(host.subprocess, 'run', side_effect=subprocess.TimeoutExpired(['slow'], 0.1, stdout, stderr)):
                result = host.run_probe(['slow'], timeout=0.1)
                self.assertIsNone(result['returncode'])
                self.assertEqual(result['stdout'], expected)
                self.assertEqual(result['stderr'], 'err' if stderr else '')
                self.assertIn('timeout', result['error'])

    def test_collection_commands_are_read_only_and_personal_data_excluded(self):
        calls = []
        raw = fixtures()
        def runner(argv):
            calls.append(argv)
            return probe('observed')
        result = host.collect('/fake/cumetal', '/fake/clang', runner=runner)
        self.assertEqual(len(calls), len(raw))
        self.assertIn(['/usr/sbin/sysctl', '-n', 'hw.model'], calls)
        self.assertIn(['/usr/sbin/sysctl', '-n', 'hw.memsize'], calls)
        self.assertIn(['/usr/bin/xcodebuild', '-version'], calls)
        self.assertIn(['/usr/sbin/system_profiler', 'SPDisplaysDataType', '-detailLevel', 'mini', '-json'], calls)
        self.assertFalse(any('SPHardwareDataType' in arg or 'UUID' in arg or 'serial' in arg for call in calls for arg in call))
        self.assertEqual(result['soc'], 'observed')

    def test_classifier_links_raw_manifest_and_legacy_brand(self):
        import test_matrix_classifier as classifier_fixture
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'raw'
            (root / 'manifest').mkdir(parents=True)
            observed = host.interpret(fixtures())
            (root / 'manifest/host.json').write_text(json.dumps(observed))
            (root / 'manifest/env.txt').write_text('hw_model=old brand\n')
            target = Path(directory) / 'result.json'
            classifier_fixture.classifier.evidence.seal(root, 3, True)
            classifier_fixture.classifier.main(str(root), str(target), 3, initial=True)
            result = json.loads(target.read_text())
            self.assertEqual(result['manifest']['host'], observed)
            self.assertEqual(result['manifest']['hw_model'], observed['soc'])
            self.assertIn('legacy SoC brand', result['manifest']['hw_model_meaning'])
            self.assertEqual(result['manifest']['cumetal_version'], observed['cumetal_version'])
            self.assertNotEqual(result['manifest']['hw_model'], observed['mac_model'])

    def test_cli_preserves_unicode_and_refuses_collision(self):
        # Execute the unchanged entrypoint; replace only the external process boundary.
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'host.json'
            command = [str(SPEC.origin), str(output), '--cumetal', '/missing', '--clang', '/missing']
            completed = subprocess.CompletedProcess([], 0, '테스트\n', '')
            with patch.object(sys, 'argv', command), patch.object(subprocess, 'run', return_value=completed):
                runpy.run_path(str(SPEC.origin), run_name='__main__')
            self.assertIn('테스트', output.read_text())
            original = output.read_bytes()
            with patch.object(sys, 'argv', command), self.assertRaises(FileExistsError):
                runpy.run_path(str(SPEC.origin), run_name='__main__')
            self.assertEqual(output.read_bytes(), original)


if __name__ == '__main__':
    unittest.main()
