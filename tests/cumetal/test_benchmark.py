"""CPU timing/control-flow tests. No CuMetal or GPU subprocesses."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / 'scripts/cumetal/benchmark/run_benchmark.py'
SPEC = importlib.util.spec_from_file_location('benchmark_runner', SCRIPT)
bench = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bench)
GPU = ('CUMETAL_PROVENANCE event=kernel_launch device=apple_gpu source=jit '
       'semantic_quality=exact launch_success=true\n')


def timing(raw=(40, 50, 60, 70)):
    return dict(unit='ns', warmup_count=2, sample_count=4, first_dispatch_ns=10,
                samples_ns=list(raw), median_ns=55, p95_ns=70)


def output():
    return (GPU * 7 + 'BENCHMARK_RESULT ' + json.dumps(timing()) + '\n' +
            'MATRIX_RESULT probe=sgemm_handwritten status=PASS digest=abc finite=true\n')


class FakeTools:
    def __init__(self, failure=None):
        self.failure = failure
        self.calls = []

    def __call__(self, command, timeout, env=None):
        command = list(map(str, command))
        self.calls.append(command)
        result = dict(command=command, exit=0, output='', error=None, elapsed_ns=100)
        if '--version' in command:
            result['output'] = 'fake version'
            if self.failure == 'version':
                result['exit'] = 1
        elif 'swiftc' in command:
            assert '-module-cache-path' in command
            if self.failure == 'thermal_prepare':
                result.update(exit=1, output='swift unavailable')
        elif command[0].endswith('/thermal'):
            result['output'] = 'fair\n'
            if self.failure == 'thermal_run':
                result.update(exit=None, error='timeout', output='')
            elif self.failure == 'thermal_invalid':
                result['output'] = 'unknown'
        elif '--benchmark' in command:
            assert env['CUMETAL_TRACE_GPU'] == '1'
            result['output'] = output()
            if self.failure == 'timeout':
                result.update(exit=None, error='timeout')
            elif self.failure == 'sync':
                result.update(exit=20, output='MATRIX_API_FAIL cudaDeviceSynchronize')
            elif self.failure == 'parity':
                result['output'] = result['output'].replace('status=PASS', 'status=MISMATCH')
            elif self.failure == 'provenance':
                result['output'] = result['output'].replace('device=apple_gpu', 'device=cpu')
            elif self.failure == 'missing_dispatch':
                result['output'] = result['output'].replace(GPU, '', 1)
            elif self.failure == 'extra_dispatch':
                result['output'] += GPU
            elif self.failure == 'timing':
                result['output'] = result['output'].replace('"unit": "ns"', '"unit": "ms"')
        elif self.failure == 'compile':
            result.update(exit=1, output='compile error')
        elif self.failure == 'link' and 'clang++' in command:
            result.update(exit=1, output='link error')
        return result


class BenchmarkTests(unittest.TestCase):
    def test_actual_cpp_loop_with_fake_clock_and_synchronized_dispatch(self):
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / 'check.cpp'
            source.write_text(r'''
#define CUMETAL_BENCHMARK_HOST_TEST
#include "sgemm_handwritten.cu"
#include <cassert>
int main() {
  long long tick = 0; int dispatches = 0, validated = 0;
  auto clock = [&]() { return tick; };
  auto dispatch = [&]() { tick += 10 * ++dispatches; return true; };
  auto validate = [&]() { assert(dispatches == ++validated); tick += 100000; return true; };
  benchmark::Samples s;
  assert(benchmark::measure(2, 4, clock, dispatch, validate, s) == benchmark::ok);
  assert(s.first_ns == 10 && s.steady_ns == std::vector<long long>({40,50,60,70}));
  assert(s.median() == 55 && s.p95() == 70 && dispatches == 7);
  benchmark::print(s, 2);
  for (int n : {1, 2, 19, 20, 21, 100}) {
    benchmark::Samples q; for (int i=1; i<=n; ++i) q.steady_ns.push_back(i);
    assert(q.p95() == (95*n+99)/100);
    assert(q.median() == (n+1)/2.0);
  }
  benchmark::Samples fail;
  assert(benchmark::measure(1, 1, clock, [](){return false;}, validate, fail) == benchmark::dispatch_failed);
  assert(benchmark::measure(1, 1, clock, dispatch, [](){return false;}, fail) == benchmark::parity_failed);
  auto backwards = [&]() { return --tick; };
  assert(benchmark::measure(1, 1, backwards, [](){return true;}, validate, fail) == benchmark::clock_failed);
  int value;
  for (const char* text : {"0", "-1", "21", "9999999999999999999999", "1x", "", "+1"})
    assert(!benchmark::count(text, 20, value));
  assert(benchmark::count("20", 20, value) && value == 20);
  assert(benchmark::count("100", 100, value) && value == 100);
  int w=0, n=0;
  char a[]="probe", b[]="--benchmark", c[]="--warmup", d[]="2", e[]="--iterations", f[]="4";
  char* argv[]={a,b,c,d,e,f};
  assert(benchmark::arguments(1, argv, w, n)==0);
  assert(benchmark::arguments(6, argv, w, n)==1 && w==2 && n==4);
  assert(benchmark::arguments(2, argv, w, n)==-1);
  argv[2]=a; assert(benchmark::arguments(6, argv, w, n)==-1); argv[2]=c;
  argv[4]=a; assert(benchmark::arguments(6, argv, w, n)==-1); argv[4]=e;
  argv[1]=a; assert(benchmark::arguments(6, argv, w, n)==-1); argv[1]=b;
  argv[3]=a; assert(benchmark::arguments(6, argv, w, n)==-1); argv[3]=d;
  argv[5]=a; assert(benchmark::arguments(6, argv, w, n)==-1);
}
''')
            compiler = shutil.which('c++')
            self.assertIsNotNone(compiler)
            binary = Path(tmp) / 'check'
            subprocess.run([compiler, '-std=c++17', '-I', str(bench.MATRIX), str(source),
                            '-o', str(binary)], check=True, capture_output=True)
            run = subprocess.run([str(binary)], check=True, capture_output=True, text=True)
            self.assertEqual(bench.samples(run.stdout, 2, 4), timing())

    def test_summary_rejects_invalid_evidence(self):
        cases = [dict(unit='ms'), dict(sample_count=5), dict(warmup_count=3),
                 dict(samples_ns=[1]), dict(samples_ns=[-1, 50, 60, 70]),
                 dict(first_dispatch_ns=float('nan')), dict(median_ns=0), dict(p95_ns=60),
                 dict(first_dispatch_ns=True), dict(samples_ns=[float('inf'), 50, 60, 70])]
        for change in cases:
            with self.subTest(change=change), self.assertRaises(ValueError):
                bench.samples('BENCHMARK_RESULT ' + json.dumps(timing() | change), 2, 4)
        for value in ('', 'BENCHMARK_RESULT {}', output() + output()):
            with self.subTest(value=value), self.assertRaises((ValueError, KeyError)):
                bench.samples(value, 2, 4)

    def test_cli_limits(self):
        for flag, limit in [('--warmup', 20), ('--iterations', 100), ('--timeout', 600),
                            ('--compile-timeout', 600)]:
            for value in ('0', '-1', str(limit + 1), '1.5'):
                with self.subTest(flag=flag, value=value), contextlib.redirect_stderr(io.StringIO()):
                    with self.assertRaises(SystemExit):
                        bench.parser().parse_args(['--output', '/tmp/unused', flag, value])

    def test_runner_modes_and_failure_branches(self):
        for mode in ('aot', 'jit'):
            for failure in (None, 'compile', 'link', 'timeout', 'sync', 'parity', 'provenance',
                            'missing_dispatch', 'extra_dispatch', 'timing', 'version',
                            'thermal_prepare', 'thermal_run', 'thermal_invalid'):
                if mode == 'aot' and failure == 'link':
                    continue
                with self.subTest(mode=mode, failure=failure), tempfile.TemporaryDirectory() as tmp:
                    args = bench.parser().parse_args(['--output', str(Path(tmp)/'result.json'),
                             '--mode', mode, '--warmup', '2', '--iterations', '4'])
                    fake = FakeTools(failure)
                    result = bench.collect(args, Path(tmp), fake)
                    expected = failure is None or failure.startswith('thermal_')
                    self.assertEqual(result['performance_success'], expected)
                    self.assertEqual(result['status'], 'measured' if expected else 'failed')
                    if expected:
                        self.assertEqual(result['compile_link_wall_ns'], 100 if mode == 'aot' else 200)
                        self.assertEqual(result['timing'], timing())
                        state = 'unavailable' if failure else 'fair'
                        self.assertEqual(result['thermal_before']['state'], state)
                        self.assertEqual(result['thermal_after']['state'], state)
                        if failure:
                            self.assertTrue(result['thermal_before']['reason'])

    def test_stage_clock_timeout_and_missing_tool(self):
        ticks = iter((100, 175))
        result = bench.stage([sys.executable, '-c', 'print("cpu")'], 10, clock=lambda: next(ticks))
        self.assertEqual(result['elapsed_ns'], 75)
        self.assertEqual(result['output'], 'cpu\n')
        self.assertTrue(bench.succeeded(result))
        result = bench.stage([sys.executable, '-c', 'import time; time.sleep(10)'], 0.02)
        self.assertEqual(result['error'], 'timeout')
        self.assertIsNone(result['exit'])
        result = bench.stage(['/nonexistent/cumetal-test-tool'], 1)
        self.assertFalse(bench.succeeded(result))
        self.assertTrue(result['error'])

    def test_main_writes_failure_and_success(self):
        for success in (False, True):
            with tempfile.TemporaryDirectory() as tmp:
                target = Path(tmp) / 'result.json'
                with patch.object(sys, 'argv', [str(SCRIPT), '--output', str(target)]), \
                     patch.object(bench, 'collect', return_value={'performance_success': success}):
                    self.assertEqual(bench.main(), 0 if success else 1)
                self.assertEqual(json.loads(target.read_text()), {'performance_success': success})
