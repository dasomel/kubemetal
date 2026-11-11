"""Bounded, read-only matrix host identification; no serial/UUID probes."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess


def text(value):
    return value.decode('utf-8', errors='replace') if isinstance(value, bytes) else (value or '')


def run_probe(argv, timeout=5):
    result = {'argv': argv, 'stdout': '', 'stderr': '', 'returncode': None, 'error': None}
    try:
        proc = subprocess.run(argv, capture_output=True, text=True, encoding='utf-8', errors='replace',
                              timeout=timeout, env={**os.environ, 'LC_ALL': 'C'})
        result.update(stdout=proc.stdout, stderr=proc.stderr, returncode=proc.returncode,
                      error=f'exit {proc.returncode}' if proc.returncode != 0 else None)
    except subprocess.TimeoutExpired as exc:
        result.update(stdout=text(exc.stdout), stderr=text(exc.stderr), error=f'timeout after {timeout}s')
    except OSError as exc:
        result['error'] = str(exc)
    return result


def positive_integer(value):
    if type(value) not in (str, int) or not re.fullmatch(r'[0-9]+', str(value).strip()):
        raise ValueError('expected positive integer')
    number = int(value)
    if number <= 0:
        raise ValueError('expected positive integer')
    return number


def interpret(probes):
    result = {'schema': 'cumetal-host/1', 'probes': probes, 'errors': {}}
    fields = ('mac_model', 'soc', 'ram_bytes', 'gpu_cores', 'macos', 'xcode_version',
              'cumetal_version', 'llvm_clang', 'xcrun_clang', 'metal_version')
    for field in fields:
        result[field] = None
        try:
            probe = probes.get(field)
            if not probe:
                raise ValueError('probe missing')
            if probe.get('error') or probe.get('returncode') != 0:
                raise ValueError(probe.get('error') or f"exit {probe.get('returncode')}")
            value = probe.get('stdout', '').strip()
            if not value:
                raise ValueError('empty stdout')
            if field == 'ram_bytes':
                value = positive_integer(value)
            elif field == 'gpu_cores':
                devices = json.loads(value)['SPDisplaysDataType']
                if not isinstance(devices, list) or len(devices) != 1:
                    raise ValueError('expected one GPU; missing or ambiguous devices')
                value = positive_integer(devices[0]['sppci_cores'])
            elif field == 'xcode_version':
                if not re.fullmatch(r'Xcode [0-9][^\n]*\nBuild version \S+', value):
                    raise ValueError('missing Xcode version/build identity (CLT is not Xcode)')
            elif field == 'macos':
                if not re.search(r'^ProductVersion:\s*\S+', value, re.M) or not re.search(r'^BuildVersion:\s*\S+', value, re.M):
                    raise ValueError('missing macOS version/build identity')
            else:
                value = value.splitlines()[0]
            result[field] = value
        except (ValueError, KeyError, TypeError) as exc:
            result['errors'][field] = str(exc)
    return result


def collect(cumetal, clang, runner=run_probe):
    # D22: mini omits personal information; request displays only, never hardware serials.
    # Cost: missing/ambiguous GPU counts stay null; raw probes allow later parser fixes.
    commands = {
        'mac_model': ['/usr/sbin/sysctl', '-n', 'hw.model'],
        'soc': ['/usr/sbin/sysctl', '-n', 'machdep.cpu.brand_string'],
        'ram_bytes': ['/usr/sbin/sysctl', '-n', 'hw.memsize'],
        'gpu_cores': ['/usr/sbin/system_profiler', 'SPDisplaysDataType', '-detailLevel', 'mini', '-json'],
        'macos': ['/usr/bin/sw_vers'],
        'xcode_version': ['/usr/bin/xcodebuild', '-version'],
        'cumetal_version': [cumetal, '--version'],
        'llvm_clang': [clang, '--version'],
        'xcrun_clang': ['/usr/bin/xcrun', 'clang++', '--version'],
        'metal_version': ['/usr/bin/xcrun', 'metal', '--version'],
    }
    return interpret({field: runner(argv) for field, argv in commands.items()})


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    parser.add_argument('--cumetal', required=True)
    parser.add_argument('--clang', required=True)
    args = parser.parse_args()
    with args.output.open('x', encoding='utf-8') as stream:
        json.dump(collect(args.cumetal, args.clang), stream, indent=2, ensure_ascii=False, sort_keys=True)
        stream.write('\n')
