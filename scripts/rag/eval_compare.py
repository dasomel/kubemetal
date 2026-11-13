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


def invoke(args, model=None, loader=None):
    output = io.StringIO()
    route = {"resolved_mode": args.get("mode"), "rule": "explicit"}
    classify = rag_host.classify_query

    def observe(query):
        resolved, rule = classify(query)
        route.update(resolved_mode=resolved, rule=rule)
        return resolved, rule

    if args.get("mode") == "auto":
        route = {}  # Unknown until production actually classifies the query.
    started = time.perf_counter()
    with patch.object(rag_host, "get_embedding_model", side_effect=loader or (lambda name: model)), \
            patch.object(rag_host, "classify_query", side_effect=observe), \
            contextlib.redirect_stdout(output):
        try:
            rag_host.cmd_query(argparse.Namespace(**args))
            response = json.loads(output.getvalue())
        except (Exception, SystemExit) as exc:
            response = {"status": "error", **route,
                        "error": f"{type(exc).__name__}: {exc}",
                        "production_output": output.getvalue()}
    return {"response": response, "query_ms": (time.perf_counter() - started) * 1000}


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
                    item["subprocess"] = cold_runner(request)
                    warm, cold = item["response"], item["subprocess"]["response"]
                    # D-C3: production owns auto routing; preserve both observations.
                    # Cost: failed pairs have no aggregate score; rerun after fixing the cause.
                    errors = [f"{name}: {response.get('error', 'production failed')}"
                              for name, response in (("resident", warm), ("subprocess", cold))
                              if response.get("status") != "ok"]
                    if errors:
                        raise ValueError("; ".join(errors))
                    if mode != "auto" and any(
                            (response.get("resolved_mode"), response.get("rule")) != (mode, "explicit")
                            for response in (warm, cold)):
                        raise ValueError("Production routing metadata differs from requested mode")
                    if warm != cold:
                        raise ValueError(f"Production response divergence: resident route "
                                         f"{warm.get('resolved_mode')}/{warm.get('rule')}; subprocess route "
                                         f"{cold.get('resolved_mode')}/{cold.get('rule')}")
                    item["metrics"] = score(warm, query, chunks)
                    score(cold, query, chunks)
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
    try:
        result = json.loads(process.stdout)
    except ValueError as exc:
        raise RuntimeError(f"Subprocess exit {process.returncode}: {process.stderr} {process.stdout}") from exc
    if process.returncode and result.get("response", {}).get("status") != "error":
        raise RuntimeError(f"Subprocess exit {process.returncode}: {process.stderr} {process.stdout}")
    result.update(wall_ms=wall_ms, stderr=process.stderr, exit_code=process.returncode)
    return result


def worker(request):
    load_ms = 0

    def loader(path):
        nonlocal load_ms
        started = time.perf_counter()
        try:
            return load_model(path)
        finally:
            load_ms += (time.perf_counter() - started) * 1000

    result = invoke(request, loader=loader)
    result["model_load_ms"] = load_ms
    result["query_ms"] -= load_ms
    print(json.dumps(result))
    return 0 if result["response"].get("status") == "ok" else 1


def evaluate(args):
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
    return evidence


def run(args):
    temporary = None
    try:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        # D-C4: probe the destination before fixture/model work and publish atomically.
        # Cost: a sibling temporary file; remove it on failure and retain prior evidence.
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=args.output.parent,
                                         prefix=f".{args.output.name}.", delete=False) as stream:
            temporary = Path(stream.name)
            if args.output.exists() and not args.output.is_file():
                raise ValueError(f"Output is not a file: {args.output}")
            evidence = evaluate(args)
            json.dump(evidence, stream, ensure_ascii=False, indent=2)
            stream.write("\n")
        os.replace(temporary, args.output)
        if evidence["status"] != "ok":
            print(f"Comparison {evidence['status']}: {evidence.get('error', 'see per-query errors in output')}",
                  file=sys.stderr)
        return 0 if evidence["status"] == "ok" else 1
    except (Exception, SystemExit) as exc:
        print(f"Cannot write comparison output {args.output}: {type(exc).__name__}: {exc}", file=sys.stderr)
        return 1
    finally:
        if temporary is not None:
            try:
                temporary.unlink(missing_ok=True)
            except OSError as exc:
                print(f"Cannot remove temporary output {temporary}: {exc}", file=sys.stderr)


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
        try:
            return worker(json.loads(args.worker))
        except (Exception, SystemExit) as exc:
            print(f"Comparison worker failed: {type(exc).__name__}: {exc}", file=sys.stderr)
            return 1
    if not args.fixture or not args.model_path or not args.output or args.repeats < 1:
        parser.error("--fixture, --model-path, --output and positive --repeats are required")
    return run(args)


if __name__ == "__main__":
    sys.exit(main())
