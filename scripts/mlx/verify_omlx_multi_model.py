#!/usr/bin/env python3
"""On-device verification harness for KubeMetal issue #58 multi-model behavior.

The harness intentionally uses only HTTP APIs. It records load/switch/unload evidence and can
optionally run alternating chat probes. It does not assume an admin action succeeded merely from
transport success; every HTTP status/body is retained in the JSON evidence.
"""

from __future__ import annotations

import argparse
import json
import time
import urllib.error
import urllib.request
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


@dataclass
class Event:
    operation: str
    model: str | None
    status_code: int | None
    ok: bool
    latency_ms: float
    detail: str | None = None


def request(
    endpoint: str,
    method: str,
    path: str,
    api_key: str | None,
    body: dict[str, Any] | None = None,
    timeout: int = 300,
) -> tuple[int | None, str, float]:
    headers = {"Accept": "application/json"}
    payload = None
    if body is not None:
        headers["Content-Type"] = "application/json"
        payload = json.dumps(body).encode()
    if api_key:
        headers["Authorization"] = f"Bearer {api_key}"
    req = urllib.request.Request(
        endpoint.rstrip("/") + path,
        data=payload,
        headers=headers,
        method=method,
    )
    started = time.perf_counter()
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response:
            return response.status, response.read().decode(errors="replace"), (time.perf_counter() - started) * 1000
    except urllib.error.HTTPError as exc:
        return exc.code, exc.read().decode(errors="replace"), (time.perf_counter() - started) * 1000
    except (urllib.error.URLError, TimeoutError) as exc:
        return None, str(exc), (time.perf_counter() - started) * 1000


def event(endpoint: str, operation: str, model: str, api_key: str | None) -> Event:
    status, detail, latency = request(
        endpoint,
        "POST",
        f"/admin/api/models/{model}/{operation}",
        api_key,
    )
    return Event(
        operation=operation,
        model=model,
        status_code=status,
        ok=status is not None and 200 <= status < 300,
        latency_ms=latency,
        detail=detail[:1000] if detail else None,
    )


def chat(endpoint: str, model: str, prompt: str, api_key: str | None) -> Event:
    status, detail, latency = request(
        endpoint,
        "POST",
        "/v1/chat/completions",
        api_key,
        {
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0,
            "max_tokens": 32,
        },
    )
    return Event(
        operation="chat",
        model=model,
        status_code=status,
        ok=status is not None and 200 <= status < 300,
        latency_ms=latency,
        detail=detail[:1000] if not (status is not None and 200 <= status < 300) else None,
    )


def snapshot(endpoint: str, api_key: str | None) -> dict[str, Any]:
    status, body, latency = request(endpoint, "GET", "/admin/api/models", api_key, timeout=30)
    parsed: Any = None
    if body:
        try:
            parsed = json.loads(body)
        except json.JSONDecodeError:
            parsed = body[:2000]
    return {"status_code": status, "latency_ms": latency, "body": parsed}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--endpoint", default="http://127.0.0.1:8000")
    parser.add_argument("--model-a", required=True)
    parser.add_argument("--model-b", required=True)
    parser.add_argument("--cycles", type=int, default=2)
    parser.add_argument("--chat-rounds", type=int, default=1)
    parser.add_argument("--prompt", default="Reply with exactly one short sentence about Kubernetes.")
    parser.add_argument("--api-key")
    parser.add_argument("--leave-loaded", action="store_true")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    if args.cycles < 1 or args.chat_rounds < 0:
        parser.error("--cycles must be >= 1 and --chat-rounds must be >= 0")
    if args.model_a == args.model_b:
        parser.error("--model-a and --model-b must be different")
    if not (args.endpoint.startswith("http://127.0.0.1:") or args.endpoint.startswith("http://localhost:")):
        parser.error("endpoint must remain loopback-only")

    events: list[Event] = []
    snapshots: list[dict[str, Any]] = [{"label": "initial", **snapshot(args.endpoint, args.api_key)}]

    for cycle in range(args.cycles):
        for model in (args.model_a, args.model_b):
            events.append(event(args.endpoint, "load", model, args.api_key))
        snapshots.append({"label": f"loaded-cycle-{cycle + 1}", **snapshot(args.endpoint, args.api_key)})
        for _ in range(args.chat_rounds):
            events.append(chat(args.endpoint, args.model_a, args.prompt, args.api_key))
            events.append(chat(args.endpoint, args.model_b, args.prompt, args.api_key))
        if cycle < args.cycles - 1:
            events.append(event(args.endpoint, "unload", args.model_a, args.api_key))
            events.append(event(args.endpoint, "load", args.model_a, args.api_key))
            events.append(event(args.endpoint, "unload", args.model_b, args.api_key))
            events.append(event(args.endpoint, "load", args.model_b, args.api_key))
            snapshots.append({"label": f"switch-cycle-{cycle + 1}", **snapshot(args.endpoint, args.api_key)})

    if not args.leave_loaded:
        events.append(event(args.endpoint, "unload", args.model_a, args.api_key))
        events.append(event(args.endpoint, "unload", args.model_b, args.api_key))
    snapshots.append({"label": "final", **snapshot(args.endpoint, args.api_key)})

    failed = [item for item in events if not item.ok]
    report = {
        "schema_version": 1,
        "captured_at": datetime.now(timezone.utc).isoformat(),
        "endpoint": args.endpoint,
        "models": [args.model_a, args.model_b],
        "cycles": args.cycles,
        "chat_rounds": args.chat_rounds,
        "leave_loaded": args.leave_loaded,
        "success": not failed,
        "events": [asdict(item) for item in events],
        "snapshots": snapshots,
        "notes": [
            "Use alongside KubeMetal Metal/memory diagnostics to distinguish functional success from memory pressure.",
            "This harness verifies API-visible behavior; scheduler fairness and cache reuse require benchmark/log evidence.",
        ],
    }
    rendered = json.dumps(report, indent=2, ensure_ascii=False)
    print(rendered)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered + "\n")
    return 0 if not failed else 1


if __name__ == "__main__":
    raise SystemExit(main())
