"""Synthetic on-disk evidence; no CuMetal, GPU or subprocess mocks required."""
import importlib.util
import json
import shutil
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

    def test_cusparse_runner_registration(self):
        runner = (SCRIPT.parent / 'run_matrix.sh').read_text()
        self.assertIn('cusparse_spmv:cusparse', runner.split('ALL_PROBES=(', 1)[1].split(')', 1)[0])
        self.assertTrue((SCRIPT.parent / 'cusparse_spmv.cu').is_file())

    def test_cusparse_library_cli_fixtures(self):
        self.probe = self.probe.rename(self.work / 'cusparse_spmv')
        self.write('lib', 'cusparse')
        result = RESULT.replace('launch_1d', 'cusparse_spmv')
        cases = (
            (0, GPU + result, 'supported', 'bit_identical', ''),
            (0, result, 'partial', 'bit_identical', 'library call: numeric parity only'),
            (21, 'MATRIX_API_FAIL cusparseSpMV -> NOT_SUPPORTED', 'unavailable', None, 'API failure'),
            (21, 'MATRIX_API_FAIL cusparseCreate -> allocation error', 'failed', None, 'API failure'),
            (1, GPU + result.replace('PASS', 'MISMATCH'), 'failed', None, 'numeric mismatch'),
            (1, GPU + result.replace('PASS', 'MISMATCH') + 'finite=false value=nan\n',
             'failed', None, 'numeric mismatch'),
            (0, GPU + result.replace('abc', 'def'), 'supported', 'within_tolerance', 'output bits differed'),
            (142, '', 'failed', None, 'run timed out'),
        )
        for path in ('aot', 'jit'):
            for rc, output, expected, stability, note in cases:
                with self.subTest(path=path, rc=rc, output=output):
                    for mode in ('aot', 'jit'):
                        for run in range(1, 4):
                            self.write(f'{mode}.run{run}.rc', '0')
                            self.write(f'{mode}.run{run}.out', GPU + result)
                    self.write(f'{path}.run2.rc', str(rc))
                    self.write(f'{path}.run2.out', output)
                    target = self.work / 'matrix.json'
                    proc = subprocess.run([sys.executable, '-I', '-B', str(SCRIPT), str(self.work),
                                           str(target), '3'], capture_output=True, text=True)
                    self.assertEqual(proc.returncode, 0, proc.stderr)
                    row = json.loads(target.read_text())['results'][0]
                    self.assertEqual(row['lib'], 'cusparse')
                    self.assertEqual(row[path]['class'], expected)
                    self.assertIn(note, row['note'])
                    self.assertEqual(row['overall'], 'partial' if expected == 'unavailable' else expected)
                    if stability:
                        self.assertEqual(row[path]['stability'], stability)

    def test_cusparse_probe_cpu_fault_injection(self):
        # Host-only fake APIs exercise the actual probe, never CuMetal or a GPU.
        compiler = shutil.which('c++')
        if not compiler:
            self.skipTest('host C++ compiler unavailable')
        for name in ('common.h', 'cusparse_spmv.cu'):
            shutil.copyfile(SCRIPT.parent / name, self.work / name)
        (self.work / 'cuda_runtime.h').write_text('''#pragma once
#include <cstdlib>
#include <cstring>
typedef int cudaError_t;
const int cudaSuccess=0, cudaMemcpyHostToDevice=1, cudaMemcpyDeviceToHost=2;
inline const char* cudaGetErrorString(int) { return "injected CUDA error"; }
template<class T> int cudaMalloc(T** p, size_t n) { *p=(T*)malloc(n); return 0; }
inline int cudaFree(void* p) { free(p); return 0; }
inline int cudaMemcpy(void* d, const void* s, size_t n, int) { memcpy(d,s,n); return 0; }
inline int cudaDeviceSynchronize() { return getenv("SYNC_FAIL") ? 1 : 0; }
''')
        (self.work / 'cusparse.h').write_text('''#pragma once
typedef int cusparseStatus_t, cusparseHandle_t, cusparseSpMatDescr_t, cusparseDnVecDescr_t;
const int CUSPARSE_STATUS_SUCCESS=0, CUSPARSE_INDEX_32I=0, CUSPARSE_INDEX_BASE_ZERO=0;
const int CUDA_R_32F=0, CUSPARSE_OPERATION_NON_TRANSPOSE=0, CUSPARSE_SPMV_ALG_DEFAULT=0;
inline const char* cusparseGetErrorString(int) { return "injected API error"; }
#define SIMPLE(name) template<class... T> int name(T...) { return getenv("API_FAIL") ? 3 : 0; }
SIMPLE(cusparseCreate) SIMPLE(cusparseCreateCsr) SIMPLE(cusparseDestroyDnVec)
SIMPLE(cusparseDestroySpMat) SIMPLE(cusparseDestroy)
static float* output;
inline int cusparseCreateDnVec(int*, int, float* p, int) { output=p; return 0; }
inline int cusparseSpMV_bufferSize(int,int,float*,int,int,float*,int,int,int,size_t* n) {
  *n=getenv("WORKSPACE") ? 64 : 0; return 0;
}
inline int cusparseSpMV(int,int,float*,int,int,float*,int,int,int,void*) {
  float expected[]={-4.25f,1.0f,-1.34375f,-18.25f};
  memcpy(output,expected,sizeof expected);
  const char* value=getenv("OUTPUT"); if(value) output[2]=strtof(value,nullptr);
  return 0;
}
''')
        binary = self.work / 'probe'
        build = subprocess.run([compiler, '-std=c++17', '-x', 'c++', '-I', str(self.work),
                                str(self.work / 'cusparse_spmv.cu'), '-o', str(binary)],
                               capture_output=True, text=True)
        self.assertEqual(build.returncode, 0, build.stderr)
        for env, rc, marker in (({}, 0, 'status=PASS'), ({'WORKSPACE': '1'}, 0, 'status=PASS'),
                                ({'OUTPUT': '-1.34374'}, 0, 'status=PASS'),
                                ({'OUTPUT': '0'}, 1, 'status=MISMATCH'),
                                ({'OUTPUT': 'nan'}, 1, 'finite=false'),
                                ({'OUTPUT': 'inf'}, 1, 'finite=false'),
                                ({'API_FAIL': '1'}, 21, 'MATRIX_API_FAIL'),
                                ({'SYNC_FAIL': '1'}, 20, 'MATRIX_API_FAIL')):
            with self.subTest(env=env):
                proc = subprocess.run([str(binary)], env=env, capture_output=True, text=True)
                self.assertEqual(proc.returncode, rc, proc.stderr)
                self.assertIn(marker, proc.stdout + proc.stderr)
                if rc in (0, 1):
                    self.assertRegex(proc.stdout, r'digest=[0-9a-f]{16}')


if __name__ == '__main__':
    unittest.main()
