#!/usr/bin/env python3
"""
KubeMetal Local RAG & DVC Helper Script (Phase 4c)
Handles document chunking, embedding generation, LanceDB indexing/querying,
and DVC dataset versioning with SeaweedFS S3 remote.
"""

import argparse
import json
import os
import re
import shutil
import sys
import subprocess
import sqlite3
import tempfile
from pathlib import Path

RRF_K = 60
HYBRID_CANDIDATE_COUNT = 50

K8S_RESOURCE_NAMES = {
    "pod", "pods", "service", "services", "svc", "deployment", "deployments",
    "deploy", "daemonset", "daemonsets", "ds", "statefulset", "statefulsets",
    "sts", "configmap", "configmaps", "cm", "secret", "secrets", "ingress",
    "ingresses", "ing", "namespace", "namespaces", "ns", "node", "nodes",
    "no", "persistentvolume", "persistentvolumes", "pv", "persistentvolumeclaim",
    "persistentvolumeclaims", "pvc", "serviceaccount", "serviceaccounts", "sa",
    "crd", "crds", "customresourcedefinition", "customresourcedefinitions",
    "job", "jobs", "cronjob", "cronjobs", "cj", "networkpolicy", "networkpolicies",
    "netpol", "endpointslice", "endpointslices", "endpoints", "ep",
    "k8s", "kubectl", "kubelet", "k3s", "externalname",
}

_VERSION_RE = re.compile(
    r"\b(v\d+(?:alpha\d+|beta\d+)?|\d+\.\d+(?:\.\d+)*[a-zA-Z0-9_\-]*|[0-9]+-?bit|bf16|fp16)\b",
    re.IGNORECASE,
)
_CAMEL_CASE_RE = re.compile(r"\b[a-zA-Z]*[a-z][A-Z][a-zA-Z0-9]*\b")
# Only double quotes (anywhere) or a whole query wrapped in paired single quotes count as a
# quoted phrase; an apostrophe inside "don't ... it's" is just an apostrophe.
_QUOTED_RE = re.compile(r'"[^"]+"|\'[^\']+\'')
# Sentence punctuation and wrappers at a token's edge are prose, not identifier symbols.
_EDGE_PUNCT = "?!:;.,()[]{}\"'`"
_QUESTION_LEADS = {
    "what", "why", "how", "when", "where", "which", "who", "whom", "whose",
    "is", "are", "does", "do", "did", "can", "should", "explain", "describe",
}


def classify_query(query: str) -> tuple[str, str]:
    """Deterministic, local, explainable rule classifier for RAG auto mode.

    Maps identifier-like queries to 'lexical' and natural-language / semantic
    queries to 'hybrid'. Returns (resolved_mode, rule_name).
    """
    if not query or not query.strip():
        return ("lexical", "empty_query")

    trimmed = query.strip()

    # 1. Quoted phrases -> exact lexical match
    if _QUOTED_RE.fullmatch(trimmed) or ('"' in trimmed and _QUOTED_RE.search(trimmed)):
        return ("lexical", "quoted_phrase")

    # 2. Path separators (/ or \\) -> filepath or URI identifier
    if "/" in trimmed or "\\" in trimmed:
        return ("lexical", "path_separator")

    raw_tokens = trimmed.split()

    # 3. Multi-word natural-language / semantic queries (> 3 tokens) without path/quotes
    if len(raw_tokens) > 3:
        return ("hybrid", "natural_language")

    # Edge-stripped tokens: "Error:" / "배포?" / "refused." lose their sentence punctuation.
    stripped = [t.strip(_EDGE_PUNCT) for t in raw_tokens]

    # 4. Version strings (v1.2.3, 0.34.0, v2, 4bit, bf16)
    if _VERSION_RE.search(trimmed):
        return ("lexical", "version_string")

    # 5. Kubernetes resource names / kinds (for short 1-3 token queries)
    if any(w.lower() in K8S_RESOURCE_NAMES for w in stripped if w):
        return ("lexical", "k8s_resource")

    # 6. A leading interrogative is a question even when it names an id ("what is D26").
    if stripped and stripped[0].lower() in _QUESTION_LEADS:
        return ("hybrid", "natural_language")

    # 7. Identifier symbols or casing: underscore, in-word dot, or camelCase/PascalCase
    if any("_" in t or "." in t or _CAMEL_CASE_RE.search(t) for t in stripped):
        return ("lexical", "symbol_or_casing")

    # 8. Short 1-3 token queries with a digit or a symbol inside a token (D26, k=60,
    # mlflow-deployment, @theme). Apostrophes are prose; edge punctuation was stripped above.
    if any(c.isdigit() or not (c.isalpha() or c == "'" or c == "\u2019") for t in stripped for c in t):
        return ("lexical", "short_token_with_digit_or_symbol")

    # 9. Otherwise: 1-3 token semantic query without symbols/identifiers
    return ("hybrid", "natural_language")


def lancedb_table_names(list_tables_result):
    """Normalize `LanceDBConnection.list_tables()` across API shapes.

    Older lancedb returned a plain list[str]. The installed 0.34.0 returns a
    pydantic `ListTablesResponse` with a `.tables` list[str] attribute; `in`
    against the response object itself is always False (it has no matching
    `__contains__`), so every query silently reported "collection not found"
    regardless of what was actually indexed (measured on this Mac). Prefer
    `.tables` when present so this keeps working across either shape instead
    of assuming one.
    """
    tables = getattr(list_tables_result, "tables", list_tables_result)
    return list(tables)

def ensure_fts5(connection_factory=sqlite3.connect):
    """Fail explicitly when this Python build lacks SQLite FTS5."""
    try:
        connection = connection_factory(":memory:")
        connection.execute("CREATE VIRTUAL TABLE fts_probe USING fts5(text)")
        connection.close()
    except sqlite3.Error as exc:
        raise RuntimeError(f"SQLite FTS5 is unavailable: {exc}") from exc

def lexical_search(chunks, query, top_k, connection_factory=sqlite3.connect):
    tokens = []
    token = []
    for char in query:
        if char.isalnum():
            token.append(char)
        elif token:
            tokens.append("".join(token))
            token = []
    if token:
        tokens.append("".join(token))
    if not tokens:
        return []

    ensure_fts5(connection_factory)
    with tempfile.TemporaryDirectory(prefix="kubemetal-rag-fts-") as temp_dir:
        # Rebuild from the available first slice; persistent indexing is intentionally deferred.
        connection = connection_factory(str(Path(temp_dir) / "chunks.sqlite3"))
        connection.execute("CREATE VIRTUAL TABLE chunks USING fts5(text, filename, source, tokenize='unicode61')")
        connection.executemany("INSERT INTO chunks(text, filename, source) VALUES (?, ?, ?)",
                               [(item.get("text", ""), item.get("filename", ""), item.get("source", "")) for item in chunks])
        # Quote each token so user input cannot introduce FTS operators or column filters.
        match_query = " OR ".join('"' + value.replace('"', '""') + '"' for value in tokens)
        rows = connection.execute(
            "SELECT rowid, bm25(chunks) FROM chunks WHERE chunks MATCH ? ORDER BY bm25(chunks) LIMIT ?",
            (match_query, top_k),
        ).fetchall()
        results = []
        for rowid, score in rows:
            item = chunks[rowid - 1]
            results.append({"id": item.get("id"), "text": item.get("text", ""), "filename": item.get("filename", ""),
                            "source": item.get("source", ""), "chunk_index": item.get("chunk_index", 0),
                            "score": float(score), "mode": "lexical"})
        connection.close()
        return results

def retrieval_score(result):
    """Return the backend-native score without normalizing incomparable scales."""
    return float(result.get("_distance", result.get("score", 0.0)))

def rrf_fuse(lexical_results, dense_results, top_k, k=RRF_K):
    """Fuse ranks and retain retriever-specific rank and score provenance."""
    fused = {}
    for retriever, results in (("lexical", lexical_results), ("dense", dense_results)):
        for rank, result in enumerate(results, start=1):
            chunk_id = result.get("id")
            if not isinstance(chunk_id, str) or not chunk_id:
                raise RuntimeError("Hybrid retrieval requires every result to have a chunk id.")

            # Ingest ids use the basename only, so same-named files collide; fuse on
            # the chunk's true identity (source path + index) instead.
            key = (result.get("source", ""), result.get("chunk_index", 0), chunk_id)
            entry = fused.setdefault(key, {
                "id": chunk_id,
                "text": result.get("text", ""),
                "filename": result.get("filename", ""),
                "source": result.get("source", ""),
                "chunk_index": result.get("chunk_index", 0),
                "score": 0.0,
                "hits": {},
            })
            # Preserve the first (best) rank if a backend returns a duplicate id.
            if retriever in entry["hits"]:
                continue
            entry["score"] += 1.0 / (k + rank)
            entry["hits"][retriever] = {"rank": rank, "score": retrieval_score(result)}

    ranked = sorted(fused.values(), key=lambda item: (-item["score"], item["source"], item["chunk_index"], item["id"]))
    return [{
        "id": item["id"],
        "text": item["text"],
        "filename": item["filename"],
        "source": item["source"],
        "chunk_index": item["chunk_index"],
        "score": item["score"],
        "mode": "hybrid",
        "provenance": {
            "retrievers": [name for name in ("lexical", "dense") if name in item["hits"]],
            "lexical": item["hits"].get("lexical"),
            "dense": item["hits"].get("dense"),
        },
    } for item in ranked[:top_k]]

def get_dvc_bin() -> str:
    """
    Find dvc binary path in current Python environment or system PATH.
    """
    venv_bin = Path(sys.executable).parent / "dvc"
    if venv_bin.is_file():
        return str(venv_bin)
    found = shutil.which("dvc")
    if found:
        return found
    return "dvc"

def get_embedding_model(model_name: str):
    """
    Load embedding model using sentence-transformers.
    """
    try:
        from sentence_transformers import SentenceTransformer
        return SentenceTransformer(model_name)
    except ImportError:
        raise RuntimeError("sentence-transformers가 설치되지 않았습니다. setup_rag_env를 실행하세요.")

def chunk_text(text: str, chunk_size: int = 500, overlap: int = 50):
    """
    Simple text chunker with character limit and overlap.
    """
    chunks = []
    start = 0
    text_len = len(text)
    while start < text_len:
        end = min(start + chunk_size, text_len)
        chunk = text[start:end].strip()
        if chunk:
            chunks.append(chunk)
        if end >= text_len:
            break
        start += (chunk_size - overlap)
    return chunks

def cmd_index(args):
    docs_dir = Path(args.docs_dir).expanduser().resolve()
    db_path = Path(args.db_path).expanduser().resolve()
    collection = args.collection
    model_name = args.model

    if not docs_dir.exists():
        print(json.dumps({"status": "error", "error": f"문서 경로가 존재하지 않습니다: {docs_dir}"}))
        sys.exit(1)

    try:
        import lancedb
    except ImportError:
        print(json.dumps({"status": "error", "error": "lancedb가 설치되지 않았습니다. setup_rag_env를 실행하세요."}))
        sys.exit(1)

    # Collect files
    supported_exts = {".txt", ".md", ".json", ".csv", ".py", ".rs", ".rst", ".yaml", ".yml"}
    files = []
    if docs_dir.is_file():
        files.append(docs_dir)
    else:
        for root, _, filenames in os.walk(docs_dir):
            for fn in filenames:
                p = Path(root) / fn
                if p.suffix.lower() in supported_exts:
                    files.append(p)

    if not files:
        print(json.dumps({"status": "error", "error": f"인덱싱할 문서를 찾을 수 없습니다: {docs_dir}"}))
        sys.exit(1)

    # Extract text & chunk
    chunks_data = []
    doc_count = 0
    for fpath in files:
        try:
            content = fpath.read_text(encoding="utf-8", errors="ignore")
            file_chunks = chunk_text(content, chunk_size=500, overlap=50)
            if file_chunks:
                doc_count += 1
                for idx, chunk in enumerate(file_chunks):
                    chunks_data.append({
                        "id": f"{fpath.name}_{idx}",
                        "source": str(fpath),
                        "filename": fpath.name,
                        "chunk_index": idx,
                        "text": chunk
                    })
        except Exception:
            continue

    if not chunks_data:
        print(json.dumps({"status": "error", "error": "인덱싱 가능한 텍스트 내용이 없습니다."}))
        sys.exit(1)

    # Embed chunks
    model = get_embedding_model(model_name)
    texts = [item["text"] for item in chunks_data]
    embeddings = model.encode(texts, show_progress_bar=False)

    for item, emb in zip(chunks_data, embeddings):
        item["vector"] = emb.tolist()

    # LanceDB save
    db_path.mkdir(parents=True, exist_ok=True)
    db = lancedb.connect(str(db_path))

    existing_tables = lancedb_table_names(db.list_tables())
    mode = "overwrite" if collection in existing_tables else "create"
    table = db.create_table(collection, data=chunks_data, mode=mode)

    print(json.dumps({
        "status": "ok",
        "collection": collection,
        "indexed_docs": doc_count,
        "total_chunks": len(chunks_data),
        "db_path": str(db_path)
    }))

def cmd_query(args):
    db_path = Path(args.db_path).expanduser().resolve()
    collection = args.collection
    query_str = args.query
    top_k = args.top_k
    model_name = args.model

    if not db_path.exists():
        print(json.dumps({"status": "error", "error": f"LanceDB 경로가 존재하지 않습니다: {db_path}"}))
        sys.exit(1)

    try:
        import lancedb
    except ImportError:
        print(json.dumps({"status": "error", "error": "lancedb가 설치되지 않았습니다."}))
        sys.exit(1)

    db = lancedb.connect(str(db_path))
    existing_tables = lancedb_table_names(db.list_tables())
    if collection not in existing_tables:
        print(json.dumps({"status": "error", "error": f"컬렉션 '{collection}'을 찾을 수 없습니다."}))
        sys.exit(1)

    table = db.open_table(collection)

    if args.mode == "auto":
        resolved_mode, rule_name = classify_query(query_str)
    else:
        resolved_mode = args.mode
        rule_name = "explicit"

    if resolved_mode == "lexical":
        # LanceTable has no .to_list() on the installed lancedb (0.34.0) -
        # measured on this Mac (AttributeError). to_arrow().to_pylist() is
        # the stable route since pyarrow is a hard lancedb dependency.
        search_results = lexical_search(table.to_arrow().to_pylist(), query_str, top_k)
    elif resolved_mode == "dense":
        model = get_embedding_model(model_name)
        query_vector = model.encode(query_str, show_progress_bar=False).tolist()
        search_results = table.search(query_vector).limit(top_k).to_list()
    else:
        chunks = table.to_arrow().to_pylist()
        candidate_count = max(top_k, HYBRID_CANDIDATE_COUNT)
        lexical_results = lexical_search(chunks, query_str, candidate_count)
        # A hybrid query is invalid if dense retrieval cannot run; do not
        # catch this and silently turn the result into lexical-only output.
        model = get_embedding_model(model_name)
        query_vector = model.encode(query_str, show_progress_bar=False).tolist()
        dense_results = table.search(query_vector).limit(candidate_count).to_list()
        search_results = rrf_fuse(lexical_results, dense_results, top_k)

    formatted_results = []
    for r in search_results:
        formatted = {
            "id": r.get("id"),
            "text": r.get("text", ""),
            "filename": r.get("filename", ""),
            "source": r.get("source", ""),
            "chunk_index": r.get("chunk_index", 0),
            "score": retrieval_score(r),
            "mode": r.get("mode", resolved_mode),
            "resolved_mode": resolved_mode,
            "rule": rule_name,
        }
        if "provenance" in r:
            formatted["provenance"] = r["provenance"]
        formatted_results.append(formatted)

    print(json.dumps({
        "status": "ok",
        "query": query_str,
        "resolved_mode": resolved_mode,
        "rule": rule_name,
        "results": formatted_results
    }))

def cmd_dvc_commit(args):
    data_dir = Path(args.data_dir).expanduser().resolve()
    remote_url = args.remote_url
    bucket = args.bucket
    # D13/D21: S3 크리덴셜은 CLI 인자로 받지 않는다(ps 노출 방지) — Rust 스폰 시
    # KUBEMETAL_S3_ACCESS_KEY/KUBEMETAL_S3_SECRET_KEY 환경변수로 주입된다. 기본값은
    # seaweedfs-s3-credentials.yaml의 stringData(SeaweedFS 무인증 모드 더미값)와 동일하게
    # 맞춰 Rust 없이 단독 실행해도 동작한다.
    access_key = os.environ.get("KUBEMETAL_S3_ACCESS_KEY", "kubemetal")
    secret_key = os.environ.get("KUBEMETAL_S3_SECRET_KEY", "kubemetal-local")

    if not data_dir.exists():
        print(json.dumps({"status": "error", "error": f"데이터 경로가 존재하지 않습니다: {data_dir}"}))
        sys.exit(1)

    dvc_bin = get_dvc_bin()
    work_dir = data_dir if data_dir.is_dir() else data_dir.parent

    # Check / init DVC
    dvc_dir = work_dir / ".dvc"
    if not dvc_dir.exists():
        res = subprocess.run([dvc_bin, "init", "--no-scm"], cwd=work_dir, capture_output=True, text=True)
        if res.returncode != 0:
            res = subprocess.run([dvc_bin, "init"], cwd=work_dir, capture_output=True, text=True)

    # Add S3 remote
    remote_name = "seaweedfs"
    s3_uri = f"s3://{bucket}"
    
    subprocess.run([dvc_bin, "remote", "add", "-f", "-d", remote_name, s3_uri], cwd=work_dir, capture_output=True)
    subprocess.run([dvc_bin, "remote", "modify", remote_name, "endpointurl", remote_url], cwd=work_dir, capture_output=True)
    subprocess.run([dvc_bin, "remote", "modify", remote_name, "access_key_id", access_key], cwd=work_dir, capture_output=True)
    subprocess.run([dvc_bin, "remote", "modify", remote_name, "secret_access_key", secret_key], cwd=work_dir, capture_output=True)
    subprocess.run([dvc_bin, "remote", "modify", remote_name, "use_ssl", "false"], cwd=work_dir, capture_output=True)

    target_rel = data_dir.name if data_dir != work_dir else "."
    
    # dvc add
    subprocess.run([dvc_bin, "add", target_rel], cwd=work_dir, capture_output=True, text=True)

    # dvc push
    push_res = subprocess.run([dvc_bin, "push", "-r", remote_name], cwd=work_dir, capture_output=True, text=True)
    if push_res.returncode != 0:
        print(json.dumps({
            "status": "error",
            "error": f"DVC push 실패: {push_res.stderr.strip() or push_res.stdout.strip()}"
        }))
        sys.exit(1)

    print(json.dumps({
        "status": "ok",
        "message": f"DVC push 성공: {target_rel} -> {s3_uri} ({remote_url})",
        "remote": remote_name,
        "s3_uri": s3_uri
    }))

def main():
    parser = argparse.ArgumentParser(description="KubeMetal RAG & DVC CLI")
    subparsers = parser.add_subparsers(dest="command", required=True)

    # index subcommand
    p_index = subparsers.add_parser("index")
    p_index.add_argument("--docs-dir", required=True)
    p_index.add_argument("--db-path", default="~/.kubemetal/lancedb")
    p_index.add_argument("--collection", default="default")
    p_index.add_argument("--model", default="sentence-transformers/all-MiniLM-L6-v2")

    # query subcommand
    p_query = subparsers.add_parser("query")
    p_query.add_argument("--query", required=True)
    p_query.add_argument("--db-path", default="~/.kubemetal/lancedb")
    p_query.add_argument("--collection", default="default")
    p_query.add_argument("--top-k", type=int, default=3)
    p_query.add_argument("--model", default="sentence-transformers/all-MiniLM-L6-v2")
    p_query.add_argument("--mode", choices=("auto", "dense", "lexical", "hybrid"), default="dense")

    # dvc-commit subcommand
    p_dvc = subparsers.add_parser("dvc-commit")
    p_dvc.add_argument("--data-dir", required=True)
    p_dvc.add_argument("--remote-url", default="http://127.0.0.1:8333")
    p_dvc.add_argument("--bucket", default="dvc-repo")
    p_dvc.add_argument("--message", default="Commit dataset")

    args = parser.parse_args()

    if args.command == "index":
        cmd_index(args)
    elif args.command == "query":
        cmd_query(args)
    elif args.command == "dvc-commit":
        cmd_dvc_commit(args)

if __name__ == "__main__":
    main()
