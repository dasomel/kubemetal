#!/usr/bin/env python3
"""Opt-in, file-only local evidence export/verify. Never runs a collector."""
import argparse
import hashlib
import json
import math
import re
import sys
from datetime import datetime
from pathlib import Path

SCHEMA = Path(__file__).resolve().parents[2] / "docs/schemas/local-accelerator-evidence-v1.schema.json"
UNITS = {
    "gpu": {"gflops": "GFLOP/s", "elapsed_seconds": "s", "matrix_dim": "count", "iterations": "count"},
    "hardware": {"cpu_cores": "count", "gpu_cores": "count", "total_memory_gb": "GiB"},
    "metrics": {"total_memory_gb": "GiB", "used_memory_gb": "GiB",
                "memory_usage_percentage": "%", "cpu_usage_percentage": "%",
                "gpu_usage_percentage": "%", "gpu_memory_used_gb": "GiB",
                "thermal_state": "category"},
    "storage": {"write_fsync.bytes": "byte", "write_fsync.elapsed_seconds": "s",
                "read.bytes": "byte", "read.elapsed_seconds": "s"},
}


def loads(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError(f"duplicate JSON key: {key}")
            result[key] = value
        return result
    value = json.loads(raw, object_pairs_hook=pairs)
    # json.loads accepts NaN and overflowing exponents; neither is evidence.
    json.dumps(value, allow_nan=False)
    return value


def schema_check(value, schema, path="$"):
    """Validate the small JSON Schema vocabulary used by this local format."""
    types = {"object": dict, "array": list, "string": str, "number": (int, float),
             "null": type(None)}
    if "type" in schema:
        names = schema["type"] if isinstance(schema["type"], list) else [schema["type"]]
        if not any(isinstance(value, types[name]) and
                   not (name == "number" and isinstance(value, bool)) for name in names):
            raise ValueError(f"{path}: invalid type")
    if "enum" in schema and value not in schema["enum"]:
        raise ValueError(f"{path}: invalid enum")
    if isinstance(value, dict):
        if set(schema.get("required", [])) - value.keys():
            raise ValueError(f"{path}: missing required fields")
        props = schema.get("properties", {})
        if schema.get("additionalProperties") is False and value.keys() - props.keys():
            raise ValueError(f"{path}: unexpected fields")
        for key, item in value.items():
            if key in props:
                schema_check(item, props[key], f"{path}.{key}")
            elif isinstance(schema.get("additionalProperties"), dict):
                schema_check(item, schema["additionalProperties"], f"{path}.{key}")
    if isinstance(value, list):
        if len(value) < schema.get("minItems", 0):
            raise ValueError(f"{path}: empty array")
        for item in value:
            schema_check(item, schema.get("items", {}), path + "[]")
    if isinstance(value, str):
        if len(value.strip()) < schema.get("minLength", 0):
            raise ValueError(f"{path}: empty string")
        if "pattern" in schema and not re.fullmatch(schema["pattern"], value):
            raise ValueError(f"{path}: invalid pattern")


def timestamp(value):
    if value == "unknown":
        return None
    parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.tzinfo is None:
        raise ValueError("timestamp requires timezone")
    return parsed


def observations(kind, payload):
    result = {}
    for field, unit in UNITS[kind].items():
        value = payload
        for key in field.split("."):
            value = value.get(key) if isinstance(value, dict) else None
        if value is not None:
            if field == "thermal_state":
                if value not in ("nominal", "fair", "serious", "critical"):
                    raise ValueError("invalid thermal_state")
            elif isinstance(value, bool) or not isinstance(value, (float, int)) or not math.isfinite(value) or value < 0:
                raise ValueError(f"invalid measurement: {field}")
        reason = "absent or null in source" if value is None else None
        # D22: storage initializes counters to zero before any I/O. A null
        # phase timer plus zero bytes proves no measurement, not measured zero.
        if kind == "storage" and field.endswith(".bytes") and value == 0:
            phase = payload.get(field.split(".")[0], {})
            if phase.get("elapsed_seconds") is None:
                value, reason = None, "phase has no timing or transferred bytes"
        result[field] = {"value": value, "unit": unit,
                         "status": "unavailable" if value is None else "measured",
                         "reason": reason}
    return result


def validate(envelope):
    loads(json.dumps(envelope, allow_nan=False))
    schema_check(envelope, loads(SCHEMA.read_text()))
    timestamp(envelope["observed_at"])
    for source in envelope["sources"]:
        timestamp(source["observed_at"])
        interval = source["measurement_interval"]
        start, end = timestamp(interval["start"]), timestamp(interval["end"])
        if (start is None) != (end is None) or (start and start > end):
            raise ValueError("invalid measurement interval")
        raw = source["raw_json"]
        if hashlib.sha256(raw.encode("utf-8")).hexdigest() != source["sha256"]:
            raise ValueError("source hash mismatch")
        payload = loads(raw)
        if not isinstance(payload, dict):
            raise ValueError("source must be an object")
        if source["observations"] != observations(source["kind"], payload):
            raise ValueError("observations/units differ from source")
        if source["kind"] == "hardware" and payload.get("brand_name") != envelope["device"]["name"]:
            raise ValueError("device differs from hardware source")
        if source["kind"] == "metrics" and payload.get("gpu_backend") != envelope["device"]["backend"]:
            raise ValueError("backend differs from metrics source")
        limitations = payload.get("limitations", [])
        if source["limitations"] != limitations:
            raise ValueError("source limitations differ")
        if source["kind"] == "gpu" and any(payload.get(k) is None for k in UNITS["gpu"]):
            raise ValueError("incomplete GPU result")
    return envelope


def export(manifest, base):
    if not isinstance(manifest, dict):
        raise ValueError("manifest must be an object")
    if not isinstance(manifest.get("sources"), list):
        raise ValueError("manifest sources must be an array")
    envelope = {key: value for key, value in manifest.items() if key != "sources"}
    envelope["schema_version"] = "kubemetal-local-accelerator-evidence/v1"
    envelope["sources"] = []
    for entry in manifest["sources"]:
        if not isinstance(entry, dict):
            raise ValueError("manifest source must be an object")
        source = {key: value for key, value in entry.items() if key != "path"}
        raw = (base / entry["path"]).read_bytes().decode("utf-8")
        payload = loads(raw)
        if not isinstance(payload, dict):
            raise ValueError("source must be an object")
        source.update(raw_json=raw, sha256=hashlib.sha256(raw.encode("utf-8")).hexdigest(),
                      observations=observations(source["kind"], payload),
                      limitations=payload.get("limitations", []))
        envelope["sources"].append(source)
    return validate(envelope)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("export", "verify"))
    parser.add_argument("file", type=Path, help="manifest for export; envelope for verify")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--manifest", type=Path, help="verify against current original source files")
    args = parser.parse_args(argv)
    try:
        data = loads(args.file.read_text())
        if args.action == "export":
            data = export(data, args.file.parent)
            if args.output is None:
                raise ValueError("export requires --output")
            # D22: exclusive creation costs an explicit new filename on retry;
            # it preserves existing evidence. Remove the output explicitly to retry.
            with args.output.open("x") as output:
                output.write(json.dumps(data, indent=2, allow_nan=False) + "\n")
            validate(loads(args.output.read_text()))
        else:
            validate(data)
            if args.manifest is not None:
                expected = export(loads(args.manifest.read_text()), args.manifest.parent)
                if data != expected:
                    raise ValueError("envelope differs from original sources/manifest")
        print("validated local evidence (integrity only; not authenticated provenance)")
        return 0
    except (ValueError, OSError, KeyError, TypeError) as error:
        print(f"evidence validation failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
