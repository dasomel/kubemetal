#!/usr/bin/env python3
"""Compare KubeMetal #58 benchmark evidence without collapsing metrics into one score."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


LOWER_IS_BETTER = ("latency_ms_p50", "latency_ms_p95", "ttft_ms_p50", "ttft_ms_p95")
HIGHER_IS_BETTER = (
    "requests_per_second",
    "completion_tokens_per_second",
    "prefill_tokens_per_second_p50_api_boundary",
    "generation_tokens_per_second_p50",
)


def load(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text())
    if not isinstance(value, dict):
        raise ValueError(f"benchmark root must be an object: {path}")
    return value


def number(value: Any) -> float | None:
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return float(value)
    return None


def pct_change(old: float, new: float) -> float | None:
    if old == 0:
        return None
    return ((new - old) / old) * 100


def identity(report: dict[str, Any]) -> dict[str, Any]:
    return {
        "model": report.get("model"),
        "traffic": report.get("traffic"),
        "host_machine": (report.get("host") or {}).get("machine"),
        "host_chip": (report.get("host") or {}).get("chip"),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--max-latency-regression-pct", type=float, default=15.0)
    parser.add_argument("--max-throughput-regression-pct", type=float, default=15.0)
    parser.add_argument("--allow-profile-difference", action="store_true")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    baseline = load(args.baseline)
    candidate = load(args.candidate)
    failures: list[str] = []
    warnings: list[str] = []
    metrics: dict[str, Any] = {}

    if identity(baseline) != identity(candidate):
        message = "benchmark identity/profile differs; performance comparison is not apples-to-apples"
        if args.allow_profile_difference:
            warnings.append(message)
        else:
            failures.append(message)

    base_summary = baseline.get("summary") or {}
    cand_summary = candidate.get("summary") or {}
    if number(cand_summary.get("failed")) not in (None, 0.0):
        failures.append(f"candidate contains {cand_summary.get('failed')} failed request(s)")

    for name in LOWER_IS_BETTER:
        old = number(base_summary.get(name))
        new = number(cand_summary.get(name))
        if old is None or new is None:
            metrics[name] = {"baseline": old, "candidate": new, "change_pct": None, "status": "unavailable"}
            continue
        change = pct_change(old, new)
        regressed = change is not None and change > args.max_latency_regression_pct
        metrics[name] = {
            "baseline": old,
            "candidate": new,
            "change_pct": change,
            "status": "fail" if regressed else "pass",
        }
        if regressed:
            failures.append(f"{name} regressed by {change:.1f}%")

    for name in HIGHER_IS_BETTER:
        old = number(base_summary.get(name))
        new = number(cand_summary.get(name))
        if old is None or new is None:
            metrics[name] = {"baseline": old, "candidate": new, "change_pct": None, "status": "unavailable"}
            continue
        change = pct_change(old, new)
        regression = -change if change is not None and change < 0 else 0.0
        regressed = regression > args.max_throughput_regression_pct
        metrics[name] = {
            "baseline": old,
            "candidate": new,
            "change_pct": change,
            "status": "fail" if regressed else "pass",
        }
        if regressed:
            failures.append(f"{name} regressed by {regression:.1f}%")

    report = {
        "schema_version": 1,
        "baseline": str(args.baseline),
        "candidate": str(args.candidate),
        "thresholds": {
            "max_latency_regression_pct": args.max_latency_regression_pct,
            "max_throughput_regression_pct": args.max_throughput_regression_pct,
        },
        "metrics": metrics,
        "warnings": warnings,
        "failures": failures,
        "pass": not failures,
        "quality_gate": {
            "status": "separate",
            "note": "LLM output-quality regression belongs to issue #21 evaluation evidence and is never inferred from serving performance.",
        },
    }
    rendered = json.dumps(report, indent=2, ensure_ascii=False)
    print(rendered)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered + "\n")
    return 0 if not failures else 1


if __name__ == "__main__":
    raise SystemExit(main())
