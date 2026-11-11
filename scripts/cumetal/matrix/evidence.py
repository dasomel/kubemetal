"""Seal and verify portable matrix evidence; never retain build products."""
import hashlib
import json
from pathlib import Path
import sys

INDEX = 'manifest/evidence.json'


def file_sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def inventory(root):
    files = {}
    for path in sorted(root.rglob('*')):
        if path.is_symlink():
            raise ValueError(f'symlink in evidence: {path}')
        if not path.is_file() or path == root / INDEX:
            continue
        if path.suffix in ('.bin', '.o'):
            raise ValueError(f'build product in evidence: {path}')
        files[path.relative_to(root).as_posix()] = file_sha256(path)
    return files


def seal(root, repeats, complete):
    root = Path(root)
    doc = {'schema': 'cumetal-evidence/1', 'repeats': repeats,
           'complete': complete, 'files': inventory(root)}
    with (root / INDEX).open('x') as stream:
        json.dump(doc, stream, indent=2, sort_keys=True)
        stream.write('\n')


def verify(root, repeats):
    root = Path(root)
    files = inventory(root)
    doc = json.loads((root / INDEX).read_text())
    if doc['schema'] != 'cumetal-evidence/1' or doc['repeats'] != repeats:
        raise ValueError('evidence schema/repeats mismatch')
    if not doc['complete']:
        raise ValueError('partial matrix execution; evidence is incomplete')
    if files != doc['files']:
        raise ValueError('evidence hash/inventory mismatch')
    return doc


if __name__ == '__main__':
    seal(sys.argv[1], int(sys.argv[2]), sys.argv[3] == 'complete')
