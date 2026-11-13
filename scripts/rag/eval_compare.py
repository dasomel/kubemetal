#!/usr/bin/env python3
"""Opt-in, offline production retrieval comparison (#38). No model downloads."""

import argparse
import contextlib
import hashlib
import importlib.metadata
import io
import json
import os
import platform
import sqlite3
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import rag_host
from eval_retrieval import aggregate, ndcg_at_k, ranked_docs, recall_at_k, reciprocal_rank

MODES = ("exact", "lexical", "dense", "hybrid", "auto")
KS = (1, 3, 5)


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, ensure_ascii=False).encode()).hexdigest()


def file_hash(path):
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def tree_hash(path):
    files = {str(p.relative_to(path)): file_hash(p) for p in sorted(path.rglob("*")) if p.is_file()}
    if not files:
        raise ValueError(f"Empty snapshot: {path}")
    return digest(files)


def fixture_snapshot(root):
    docs = sorted((root / "docs").iterdir())
    if not docs or any(not p.is_file() for p in docs):
        raise ValueError("Fixture requires a flat, nonempty docs directory")
    chunks, corpus = [], {}
    for path in docs:
        content = path.read_text(encoding="utf-8")
        corpus[path.name] = file_hash(path)
        # D-C1: stable fixture-relative sources preserve tie order across machines.
        # Cost: synthetic paths; use a separate harness for real user corpora.
        chunks.extend(rag_host.chunk_records(Path("docs") / path.name, content))
    queries = json.loads((root / "queries.json").read_text(encoding="utf-8"))["queries"]
    ids = [q["id"] for q in queries]
    if not chunks or not ids or len(ids) != len(set(ids)):
        raise ValueError("Empty chunks/queries or duplicate query ids")
    for query in queries:
        if not query["text"] or not query["type"] or not query["relevant"] \
                or len(query["relevant"]) != len(set(query["relevant"])) \
                or not set(query["relevant"]) <= corpus.keys():
            raise ValueError(f"Invalid qrels: {query['id']}")
    return chunks, queries, {"corpus": digest(corpus), "queries": file_hash(root / "queries.json"),
                             "chunks": digest(chunks)}


def invoke(args, model):
    output = io.StringIO()
    started = time.perf_counter()
    with patch.object(rag_host, "get_embedding_model", return_value=model), \
            contextlib.redirect_stdout(output):
        try:
            rag_host.cmd_query(argparse.Namespace(**args))
        except SystemExit as exc:
            raise RuntimeError(f"Production exit {exc.code}: {output.getvalue()}") from exc
    return {"response": json.loads(output.getvalue()),
            "query_ms": (time.perf_counter() - started) * 1000}


def score(response, query, chunks):
    if response.get("status") != "ok":
        raise ValueError(f"Production error: {response}")
    if "resolved_mode" not in response or "rule" not in response:
        raise ValueError("Missing routing metadata")
    results = response["results"]
    known = {c["id"]: c for c in chunks}
    for result in results:
        original = known.get(result.get("id"))
        if original is None or any(result.get(key) != original[key]
                                   for key in ("filename", "source", "text", "chunk_index")):
            raise ValueError("Missing or mismatched chunk metadata")
    ranked = ranked_docs(results)
    return {"id": query["id"], "type": query["type"], "ranked": ranked,
            "recall": {k: recall_at_k(ranked, query["relevant"], k) for k in KS},
            "ndcg": {k: ndcg_at_k(ranked, query["relevant"], k) for k in KS},
            "rr": reciprocal_rank(ranked, query["relevant"])}


def summarize(raw, queries, chunks):
    """Recompute metrics only from saved production responses, not stored scores."""
    lookup = {q["id"]: q for q in queries}
    tables = {}
    for mode in MODES:
        rows = []
        for item in raw:
            if item["mode"] == mode and item["status"] == "ok":
                row = score(item["response"], lookup[item["query_id"]], chunks)
                row["latency_ms"] = item["query_ms"]
                rows.append(row)
        tables[mode] = aggregate(rows, KS) if rows else {"ALL": {"n": 0}}
    return tables


def measure(chunks, queries, repeats, args, model, cold_runner):
    raw = []
    for repeat in range(repeats):
        for qi, query in enumerate(queries):
            # D-C2: rotate all five modes across repetitions to distribute order bias.
            # Cost: no randomization; change the schedule explicitly for larger studies.
            offset = (repeat + qi) % len(MODES)
            for mode in MODES[offset:] + MODES[:offset]:
                request = dict(args, query=query["text"], mode=mode)
                item = {"repeat": repeat, "query_id": query["id"], "mode": mode}
                started = time.perf_counter()
                try:
                    item.update(invoke(request, model))
                    expected_mode, expected_rule = (rag_host.classify_query(query["text"])
                                                    if mode == "auto" else (mode, "explicit"))
                    if (item["response"].get("resolved_mode"), item["response"].get("rule")) != (expected_mode, expected_rule):
                        raise ValueError("Production routing metadata differs from requested mode")
                    item["metrics"] = score(item["response"], query, chunks)
                    item["subprocess"] = cold_runner(request)
                    cold_response = item["subprocess"]["response"]
                    if (cold_response.get("resolved_mode"), cold_response.get("rule")) != (expected_mode, expected_rule):
                        raise ValueError("Subprocess routing metadata differs from requested mode")
                    score(cold_response, query, chunks)
                    item["status"] = "ok"
                except (Exception, SystemExit) as exc:
                    item.update(status="error", error=f"{type(exc).__name__}: {exc}")
                item["request_wall_ms"] = (time.perf_counter() - started) * 1000
                raw.append(item)
    return {"status": "ok" if all(r["status"] == "ok" for r in raw) else "error",
            "raw": raw, "report": summarize(raw, queries, chunks)}


def offline():
    os.environ.update(HF_HUB_OFFLINE="1", TRANSFORMERS_OFFLINE="1", HF_HUB_DISABLE_TELEMETRY="1")


def load_model(path):
    from sentence_transformers import SentenceTransformer
    return SentenceTransformer(str(path), device="cpu", local_files_only=True, trust_remote_code=False)


def cold_query(request):
    started = time.perf_counter()
    process = subprocess.run([sys.executable, "-B", str(Path(__file__).resolve()),
                              "--worker", json.dumps(request)], capture_output=True, text=True)
    wall_ms = (time.perf_counter() - started) * 1000
    if process.returncode:
        raise RuntimeError(f"Subprocess exit {process.returncode}: {process.stderr} {process.stdout}")
    result = json.loads(process.stdout)
    result.update(wall_ms=wall_ms, stderr=process.stderr)
    return result


def worker(request):
    started = time.perf_counter()
    resolved = rag_host.classify_query(request["query"])[0] if request["mode"] == "auto" else request["mode"]
    model = load_model(request["model"]) if resolved in ("dense", "hybrid") else None
    load_ms = (time.perf_counter() - started) * 1000
    result = invoke(request, model)
    result["model_load_ms"] = load_ms
    print(json.dumps(result))


def run(args):
    config = {"modes": MODES, "repeats": args.repeats, "ks": KS, "top_k": None,
              "chunking": "fixed", "device": "cpu", "rrf_k": rag_host.RRF_K,
              "hybrid_candidate_count": rag_host.HYBRID_CANDIDATE_COUNT}
    evidence = {"schema_version": 1, "status": "unavailable", "config": config, "raw": []}
    try:
        started = time.perf_counter()
        chunks, queries, hashes = fixture_snapshot(args.fixture)
        evidence["fixture_prepare_ms"] = (time.perf_counter() - started) * 1000
        config["top_k"] = len(chunks)
        evidence.update(hashes=hashes, queries=queries, chunks=chunks)
        model_path = args.model_path.expanduser().resolve(strict=True)
        if not model_path.is_dir():
            raise ValueError("--model-path must be an existing local snapshot directory")
        hashes["model"] = tree_hash(model_path)
        hashes["config"] = digest(config)
        hashes["production"] = file_hash(Path(rag_host.__file__))
        hashes["harness"] = file_hash(Path(__file__))
        hashes["metrics"] = file_hash(Path(__file__).with_name("eval_retrieval.py"))
        evidence["model_path"] = str(model_path)
        evidence["versions"] = {"python": platform.python_version(), "platform": platform.platform(),
                                "sqlite": sqlite3.sqlite_version,
                                **{name: importlib.metadata.version(name) for name in
                                   ("lancedb", "sentence-transformers", "torch", "numpy", "pyarrow")}}
        import lancedb
        started = time.perf_counter()
        model = load_model(model_path)
        evidence["model_load_ms"] = (time.perf_counter() - started) * 1000
        with tempfile.TemporaryDirectory(prefix="kubemetal-compare-") as db_path:
            started = time.perf_counter()
            vectors = model.encode([c["text"] for c in chunks], show_progress_bar=False)
            records = [dict(c, vector=v.tolist()) for c, v in zip(chunks, vectors)]
            if len(records) != len(chunks):
                raise ValueError("Embedding count differs from chunk count")
            db = lancedb.connect(db_path)
            table = db.create_table("fixture", data=records)
            # Read the real table, not the input list: every mode must see the same corpus.
            if sorted(table.to_arrow().to_pylist(), key=lambda c: c["id"]) != sorted(records, key=lambda c: c["id"]):
                raise ValueError("Indexed snapshot differs from fixture")
            evidence["index_prepare_ms"] = (time.perf_counter() - started) * 1000
            evidence["hashes"]["index_records"] = digest(records)
            evidence["index_version"] = table.version
            request = {"db_path": db_path, "collection": "fixture", "model": str(model_path),
                       "top_k": len(chunks)}
            evidence.update(measure(chunks, queries, args.repeats, request, model, cold_query))
            if table.version != evidence["index_version"]:
                raise ValueError("Index version changed during measurement")
            if tree_hash(model_path) != hashes["model"] or fixture_snapshot(args.fixture)[2] != {
                    k: hashes[k] for k in ("corpus", "queries", "chunks")}:
                raise ValueError("Fixture or model changed during measurement")
    except (Exception, SystemExit) as exc:
        evidence.update(status="unavailable", error=f"{type(exc).__name__}: {exc}")
    args.output.write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return 0 if evidence["status"] == "ok" else 1


def main():
    offline()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", type=Path)
    parser.add_argument("--model-path", type=Path)
    parser.add_argument("--repeats", type=int, default=5)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--worker", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.worker:
        worker(json.loads(args.worker))
        return 0
    if not args.fixture or not args.model_path or not args.output or args.repeats < 1:
        parser.error("--fixture, --model-path, --output and positive --repeats are required")
    return run(args)


if __name__ == "__main__":
    sys.exit(main())
