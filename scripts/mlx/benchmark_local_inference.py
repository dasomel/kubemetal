#!/usr/bin/env python3
"""Reproducible local inference benchmark for KubeMetal issue #58.

Uses only Python stdlib so it can run in offline/air-gapped environments. It intentionally
measures the API boundary rather than importing oMLX internals; the same harness therefore
works with oMLX and the existing mlx_lm.server baseline.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import platform
import statistics
import subprocess
import time
import urllib.error
import urllib.request
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


@dataclass
class Sample:
    ok: bool
    latency_ms: float
    prompt_tokens: int | None = None
    completion_tokens: int | None = None
    total_tokens: int | None = None
    error: str | None = None


def request_once(endpoint: str, model: str, prompt: str, max_tokens: int, api_key: str | None) -> Sample:
    payload = json.dumps(
        {
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": max_tokens,
            "temperature": 0,
            "stream": False,
        }
    ).encode()
    headers = {"Content-Type": "application/json"}
    if api_key:
        headers["Authorization"] = f"Bearer {api_key}"
    req = urllib.request.Request(
        endpoint.rstrip("/") + "/v1/chat/completions",
        data=payload,
        headers=headers,
        method="POST",
    )
    started = time.perf_counter()
    try:
        with urllib.request.urlopen(req, timeout=300) as response:
            data = json.loads(response.read())
        latency_ms = (time.perf_counter() - started) * 1000
        usage = data.get("usage") or {}
        return Sample(
            ok=True,
            latency_ms=latency_ms,
            prompt_tokens=usage.get("prompt_tokens"),
            completion_tokens=usage.get("completion_tokens"),
            total_tokens=usage.get("total_tokens"),
        )
    except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError, json.JSONDecodeError) as exc:
        return Sample(ok=False, latency_ms=(time.perf_counter() - started) * 1000, error=str(exc))


def command_output(*args: str) -> str | None:
    try:
        return subprocess.check_output(args, text=True, stderr=subprocess.DEVNULL, timeout=5).strip()
    except (OSError, subprocess.SubprocessError):
        return None


def percentile(values: list[float], p: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    index = min(len(ordered) - 1, max(0, round((len(ordered) - 1) * p)))
    return ordered[index]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--endpoint", default="http://127.0.0.1:8000")
    parser.add_argument("--model", required=True)
    parser.add_argument("--prompt", default="Explain Kubernetes reconciliation in three concise sentences.")
    parser.add_argument("--max-tokens", type=int, default=128)
    parser.add_argument("--requests", type=int, default=8)
    parser.add_argument("--concurrency", type=int, default=1)
    parser.add_argument("--runtime", choices=["omlx", "mlx-lm"], required=True)
    parser.add_argument("--cache-state", choices=["cold", "warm", "ssd-restore", "unknown"], default="unknown")
    parser.add_argument("--api-key")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    if args.requests < 1 or args.concurrency < 1:
        parser.error("--requests and --concurrency must be positive")

    started = time.perf_counter()
    samples: list[Sample] = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.concurrency) as executor:
        futures = [
            executor.submit(
                request_once,
                args.endpoint,
                args.model,
                args.prompt,
                args.max_tokens,
                args.api_key,
            )
            for _ in range(args.requests)
        ]
        for future in concurrent.futures.as_completed(futures):
            samples.append(future.result())
    wall_seconds = time.perf_counter() - started

    successful = [sample for sample in samples if sample.ok]
    latencies = [sample.latency_ms for sample in successful]
    completion_tokens = sum(sample.completion_tokens or 0 for sample in successful)
    report: dict[str, Any] = {
        "schema_version": 1,
        "captured_at": datetime.now(timezone.utc).isoformat(),
        "runtime": args.runtime,
        "runtime_version": command_output("omlx", "--version") if args.runtime == "omlx" else None,
        "endpoint": args.endpoint,
        "model": args.model,
        "cache_state": args.cache_state,
        "traffic": {
            "requests": args.requests,
            "concurrency": args.concurrency,
            "max_tokens": args.max_tokens,
            "prompt_chars": len(args.prompt),
        },
        "host": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "macos": platform.mac_ver()[0],
            "chip": command_output("sysctl", "-n", "machdep.cpu.brand_string"),
            "memory_bytes": command_output("sysctl", "-n", "hw.memsize"),
        },
        "summary": {
            "success": len(successful),
            "failed": len(samples) - len(successful),
            "wall_seconds": wall_seconds,
            "requests_per_second": len(successful) / wall_seconds if wall_seconds else None,
            "completion_tokens_per_second": completion_tokens / wall_seconds if wall_seconds else None,
            "latency_ms_p50": statistics.median(latencies) if latencies else None,
            "latency_ms_p95": percentile(latencies, 0.95),
            "latency_ms_max": max(latencies) if latencies else None,
        },
        "samples": [asdict(sample) for sample in samples],
    }

    rendered = json.dumps(report, indent=2, ensure_ascii=False)
    print(rendered)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered + "\n")
    return 0 if len(successful) == len(samples) else 1


if __name__ == "__main__":
    raise SystemExit(main())
