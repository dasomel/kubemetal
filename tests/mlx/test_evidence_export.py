"""Existing collector-format fixtures, not new GPU measurements."""
import copy
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts/mlx'))
import evidence_export as evidence
import storage_benchmark


class EvidenceTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        # Reuse the existing storage report constructor, including its errors,
        # null timings, environment and buffered-I/O limitations; do not measure.
        self.payloads = {
            'gpu': {'gflops': 12.5, 'elapsed_seconds': 2, 'matrix_dim': 2048, 'iterations': 20},
            'storage': storage_benchmark.new_report(),
            'hardware': {'brand_name': 'fixture Mac', 'cpu_cores': 8,
                         'total_memory_gb': 16, 'gpu_cores': None},
            'metrics': {'total_memory_gb': 16, 'used_memory_gb': 4,
                        'cpu_usage_percentage': 0, 'memory_usage_percentage': 25,
                        'thermal_state': None, 'gpu_backend': 'apple_metal'},
        }
        self.manifest = {
            'observed_at': 'unknown', 'run_id': 'fixture-run', 'workload_id': 'fixture-matmul',
            'device': {'name': 'fixture Mac', 'backend': 'apple_metal'},
            'limitations': ['GPU UUID and power unavailable; no collector run by exporter.'],
            'sources': [],
        }
        for kind, payload in self.payloads.items():
            (self.base / f'{kind}.json').write_text(json.dumps(payload) + '\n')
            self.manifest['sources'].append({
                'kind': kind, 'path': f'{kind}.json',
                'collector': {'name': kind, 'version': None},
                'observed_at': 'unknown', 'measurement_interval': {'start': 'unknown', 'end': 'unknown'},
            })
        self.manifest_path = self.base / 'manifest.json'
        self.manifest_path.write_text(json.dumps(self.manifest))
        self.envelope = evidence.export(self.manifest, self.base)

    def cli(self, *args):
        return subprocess.run([sys.executable, '-B', str(ROOT / 'scripts/mlx/evidence_export.py'),
                               *map(str, args)], capture_output=True, text=True)

    def test_saved_schema_validated_roundtrip_in_separate_process(self):
        output = self.base / 'envelope.json'
        run = self.cli('export', self.manifest_path, '--output', output)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(self.cli('verify', output).returncode, 0)
        saved = evidence.loads(output.read_text())
        evidence.schema_check(saved, evidence.loads(evidence.SCHEMA.read_text()))
        run = self.cli('verify', output, '--manifest', self.manifest_path)
        self.assertEqual(run.returncode, 0, run.stderr)
        for source in saved['sources']:
            self.assertEqual(source['raw_json'].encode(), (self.base / f"{source['kind']}.json").read_bytes())
            self.assertEqual(evidence.loads(source['raw_json']), self.payloads[source['kind']])
        self.assertEqual(self.cli('export', self.manifest_path, '--output', output).returncode, 1)
        (self.base / 'gpu.json').write_text(json.dumps({**self.payloads['gpu'], 'gflops': 99}))
        tampered = self.cli('verify', output, '--manifest', self.manifest_path)
        self.assertEqual(tampered.returncode, 1)
        self.assertIn('differs from original', tampered.stderr)

    def test_null_unknown_errors_and_limitations_preserved(self):
        gpu, storage, hardware, metrics = self.envelope['sources']
        self.assertEqual(storage['limitations'], storage_benchmark.LIMITATIONS)
        self.assertEqual(storage['observations']['read.bytes']['status'], 'unavailable')
        self.assertIsNone(storage['observations']['read.bytes']['value'])
        partial = {'read': {'bytes': 3, 'elapsed_seconds': None}}
        self.assertEqual(evidence.observations('storage', partial)['read.bytes']['value'], 3)
        zero = {'read': {'bytes': 0, 'elapsed_seconds': 1}}
        self.assertEqual(evidence.observations('storage', zero)['read.bytes']['value'], 0)
        self.assertEqual(evidence.loads(storage['raw_json'])['status'], 'error')
        self.assertEqual(storage['observations']['read.elapsed_seconds']['status'], 'unavailable')
        self.assertIsNone(hardware['observations']['gpu_cores']['value'])
        self.assertEqual(metrics['observations']['gpu_usage_percentage']['status'], 'unavailable')
        self.assertEqual(metrics['observations']['cpu_usage_percentage']['value'], 0)
        self.assertEqual(gpu['observed_at'], 'unknown')
        self.assertIsNone(gpu['collector']['version'])
        self.assertNotIn('uuid', self.envelope['device'])

    def test_missing_identifiers_units_hash_and_mutation_rejected(self):
        for field in ('run_id', 'workload_id', 'device', 'sources', 'observed_at'):
            bad = copy.deepcopy(self.envelope)
            del bad[field]
            with self.subTest(field=field), self.assertRaises(ValueError):
                evidence.validate(bad)
        for field in ('sha256', 'raw_json', 'observations', 'limitations', 'collector'):
            bad = copy.deepcopy(self.envelope)
            bad['sources'][0][field] = {'sha256': 'x', 'raw_json': '{}', 'observations': {},
                                        'limitations': ['changed'], 'collector': {}}[field]
            with self.subTest(field=field), self.assertRaises(ValueError):
                evidence.validate(bad)
        for unit in (None, '', 'W'):
            bad = copy.deepcopy(self.envelope)
            bad['sources'][0]['observations']['gflops']['unit'] = unit
            with self.subTest(unit=unit), self.assertRaises(ValueError):
                evidence.validate(bad)
        bad = copy.deepcopy(self.envelope)
        del bad['sources'][0]['observations']['gflops']['unit']
        with self.assertRaises(ValueError):
            evidence.validate(bad)

    def test_invalid_json_and_measurements(self):
        for raw in ('{"x": NaN}', '{"x": Infinity}', '{"x": 1e999}', '{"x":1,"x":2}', 'bad'):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                evidence.loads(raw)
        for value in (True, -1, '12', float('nan'), float('inf')):
            with self.subTest(value=value), self.assertRaises(ValueError):
                evidence.observations('gpu', {'gflops': value})
        for payload in ({'thermal_state': 'hot'},):
            with self.assertRaises(ValueError):
                evidence.observations('metrics', payload)

    def test_times_identity_and_schema_types(self):
        for start, end in [('yesterday', 'unknown'), ('2026-11-13', 'unknown'),
                           ('2026-11-13T01:00:00Z', 'unknown'),
                           ('2026-11-13T02:00:00Z', '2026-11-13T01:00:00Z')]:
            bad = copy.deepcopy(self.envelope)
            bad['sources'][0]['measurement_interval'] = {'start': start, 'end': end}
            with self.subTest(start=start), self.assertRaises(ValueError):
                evidence.validate(bad)
        good = copy.deepcopy(self.envelope)
        good['sources'][0]['measurement_interval'] = {'start': '2026-11-13T01:00:00Z', 'end': '2026-11-13T02:00:00Z'}
        evidence.validate(good)
        for field, value in [('run_id', ''), ('sources', []), ('sources', None),
                             ('schema_version', 'narwhal'), ('extra', 1)]:
            bad = copy.deepcopy(self.envelope)
            bad[field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                evidence.validate(bad)
        good['observed_at'] = '2026-11-13T01:00:00+09:00'
        evidence.validate(good)
        for field in ('name', 'backend'):
            bad = copy.deepcopy(self.envelope)
            bad['device'][field] = 'different'
            with self.assertRaises(ValueError):
                evidence.validate(bad)

    def test_source_shape_hash_and_incomplete_gpu(self):
        import hashlib
        for raw in ('[]', '{}', '{"gflops":null}', '{"gflops":NaN}'):
            bad = copy.deepcopy(self.envelope)
            bad['sources'][0]['raw_json'] = raw
            bad['sources'][0]['sha256'] = hashlib.sha256(raw.encode()).hexdigest()
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                evidence.validate(bad)
        bad = copy.deepcopy(self.envelope)
        bad['sources'][0]['sha256'] = '0' * 64
        with self.assertRaisesRegex(ValueError, 'hash mismatch'):
            evidence.validate(bad)
        (self.base / 'gpu.json').write_text('[]')
        self.assertEqual(self.cli('export', self.manifest_path, '--output', self.base / 'out').returncode, 1)
        for manifest in ([], {}, {'sources': None}, {'sources': [None]}):
            with self.subTest(manifest=manifest), self.assertRaises(ValueError):
                evidence.export(manifest, self.base)

    def test_cli_errors_and_help(self):
        self.assertEqual(self.cli('--help').returncode, 0)
        self.assertEqual(self.cli('export', self.manifest_path).returncode, 1)
        self.assertEqual(self.cli('verify', self.base / 'missing').returncode, 1)
        bad = self.base / 'bad.json'
        bad.write_text('[]')
        self.assertEqual(self.cli('verify', bad).returncode, 1)
        bad.write_text('{}')
        self.assertEqual(self.cli('export', bad, '--output', self.base / 'out').returncode, 1)


if __name__ == '__main__':
    unittest.main()
