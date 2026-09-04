#!/usr/bin/env python3
"""Reproducible local inference benchmark for KubeMetal issue #58.

Stdlib-only so it can run in offline/air-gapped environments. It measures the
OpenAI-compatible API boundary and works with both oMLX and mlx_lm.server. Values that
cannot be measured at this boundary stay null instead of being fabricated.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import os
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
    ttft_ms: float | None = None
    prompt_tokens: int | None = None
    completion_tokens: int | None = None
    total_tokens: int | None = None
    prefill_tokens_per_second: float | None = None
    generation_tokens_per_second: float | None = None
    error: str | None = None


def build_request(endpoint: str, payload: dict[str, Any], api_key: str | None) -> urllib.request.Request:
    headers = {"Content-Type": "application/json", "Accept": "application/json"}
    if api_key:
        headers["Authorization"] = f"Bearer {api_key}"
    return urllib.request.Request(
        endpoint.rstrip("/") + "/v1/chat/completions",
        data=json.dumps(payload).encode(),
        headers=headers,
        method="POST",
    )


def request_non_streaming(
    endpoint: str, model: str, prompt: str, max_tokens: int, api_key: str | None
) -> Sample:
    req = build_request(
        endpoint,
        {
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": max_tokens,
            "temperature": 0,
            "stream": False,
        },
        api_key,
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


def request_streaming(
    endpoint: str, model: str, prompt: str, max_tokens: int, api_key: str | None
) -> Sample:
    req = build_request(
        endpoint,
        {
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": max_tokens,
            "temperature": 0,
            "stream": True,
            "stream_options": {"include_usage": True},
        },
        api_key,
    )
    started = time.perf_counter()
    first_data_at: float | None = None
    usage: dict[str, Any] = {}
    try:
        with urllib.request.urlopen(req, timeout=300) as response:
            for raw_line in response:
                line = raw_line.decode(errors="replace").strip()
                if not line.startswith("data:"):
                    continue
                data = line[5:].strip()
                if data == "[DONE]":
                    break
                if first_data_at is None:
                    first_data_at = time.perf_counter()
                try:
                    event = json.loads(data)
                except json.JSONDecodeError:
                    continue
                if event.get("usage"):
                    usage = event["usage"]
        completed = time.perf_counter()
        ttft_ms = ((first_data_at - started) * 1000) if first_data_at else None
        prompt_tokens = usage.get("prompt_tokens")
        completion_tokens = usage.get("completion_tokens")
        prefill_tps = None
        generation_tps = None
        if prompt_tokens and ttft_ms and ttft_ms > 0:
            # API-boundary approximation: TTFT also includes scheduling/network/model-load overhead.
            # Keep the provenance explicit in the report rather than presenting this as engine-only prefill.
            prefill_tps = prompt_tokens / (ttft_ms / 1000)
        if completion_tokens and first_data_at and completed > first_data_at:
            generation_tps = completion_tokens / (completed - first_data_at)
        return Sample(
            ok=True,
            latency_ms=(completed - started) * 1000,
            ttft_ms=ttft_ms,
            prompt_tokens=prompt_tokens,
            completion_tokens=completion_tokens,
            total_tokens=usage.get("total_tokens"),
            prefill_tokens_per_second=prefill_tps,
            generation_tokens_per_second=generation_tps,
        )
    except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError) as exc:
        return Sample(ok=False, latency_ms=(time.perf_counter() - started) * 1000, error=str(exc))


def command_output(*args: str) -> str | None:
    try:
        return subprocess.check_output(args, text=True, stderr=subprocess.DEVNULL, timeout=5).strip()
    except (OSError, subprocess.SubprocessError):
        return None


def int_command_output(*args: str) -> int | None:
    raw = command_output(*args)
    if raw is None:
        return None
    try:
        return int(raw.strip())
    except ValueError:
        return None


def process_rss_bytes(pid: int | None) -> int | None:
    if not pid:
        return None
    raw = command_output("ps", "-o", "rss=", "-p", str(pid))
    if not raw:
        return None
    try:
        return int(raw.split()[0]) * 1024
    except (ValueError, IndexError):
        return None


def directory_bytes(path: Path | None) -> int | None:
    if path is None or not path.exists():
        return None
    total = 0
    try:
        for root, dirs, files in os.walk(path, followlinks=False):
            dirs[:] = [name for name in dirs if not (Path(root) / name).is_symlink()]
            for name in files:
                candidate = Path(root) / name
                if candidate.is_symlink():
                    continue
                try:
                    total += candidate.stat().st_size
                except OSError:
                    continue
    except OSError:
        return None
    return total


def percentile(values: list[float], p: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    index = min(len(ordered) - 1, max(0, round((len(ordered) - 1) * p)))
    return ordered[index]


def optional_median(values: list[float | None]) -> float | None:
    measured = [value for value in values if value is not None]
    return statistics.median(measured) if measured else None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--endpoint", default="http://127.0.0.1:8000")
    parser.add_argument("--model", required=True)
    parser.add_argument("--model-repo")
    parser.add_argument("--model-revision")
    parser.add_argument("--model-digest")
    parser.add_argument("--quantization")
    parser.add_argument("--prompt", default="Explain Kubernetes reconciliation in three concise sentences.")
    parser.add_argument("--max-tokens", type=int, default=128)
    parser.add_argument("--requests", type=int, default=8)
    parser.add_argument("--concurrency", type=int, default=1)
    parser.add_argument("--runtime", choices=["omlx", "mlx-lm"], required=True)
    parser.add_argument("--runtime-pid", type=int)
    parser.add_argument("--cache-state", choices=["cold", "warm", "ssd-restore", "unknown"], default="unknown")
    parser.add_argument("--cache-dir", type=Path)
    parser.add_argument("--stream", action="store_true", help="Use SSE streaming and measure TTFT")
    parser.add_argument("--api-key")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    if args.requests < 1 or args.concurrency < 1:
        parser.error("--requests and --concurrency must be positive")
    if args.runtime_pid is not None and args.runtime_pid <= 0:
        parser.error("--runtime-pid must be positive")

    worker = request_streaming if args.stream else request_non_streaming
    rss_before = process_rss_bytes(args.runtime_pid)
    cache_bytes_before = directory_bytes(args.cache_dir)
    started = time.perf_counter()
    samples: list[Sample] = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.concurrency) as executor:
        futures = [
            executor.submit(worker, args.endpoint, args.model, args.prompt, args.max_tokens, args.api_key)
            for _ in range(args.requests)
        ]
        for future in concurrent.futures.as_completed(futures):
            samples.append(future.result())
    wall_seconds = time.perf_counter() - started
    rss_after = process_rss_bytes(args.runtime_pid)
    cache_bytes_after = directory_bytes(args.cache_dir)

    successful = [sample for sample in samples if sample.ok]
    latencies = [sample.latency_ms for sample in successful]
    ttfts = [sample.ttft_ms for sample in successful if sample.ttft_ms is not None]
    completion_tokens = sum(sample.completion_tokens or 0 for sample in successful)
    report: dict[str, Any] = {
        "schema_version": 2,
        "captured_at": datetime.now(timezone.utc).isoformat(),
        "runtime": args.runtime,
        "runtime_version": command_output("omlx", "--version") if args.runtime == "omlx" else None,
        "runtime_pid": args.runtime_pid,
        "endpoint": args.endpoint,
        "model": {
            "id": args.model,
            "repo": args.model_repo,
            "revision": args.model_revision,
            "digest": args.model_digest,
            "quantization": args.quantization,
        },
        "cache": {
            "state": args.cache_state,
            "directory": str(args.cache_dir) if args.cache_dir else None,
            "bytes_before": cache_bytes_before,
            "bytes_after": cache_bytes_after,
        },
        "traffic": {
            "requests": args.requests,
            "concurrency": args.concurrency,
            "max_tokens": args.max_tokens,
            "prompt_chars": len(args.prompt),
            "stream": args.stream,
        },
        "host": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "macos": platform.mac_ver()[0],
            "chip": command_output("sysctl", "-n", "machdep.cpu.brand_string"),
            "memory_bytes": int_command_output("sysctl", "-n", "hw.memsize"),
            "memory_pressure_level": command_output("sysctl", "-n", "kern.memorystatus_vm_pressure_level"),
            "thermal_state": command_output("pmset", "-g", "therm"),
            "metal_wired_limit_mb": int_command_output("sysctl", "-n", "iogpu.wired_limit_mb"),
        },
        "process": {
            "rss_bytes_before": rss_before,
            "rss_bytes_after": rss_after,
            "peak_unified_memory_bytes": None,
            "note": "RSS is measured at the process boundary; peak unified/Metal allocation requires runtime or host telemetry and remains null here.",
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
            "ttft_ms_p50": statistics.median(ttfts) if ttfts else None,
            "ttft_ms_p95": percentile(ttfts, 0.95),
            "prefill_tokens_per_second_p50_api_boundary": optional_median(
                [sample.prefill_tokens_per_second for sample in successful]
            ),
            "generation_tokens_per_second_p50": optional_median(
                [sample.generation_tokens_per_second for sample in successful]
            ),
            "queue_wait_ms": None,
            "model_load_ms": None,
        },
        "measurement_notes": [
            "API-boundary prefill tok/s is prompt_tokens / TTFT and includes scheduling/network/model-load overhead.",
            "queue wait and engine-only model load time remain null unless the upstream runtime exposes trustworthy values.",
            "unsupported values are null, never zero-filled.",
        ],
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
