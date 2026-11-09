#!/usr/bin/env python3
"""Opt-in bounded temporary-file I/O measurement; no MLX or cache mutation.

Reads are buffered and may be served entirely from the OS cache. fsync is not
macOS F_FULLFSYNC and does not establish power-loss durability or cold-disk speed.
"""
import argparse
import json
import os
import platform
import sys
import tempfile
import time
from pathlib import Path

MIB = 1024 * 1024
DEFAULT_SIZE_MIB = 16
MAX_SIZE_MIB = 256
CHUNK_BYTES = MIB
LIMITATIONS = [
    "Buffered sequential I/O; reads may be served from the OS cache.",
    "No cache eviction, direct I/O, shared model-cache access, or cold-disk claim.",
    "write_fsync includes write, flush and os.fsync, not macOS F_FULLFSYNC; "
    "power-loss durability is not established.",
    "Single sample; filesystem compression, storage caches and host load affect results.",
]


class JsonArgumentParser(argparse.ArgumentParser):
    def error(self, message):
        raise ValueError(message)


def new_report():
    return {
        "schema_version": 1,
        "status": "error",
        "requested_bytes": None,
        "environment": {
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
            "python": platform.python_version(),
            "directory": None,
        },
        "write_fsync": {"bytes": 0, "elapsed_seconds": None},
        "read": {"bytes": 0, "elapsed_seconds": None},
        "temporary_file_closed": None,
        "limitations": LIMITATIONS,
        "errors": [],
    }


def record_error(report, phase, error):
    report["errors"].append({
        "phase": phase,
        "type": type(error).__name__,
        "message": str(error),
        "errno": getattr(error, "errno", None),
    })


def elapsed_since(start):
    elapsed = time.perf_counter() - start
    if elapsed <= 0:
        raise ValueError(f"non-positive elapsed time: {elapsed}")
    return elapsed


def measure(report, directory, size_bytes):
    file = None
    phase = "validation"
    try:
        if not 1 <= size_bytes <= MAX_SIZE_MIB * MIB:
            raise ValueError(f"size must be 1..{MAX_SIZE_MIB * MIB} bytes")
        report["requested_bytes"] = size_bytes
        directory = Path(directory).resolve(strict=True)
        report["environment"]["directory"] = str(directory)
        phase = "create"
        # A private anonymous file leaves no payload to export; retain the JSON
        # instead. POSIX unlink also covers process termination.
        file = tempfile.TemporaryFile(mode="w+b", dir=directory)
        report["temporary_file_closed"] = False
        phase = "prepare"
        payload = os.urandom(min(CHUNK_BYTES, size_bytes))
        phase = "write_fsync"
        start = time.perf_counter()
        remaining = size_bytes
        while remaining:
            chunk = payload[:min(len(payload), remaining)]
            written = file.write(chunk)
            report["write_fsync"]["bytes"] += written
            if written != len(chunk):
                raise OSError(f"short write: {written}/{len(chunk)} bytes")
            remaining -= written
        file.flush()
        os.fsync(file.fileno())
        report["write_fsync"]["elapsed_seconds"] = elapsed_since(start)
        phase = "read"
        file.seek(0)
        start = time.perf_counter()
        remaining = size_bytes
        while remaining:
            chunk = file.read(min(len(payload), remaining))
            report["read"]["bytes"] += len(chunk)
            if chunk != payload[:min(len(payload), remaining)]:
                raise OSError("short read or data mismatch")
            remaining -= len(chunk)
        report["read"]["elapsed_seconds"] = elapsed_since(start)
    except (Exception, KeyboardInterrupt) as error:
        # D22: failed phases keep null timings, so no throughput is available;
        # rerun explicitly after fixing the recorded error instead of using zero.
        record_error(report, phase, error)
    finally:
        if file is not None:
            try:
                file.close()
                report["temporary_file_closed"] = True
            except (Exception, KeyboardInterrupt) as error:
                record_error(report, "cleanup", error)
    report["status"] = "error" if report["errors"] else "ok"


def main(argv=None):
    report = new_report()
    parser = JsonArgumentParser(description=__doc__, allow_abbrev=False)
    parser.add_argument("--directory", default=None,
                        help="existing directory on the target filesystem (default: OS temp)")
    parser.add_argument("--size-mib", type=int, default=DEFAULT_SIZE_MIB,
                        help=f"temporary file size, 1..{MAX_SIZE_MIB} MiB (default: %(default)s)")
    try:
        args = parser.parse_args(argv)
        measure(report, args.directory if args.directory is not None else tempfile.gettempdir(),
                args.size_mib * MIB)
    except (Exception, KeyboardInterrupt) as error:
        record_error(report, "arguments", error)
    print(json.dumps(report, allow_nan=False))
    return 0 if report["status"] == "ok" else 1


if __name__ == "__main__":
    sys.exit(main())
