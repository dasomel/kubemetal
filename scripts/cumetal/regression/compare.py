"""Compare two retained matrices offline; never run a compiler or GPU probe."""
import argparse
import collections
import json
from pathlib import Path
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'matrix'))
import classify
import evidence

STATES = {'supported', 'partial', 'failed', 'unavailable', 'unverified'}
HOST_FIELDS = ('mac_model', 'soc', 'ram_bytes', 'gpu_cores', 'macos',
               'product_version', 'build')
VERSIONS = ('cumetal_version', 'llvm_clang', 'xcrun_clang', 'metal_version', 'xcode_version')


def load(path):
    path = Path(path).resolve()
    doc = json.loads(path.read_text())
    if doc.get('schema') != 'cumetal-matrix/1':
        raise ValueError('unsupported matrix schema')
    repeats = doc.get('repeats')
    if type(repeats) is not int or repeats <= 0:
        raise ValueError('invalid repeats')
    declared = doc['evidence']
    if declared['index'] != evidence.INDEX:
        raise ValueError('invalid evidence index')
    root = path.parent / declared['directory']
    # D22: replay verifies evidence.verify plus the original index/file digests.
    rebuilt = classify.classify_matrix(str(root), repeats, str(path))
    for field in ('manifest', 'results'):
        if doc[field] != rebuilt[field]:
            raise ValueError(f'matrix {field} differs from verified replay')
    manifest = doc['manifest']
    host = manifest.get('host', {})
    if host.get('schema') != 'cumetal-host/1' or host.get('errors') != {}:
        raise ValueError('missing or failed host identification')
    if any(host.get(key) in (None, '') for key in HOST_FIELDS + VERSIONS):
        raise ValueError('incomplete host/version identity')
    if not manifest.get('prefix') or manifest.get('doctor_rc') != '0':
        raise ValueError('missing execution conditions or failed doctor')
    sources = {name.removeprefix('manifest/sources/'): digest
               for name, digest in declared['files'].items()
               if name.startswith('manifest/sources/')}
    if not sources or manifest.get('probe_sha256') != sources:
        raise ValueError('source digest manifest mismatch')
    rows = {}
    for row in doc['results']:
        name = row['probe']
        if name in rows:
            raise ValueError(f'duplicate probe: {name}')
        if f'{name}.cu' not in sources:
            raise ValueError(f'missing probe source: {name}')
        for mode in ('aot', 'jit'):
            if row.get(mode, {}).get('class') not in STATES:
                raise ValueError(f'missing/invalid {mode} state: {name}')
        rows[name] = row
    if not rows:
        raise ValueError('empty probe matrix')
    return doc, rows


def compare(baseline, candidate):
    before, old = load(baseline)
    after, new = load(candidate)
    if old.keys() != new.keys():
        raise ValueError('probe set mismatch')
    left, right = before['manifest'], after['manifest']
    if any(left['host'][key] != right['host'][key] for key in HOST_FIELDS):
        raise ValueError('host mismatch')
    # D22: only timestamps and identified runtime/toolchain versions may differ.
    # Cost: paths/extra conditions must match; a new policy needs explicit review.
    def conditions(manifest):
        return {key: value for key, value in manifest.items()
                if key not in {'date_utc', 'host', *VERSIONS}}
    if before['repeats'] != after['repeats'] or conditions(left) != conditions(right):
        raise ValueError('source/execution conditions mismatch')
    changes = []
    for name in sorted(old):
        if old[name]['lib'] != new[name]['lib']:
            raise ValueError(f'library condition mismatch: {name}')
        for mode in ('aot', 'jit'):
            prev, curr = old[name][mode], new[name][mode]
            a, b = prev['class'], curr['class']
            if a == 'supported' and b in ('failed', 'unavailable'):
                change = 'regression'
            elif 'unverified' in (a, b) or (a == 'supported' and b == 'partial'):
                change = 'incomparable'
            elif a == b:
                change = 'unchanged'
            elif b == 'supported':
                change = 'improvement'
            else:
                change = 'changed'
            changes.append({'probe': name, 'mode': mode, 'before': a, 'after': b,
                            'change': change, 'baseline_evidence': prev,
                            'candidate_evidence': curr})
    counts = dict(collections.Counter(row['change'] for row in changes))
    verdict = ('regression' if counts.get('regression') else
               'incomparable' if counts.get('incomparable') else 'no_regression')
    return {'schema': 'cumetal-regression/1', 'baseline': str(Path(baseline).resolve()),
            'candidate': str(Path(candidate).resolve()), 'verdict': verdict,
            'versions': {key: {'before': left['host'][key], 'after': right['host'][key]}
                         for key in VERSIONS}, 'summary': counts, 'results': changes}


def markdown(doc):
    def cell(value):
        return str(value).replace('|', '&#124;').replace('\n', '<br>')
    lines = ['# CuMetal compatibility regression', '', f"Verdict: {doc['verdict']}", '',
             '| probe | mode | before | after | change | candidate diagnostics |',
             '|---|---|---|---|---|---|']
    for row in doc['results']:
        detail = row['candidate_evidence']
        # Keep parity lines, numerical mismatch caveats and error tails in both formats.
        diagnostic = json.dumps(detail, ensure_ascii=False, sort_keys=True)
        lines.append('| ' + ' | '.join(cell(row[key]) for key in
                     ('probe', 'mode', 'before', 'after', 'change')) +
                     ' | ' + cell(diagnostic) + ' |')
    return '\n'.join(lines) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('baseline', type=Path)
    parser.add_argument('candidate', type=Path)
    parser.add_argument('output', type=Path, help='new output JSON; Markdown written alongside')
    args = parser.parse_args()
    md = args.output.with_suffix('.md')
    try:
        if args.output.suffix != '.json':
            raise ValueError('output must end in .json')
        if args.output.exists() or md.exists():
            raise ValueError('destination exists')
        doc = compare(args.baseline, args.candidate)
        staged = []
        published = []
        try:
            for destination, content in (
                    (args.output, json.dumps(doc, indent=2, ensure_ascii=False, sort_keys=True) + '\n'),
                    (md, markdown(doc))):
                fd, name = tempfile.mkstemp(prefix=f'.{destination.name}.', dir=destination.parent)
                temporary = Path(name)
                staged.append(temporary)
                with open(fd, 'w') as stream:
                    stream.write(content)
            for temporary, destination in zip(staged, (args.output, md)):
                temporary.rename(destination)
                published.append(destination)
        except BaseException:
            for path in staged:
                path.unlink(missing_ok=True)
            for path in published:
                path.unlink(missing_ok=True)
            raise
        return {'no_regression': 0, 'regression': 1, 'incomparable': 2}[doc['verdict']]
    except (OSError, ValueError, KeyError, TypeError, AttributeError) as exc:
        print(f'comparison error: {exc}', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
