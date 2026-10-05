#!/usr/bin/env python3
"""Offline retrieval evaluation harness for issue #38 (stdlib only).

Measures recall@k and MRR per retrieval mode and per query type over a labeled
query set. Only modes that run without a model download or GPU are measured
(lexical / SQLite FTS5). dense, hybrid and auto need an embedding model plus
LanceDB; they are reported as "not measured", never estimated.

--chunking fixed|heading|both scores the same query set under the production fixed-size
chunker and/or the opt-in heading-aware Markdown chunker (chunking.py).

Relevance is judged at document (filename) level: ranked chunks are collapsed to
their first occurrence per document before scoring.
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from chunking import chunk_markdown_headings  # noqa: E402
from rag_host import chunk_text, lexical_search  # noqa: E402

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
CHUNK_FETCH = 100  # chunks fetched per query before collapsing to documents
NOT_MEASURED = {
    "dense": "requires an embedding model download + LanceDB",
    "hybrid": "requires dense retrieval (RRF fuses lexical + dense)",
    "auto": "resolves natural-language queries to hybrid",
}


CHUNKERS = {"fixed": chunk_text, "heading": chunk_markdown_headings}


def build_chunks(docs_dir, exclude=(), chunking="fixed"):
    """Chunk every top-level *.md; filename is the doc id.

    Chunk ids: fixed `name#0`, heading `name#h0` (distinct namespaces, never colliding).
    """
    chunker, marker = CHUNKERS[chunking], "h" if chunking == "heading" else ""
    chunks = []
    for path in sorted(Path(docs_dir).glob("*.md")):
        if path.name in exclude:
            continue
        for index, text in enumerate(chunker(path.read_text(encoding="utf-8"))):
            chunks.append({"id": f"{path.name}#{marker}{index}", "text": text, "filename": path.name,
                           "source": str(path), "chunk_index": index})
    return chunks


def ranked_docs(results):
    """Collapse ranked chunk results to documents, keeping first (best) occurrence."""
    seen = []
    for item in results:
        if item["filename"] not in seen:
            seen.append(item["filename"])
    return seen


def recall_at_k(ranked, relevant, k):
    return len(set(ranked[:k]) & set(relevant)) / len(relevant)


def reciprocal_rank(ranked, relevant):
    for rank, doc in enumerate(ranked, start=1):
        if doc in relevant:
            return 1.0 / rank
    return 0.0


def evaluate(chunks, queries, search_fn, ks):
    """Return per-query rows: {id, type, ranked, recall: {k: v}, rr}."""
    rows = []
    for query in queries:
        ranked = ranked_docs(search_fn(chunks, query["text"], CHUNK_FETCH))
        rows.append({"id": query["id"], "type": query["type"], "ranked": ranked,
                     "recall": {k: recall_at_k(ranked, query["relevant"], k) for k in ks},
                     "rr": reciprocal_rank(ranked, query["relevant"])})
    return rows


def aggregate(rows, ks):
    """Mean metrics overall and per query type."""
    groups = {"ALL": rows}
    for row in rows:
        groups.setdefault(row["type"], []).append(row)
    return {name: {"n": len(items),
                   **{f"recall@{k}": sum(r["recall"][k] for r in items) / len(items) for k in ks},
                   "mrr": sum(r["rr"] for r in items) / len(items)}
            for name, items in groups.items()}


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--docs-dir", default=str(REPO / "docs"))
    parser.add_argument("--queries", default=str(HERE / "eval" / "queries.json"))
    parser.add_argument("--ks", default="1,3,5")
    parser.add_argument("--exclude", default="mistakes-log.md", help="comma-separated filenames")
    parser.add_argument("--chunking", choices=("fixed", "heading", "both"), default="fixed")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    ks = [int(k) for k in args.ks.split(",")]
    queries = json.loads(Path(args.queries).read_text(encoding="utf-8"))["queries"]
    exclude = set(filter(None, args.exclude.split(",")))
    modes = ["fixed", "heading"] if args.chunking == "both" else [args.chunking]
    runs = {}
    for mode in modes:
        chunks = build_chunks(args.docs_dir, exclude, mode)
        rows = evaluate(chunks, queries, lexical_search, ks)
        runs[mode] = {"chunks": len(chunks), "rows": rows, "table": aggregate(rows, ks)}

    if args.json:
        print(json.dumps({"queries": len(queries), "chunking": {
            mode: {"corpus_chunks": run["chunks"], "measured": {"lexical": run["table"]}}
            for mode, run in runs.items()}, "not_measured": NOT_MEASURED}, indent=2))
        return
    counts = ", ".join(f"{mode}: {run['chunks']} chunks" for mode, run in runs.items())
    print(f"corpus: {args.docs_dir} ({counts}), {len(queries)} queries")
    header = ["mode", "type", "n"] + [f"recall@{k}" for k in ks] + ["mrr"]
    print("  ".join(f"{h:<17}" if i < 2 else f"{h:>9}" for i, h in enumerate(header)))
    for mode, run in runs.items():
        for name, m in run["table"].items():
            cells = [f"{'lexical/' + mode:<17}", f"{name:<17}", f"{m['n']:>9}"]
            cells += [f"{m[f'recall@{k}']:>9.3f}" for k in ks] + [f"{m['mrr']:>9.3f}"]
            print("  ".join(cells))
    for mode, reason in NOT_MEASURED.items():
        print(f"{mode:<17}  not measured ({reason})")
    for mode, run in runs.items():
        for row in run["rows"]:
            print(f"  [{mode}] {row['id']} rr={row['rr']:.3f} top3={row['ranked'][:3]}")


if __name__ == "__main__":
    main()
