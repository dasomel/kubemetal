#!/usr/bin/env python3
"""Opt-in SGEMM timing. No D40 reservation: run only on an idle Mac."""
import argparse
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import signal
import statistics
import subprocess
import tempfile
import time

HERE = Path(__file__).resolve().parent
MATRIX = HERE.parent / 'matrix'
SPEC = importlib.util.spec_from_file_location('matrix_classifier', MATRIX / 'classify.py')
CLASSIFIER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CLASSIFIER)


def bounded(limit):
    def parse(value):
        try:
            n = int(value)
        except ValueError:
            raise argparse.ArgumentTypeError('expected integer') from None
        if not 1 <= n <= limit:
            raise argparse.ArgumentTypeError(f'expected 1..{limit}')
        return n
    return parse


def parser():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--mode', choices=('aot', 'jit'), default='jit')
    p.add_argument('--warmup', type=bounded(20), default=3)
    p.add_argument('--iterations', type=bounded(100), default=10)
    p.add_argument('--timeout', type=bounded(600), default=120)
    p.add_argument('--compile-timeout', type=bounded(600), default=300)
    return p


def stage(command, timeout, env=None, clock=time.monotonic_ns):
    start = clock()
    result = {'command': [str(x) for x in command], 'exit': None, 'output': '', 'error': None}
    try:
        proc = subprocess.Popen(result['command'], stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                text=True, errors='replace', env=env, start_new_session=True)
        try:
            result['output'] = proc.communicate(timeout=timeout)[0]
            result['exit'] = proc.returncode
        except subprocess.TimeoutExpired:
            # D22: terminate the compiler/runner's children too; retain output and timeout.
            # Cost: POSIX process groups; this standalone tool targets macOS only.
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            try:
                result['output'] = proc.communicate(timeout=5)[0]
            except subprocess.TimeoutExpired:
                # A descendant that left the process group still holds the pipe; do not hang.
                result['output'] = ''
            result['error'] = 'timeout'
    except OSError as error:
        result['error'] = str(error)
    result['elapsed_ns'] = clock() - start
    return result


def succeeded(result):
    return result['exit'] == 0 and result['error'] is None


def thermal(executable, preparation, timeout, run):
    if not succeeded(preparation):
        return {'state': 'unavailable', 'reason': 'thermal tool preparation failed',
                'evidence': preparation}
    result = run([executable], timeout)
    state = result['output'].strip()
    if succeeded(result) and state in ('nominal', 'fair', 'serious', 'critical'):
        return {'state': state, 'reason': None, 'evidence': result}
    return {'state': 'unavailable', 'reason': result['error'] or
            f'thermal tool exit={result["exit"]}, invalid or missing thermalState', 'evidence': result}


def samples(output, warmup, iterations):
    lines = [line.removeprefix('BENCHMARK_RESULT ') for line in output.splitlines()
             if line.startswith('BENCHMARK_RESULT ')]
    if len(lines) != 1:
        raise ValueError('expected exactly one BENCHMARK_RESULT')
    data = json.loads(lines[0])
    raw = data['samples_ns']
    if data['unit'] != 'ns' or data['warmup_count'] != warmup or data['sample_count'] != iterations:
        raise ValueError('units/counts mismatch')
    if not isinstance(raw, list) or len(raw) != iterations:
        raise ValueError('sample count mismatch')
    for value in raw + [data['first_dispatch_ns'], data['median_ns'], data['p95_ns']]:
        if type(value) not in (int, float) or not math.isfinite(value) or value < 0:
            raise ValueError('invalid elapsed time')
    median = statistics.median(raw)
    p95 = sorted(raw)[math.ceil(0.95 * len(raw)) - 1]
    if data['median_ns'] != median or data['p95_ns'] != p95:
        raise ValueError('summary differs from raw samples')
    return data


def collect(args, work, run=stage, prefix=None):
    prefix = Path(prefix or os.environ.get('CUMETAL_PREFIX', Path.home() / '.local/opt/cumetal'))
    env = dict(os.environ, CUMETAL_TRACE_GPU='1')
    env['PATH'] = f'{prefix}/libexec/cumetal/cuda_toolchain:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin'
    clang = '/opt/homebrew/opt/llvm/bin/clang++'
    cumetalc = '/opt/homebrew/bin/cumetalc'
    source = MATRIX / 'sgemm_handwritten.cu'
    executable = work / 'sgemm'
    obj = work / 'sgemm.o'
    thermal_bin = work / 'thermal'
    doc = {'schema': 'cumetal-sgemm-benchmark/1', 'mode': args.mode, 'status': 'failed',
           'performance_success': False, 'timing': None, 'compile_link_wall_ns': None,
           'warmup_count': args.warmup, 'sample_count': args.iterations, 'prefix': str(prefix),
           'host': dict(zip(('system', 'node', 'release', 'version', 'machine'), os.uname())),
           'caveat': 'host launch+synchronize wall time; first dispatch may include JIT; no D40 reservation',
           'sha256': {str(p.resolve().relative_to(HERE.parent)): hashlib.sha256(p.read_bytes()).hexdigest()
                      for p in (source, MATRIX / 'parity.h', MATRIX / 'common.h', MATRIX / 'classify.py',
                                HERE / 'thermal.swift', Path(__file__).resolve())}}
    # Tool preparation and version probes are outside compile/link and kernel intervals.
    preparation = run(['/usr/bin/xcrun', 'swiftc', HERE / 'thermal.swift',
                       '-module-cache-path', work / 'swift-module-cache', '-o', thermal_bin],
                      args.compile_timeout)
    doc['thermal_preparation'] = preparation
    doc['versions'] = {tool: run([tool, '--version'], 10, env=env)
                       for tool in ('/opt/homebrew/bin/cumetal', cumetalc, clang)}
    if args.mode == 'aot':
        commands = [[cumetalc, '-I', MATRIX, source, '-o', executable]]
    else:
        commands = [[clang, '-x', 'cuda', '-std=c++17', '-O2', '--cuda-gpu-arch=sm_80',
                     '-nocudainc', '-nocudalib', f'-I{prefix}/include', f'-I{MATRIX}',
                     '-include', 'cuda_runtime.h', '-c', source, '-o', obj],
                    ['/usr/bin/xcrun', 'clang++', obj, f'-L{prefix}/lib', '-lcumetal',
                     f'-Wl,-rpath,{prefix}/lib', '-o', executable]]
    doc['build'] = []
    for command in commands:
        result = run(command, args.compile_timeout, env=env)
        doc['build'].append(result)
        if not succeeded(result):
            doc['failure'] = 'compile/link failed'
            return doc
    doc['compile_link_wall_ns'] = sum(step['elapsed_ns'] for step in doc['build'])
    doc['thermal_before'] = thermal(thermal_bin, preparation, args.timeout, run)
    result = run([executable, '--benchmark', '--warmup', str(args.warmup), '--iterations',
                  str(args.iterations)], args.timeout, env=env)
    doc['run'] = result
    doc['thermal_after'] = thermal(thermal_bin, preparation, args.timeout, run)
    # Reuse the matrix's parity/provenance policy; repetitions here are one process,
    # never the matrix's three independent compatibility runs relabelled as samples.
    (work / 'bench.compile.rc').write_text('0')
    (work / 'bench.run1.rc').write_text(str(result['exit'] if result['exit'] is not None else
                                              (142 if result['error'] == 'timeout' else -1)))
    (work / 'bench.run1.out').write_text(result['output'])
    evidence = CLASSIFIER.classify_path(str(work), 'bench', True, repeats=1)
    doc['evidence'] = evidence
    try:
        doc['timing'] = samples(result['output'], args.warmup, args.iterations)
    except (ValueError, KeyError, TypeError) as error:
        doc['failure'] = f'invalid timing evidence: {error}'
        return doc
    if not succeeded(result) or evidence['class'] != 'supported':
        doc['failure'] = 'execution/parity/provenance failed'
    elif any(not succeeded(version) for version in doc['versions'].values()):
        doc['failure'] = 'tool version provenance unavailable'
    elif evidence['provenance']['kernel_launches'] != 1 + args.warmup + args.iterations:
        doc['failure'] = 'missing or extra dispatch provenance'
    else:
        doc['status'] = 'measured'
        doc['performance_success'] = True
    if not doc['performance_success']:
        # D22: a failed run must not expose numbers under the key consumers read as the result.
        doc['timing_unverified'] = doc.pop('timing', None)
    return doc


def main():
    args = parser().parse_args()
    with tempfile.TemporaryDirectory(prefix='cumetal-benchmark-') as tmp:
        doc = collect(args, Path(tmp))
    args.output.write_text(json.dumps(doc, indent=2, allow_nan=False) + '\n')
    return 0 if doc['performance_success'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
