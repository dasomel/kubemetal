#!/usr/bin/env python3
"""
KubeMetal Local RAG & DVC Helper Script (Phase 4c)
Handles document chunking, embedding generation, LanceDB indexing/querying,
and DVC dataset versioning with SeaweedFS S3 remote.
"""

import argparse
import json
import os
import sys
import subprocess
from pathlib import Path

def get_embedding_model(model_name: str):
    """
    Load embedding model using sentence-transformers or mlx-embeddings fallback.
    """
    try:
        from sentence_transformers import SentenceTransformer
        return SentenceTransformer(model_name)
    except ImportError:
        try:
            # Fallback check if sentence_transformers isn't installed
            raise RuntimeError("sentence-transformers가 설치되지 않았습니다. setup_rag_env를 실행하세요.")
        except Exception as e:
            raise RuntimeError(f"임베딩 모델 로드 실패: {e}")

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
        except Exception as e:
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

    # Create table (overwrite mode if exists)
    table = db.create_table(collection, data=chunks_data, mode="overwrite")

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
    if collection not in db.table_names():
        print(json.dumps({"status": "error", "error": f"컬렉션 '{collection}'을 찾을 수 없습니다."}))
        sys.exit(1)

    table = db.open_table(collection)

    model = get_embedding_model(model_name)
    query_vector = model.encode(query_str, show_progress_bar=False).tolist()

    search_results = table.search(query_vector).limit(top_k).to_list()

    formatted_results = []
    for r in search_results:
        # Distance / Similarity score handling
        score = r.get("_distance", 0.0)
        # Convert L2 distance or cosine distance to rough similarity if possible, or pass distance
        formatted_results.append({
            "text": r.get("text", ""),
            "filename": r.get("filename", ""),
            "source": r.get("source", ""),
            "chunk_index": r.get("chunk_index", 0),
            "score": float(score)
        })

    print(json.dumps({
        "status": "ok",
        "query": query_str,
        "results": formatted_results
    }))

def cmd_dvc_commit(args):
    data_dir = Path(args.data_dir).expanduser().resolve()
    remote_url = args.remote_url
    bucket = args.bucket
    access_key = args.access_key
    secret_key = args.secret_key

    if not data_dir.exists():
        print(json.dumps({"status": "error", "error": f"데이터 경로가 존재하지 않습니다: {data_dir}"}))
        sys.exit(1)

    work_dir = data_dir if data_dir.is_dir() else data_dir.parent

    # Check / init DVC
    dvc_dir = work_dir / ".dvc"
    if not dvc_dir.exists():
        res = subprocess.run(["dvc", "init", "--no-scm"], cwd=work_dir, capture_output=True, text=True)
        if res.returncode != 0:
            # Fallback try without --no-scm if git repo or general init
            res = subprocess.run(["dvc", "init"], cwd=work_dir, capture_output=True, text=True)

    # Add S3 remote
    remote_name = "seaweedfs"
    s3_uri = f"s3://{bucket}"
    
    subprocess.run(["dvc", "remote", "add", "-f", "-d", remote_name, s3_uri], cwd=work_dir, capture_output=True)
    subprocess.run(["dvc", "remote", "modify", remote_name, "endpointurl", remote_url], cwd=work_dir, capture_output=True)
    subprocess.run(["dvc", "remote", "modify", remote_name, "access_key_id", access_key], cwd=work_dir, capture_output=True)
    subprocess.run(["dvc", "remote", "modify", remote_name, "secret_access_key", secret_key], cwd=work_dir, capture_output=True)
    subprocess.run(["dvc", "remote", "modify", remote_name, "use_ssl", "false"], cwd=work_dir, capture_output=True)

    target_rel = data_dir.name if data_dir != work_dir else "."
    
    # dvc add
    add_res = subprocess.run(["dvc", "add", target_rel], cwd=work_dir, capture_output=True, text=True)
    if add_res.returncode != 0 and "is already tracked" not in add_res.stderr:
        # If failure, report error
        pass

    # dvc push
    push_res = subprocess.run(["dvc", "push", "-r", remote_name], cwd=work_dir, capture_output=True, text=True)
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

    # dvc-commit subcommand
    p_dvc = subparsers.add_parser("dvc-commit")
    p_dvc.add_argument("--data-dir", required=True)
    p_dvc.add_argument("--remote-url", default="http://127.0.0.1:8333")
    p_dvc.add_argument("--bucket", default="dvc-repo")
    p_dvc.add_argument("--access-key", default="seaweedfsadmin")
    p_dvc.add_argument("--secret-key", default="seaweedfsadmin")
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
