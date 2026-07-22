#!/usr/bin/env python3
"""
KubeMetal Data Ingestion Pipeline Script (Phase 5a)
Supports data sources: Web URL / RSS, HuggingFace dataset, Local folder/files.
Pipeline DAG Nodes:
 1. extract: Fetch and parse raw documents from source.
 2. clean_chunk: Normalize text and generate character-overlapping chunks.
 3. lancedb_index: Generate vector embeddings and store in LanceDB vector store.
 4. dvc_backup: (Optional) Commit dataset and push to SeaweedFS S3 remote via DVC.

Returns detailed DAG node execution states (status, duration, items processed).
"""

import argparse
import html
import json
import os
import re
import shutil
import sys
import time
import urllib.request
import urllib.parse
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Dict, List, Any, Tuple


def get_dvc_bin() -> str:
    """Find dvc binary path in current Python environment or system PATH."""
    venv_bin = Path(sys.executable).parent / "dvc"
    if venv_bin.is_file():
        return str(venv_bin)
    found = shutil.which("dvc")
    if found:
        return found
    return "dvc"


import ssl


def get_ssl_context():
    """Create SSL context with unverified fallback for macOS local python envs."""
    try:
        return ssl.create_default_context()
    except Exception:
        ctx = ssl.create_default_context()
        ctx.check_hostname = False
        ctx.verify_mode = ssl.CERT_NONE
        return ctx


def fetch_url_bytes(url: str, headers: dict = None, timeout: int = 15) -> bytes:
    """Fetch URL bytes with SSL fallback."""
    req_headers = {"User-Agent": "Mozilla/5.0 (KubeMetal-DataIngest/1.0)"}
    if headers:
        req_headers.update(headers)
    req = urllib.request.Request(url, headers=req_headers)
    
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return resp.read()
    except Exception:
        # Fallback to unverified SSL context
        ctx = ssl._create_unverified_context()
        with urllib.request.urlopen(req, timeout=timeout, context=ctx) as resp:
            return resp.read()


def clean_html_text(raw_html: str) -> str:
    """Remove scripts, styles, and HTML tags from raw HTML text."""
    cleaned = re.sub(r"<(script|style)[^>]*>.*?</\1>", "", raw_html, flags=re.DOTALL | re.IGNORECASE)
    cleaned = re.sub(r"<[^>]+>", " ", cleaned)
    cleaned = html.unescape(cleaned)
    cleaned = re.sub(r"\s+", " ", cleaned).strip()
    return cleaned


def extract_web_or_rss(source_path: str) -> List[Dict[str, str]]:
    """Extract content from a Web URL or RSS feed."""
    raw_bytes = fetch_url_bytes(source_path, timeout=15)
    body = raw_bytes.decode("utf-8", errors="ignore")

    documents = []

    # Check if RSS / XML
    is_xml = "xml" in source_path.lower() or "rss" in source_path.lower() or body.lstrip().startswith("<?xml") or "<rss" in body or "<feed" in body
    if is_xml:
        try:
            root = ET.fromstring(body)
            # Standard RSS channel -> item
            items = root.findall(".//item")
            if not items:
                # Atom feed -> entry
                items = root.findall(".//{http://www.w3.org/2005/Atom}entry")
            
            for idx, item in enumerate(items):
                title_elem = item.find("title") or item.find("{http://www.w3.org/2005/Atom}title")
                desc_elem = item.find("description") or item.find("content") or item.find("{http://www.w3.org/2005/Atom}content")
                title = title_elem.text if title_elem is not None and title_elem.text else f"Item {idx+1}"
                raw_desc = desc_elem.text if desc_elem is not None and desc_elem.text else ""
                clean_desc = clean_html_text(raw_desc)
                text = f"Title: {title}\n\n{clean_desc}".strip()
                if text:
                    documents.append({
                        "source": source_path,
                        "filename": f"rss_item_{idx+1}.txt",
                        "text": text
                    })
        except Exception as e:
            # Fallback to web page parsing if XML parsing fails
            pass

    if not documents:
        text = clean_html_text(body)
        if text:
            filename = urllib.parse.urlparse(source_path).path.split("/")[-1] or "webpage.txt"
            if not filename.endswith(".txt"):
                filename += ".txt"
            documents.append({
                "source": source_path,
                "filename": filename,
                "text": text
            })

    return documents


def extract_hf_dataset(dataset_name: str) -> List[Dict[str, str]]:
    """Extract sample rows from a HuggingFace dataset."""
    documents = []

    # Try datasets library
    try:
        from datasets import load_dataset
        ds = load_dataset(dataset_name, split="train", streaming=True)
        count = 0
        for idx, row in enumerate(ds):
            if count >= 200:
                break
            # Find text content in row
            text = ""
            if isinstance(row, dict):
                for k in ["text", "content", "article", "document", "sentence", "prompt", "instruction"]:
                    if k in row and isinstance(row[k], str) and row[k].strip():
                        text = row[k].strip()
                        break
                if not text:
                    text = json.dumps(row, ensure_ascii=False)
            elif isinstance(row, str):
                text = row

            if text:
                documents.append({
                    "source": f"hf://{dataset_name}",
                    "filename": f"hf_doc_{idx+1}.txt",
                    "text": text
                })
                count += 1
        return documents
    except Exception:
        pass

    # Fallback to HuggingFace datasets-server REST API
    encoded_name = urllib.parse.quote(dataset_name, safe="")
    
    config_name = "default"
    split_name = "train"
    
    # Query dataset splits first to get accurate config & split name
    try:
        splits_url = f"https://datasets-server.huggingface.co/splits?dataset={encoded_name}"
        raw_bytes = fetch_url_bytes(splits_url, timeout=10)
        splits_info = json.loads(raw_bytes.decode("utf-8"))
        if "splits" in splits_info and len(splits_info["splits"]) > 0:
            first_split = splits_info["splits"][0]
            config_name = first_split.get("config", config_name)
            split_name = first_split.get("split", split_name)
    except Exception:
        pass

    api_url = f"https://datasets-server.huggingface.co/rows?dataset={encoded_name}&config={urllib.parse.quote(config_name)}&split={urllib.parse.quote(split_name)}&limit=100"
    try:
        raw_bytes = fetch_url_bytes(api_url, timeout=15)
        data = json.loads(raw_bytes.decode("utf-8"))
        for idx, r in enumerate(data.get("rows", [])):
            row_data = r.get("row", {})
            text = ""
            for k in ["text", "content", "article", "document", "sentence", "prompt", "instruction", "description"]:
                if k in row_data and isinstance(row_data[k], str) and row_data[k].strip():
                    text = row_data[k].strip()
                    break
            if not text:
                text = json.dumps(row_data, ensure_ascii=False)
            if text:
                documents.append({
                    "source": f"hf://{dataset_name}",
                    "filename": f"hf_doc_{idx+1}.txt",
                    "text": text
                })
    except Exception as e:
        # Final fallback: fetch dataset README metadata from HF Hub if rows endpoint fails
        try:
            readme_url = f"https://huggingface.co/datasets/{dataset_name}/raw/main/README.md"
            readme_text = fetch_url_bytes(readme_url, timeout=10).decode("utf-8", errors="ignore")
            if readme_text.strip():
                documents.append({
                    "source": f"hf://{dataset_name}",
                    "filename": "dataset_readme.txt",
                    "text": readme_text.strip()
                })
                return documents
        except Exception:
            pass
        raise RuntimeError(f"HuggingFace dataset '{dataset_name}' 로드 실패: {e}")

    return documents


def extract_local_folder(source_path: str) -> List[Dict[str, str]]:
    """Extract text from local folder or file."""
    p = Path(source_path).expanduser().resolve()
    if not p.exists():
        raise RuntimeError(f"로컬 경로가 존재하지 않습니다: {source_path}")

    supported_exts = {".txt", ".md", ".json", ".csv", ".py", ".rs", ".rst", ".yaml", ".yml", ".pdf"}
    files = []
    if p.is_file():
        files.append(p)
    else:
        for root, _, filenames in os.walk(p):
            for fn in filenames:
                fp = Path(root) / fn
                if fp.suffix.lower() in supported_exts:
                    files.append(fp)

    if not files:
        raise RuntimeError(f"인덱싱할 문서를 찾을 수 없습니다: {source_path}")

    documents = []
    for fp in files:
        try:
            if fp.suffix.lower() == ".pdf":
                # Try pypdf / PyPDF2
                text = ""
                try:
                    import pypdf
                    reader = pypdf.PdfReader(str(fp))
                    text = "\n".join([page.extract_text() or "" for page in reader.pages])
                except ImportError:
                    try:
                        import PyPDF2
                        reader = PyPDF2.PdfReader(str(fp))
                        text = "\n".join([page.extract_text() or "" for page in reader.pages])
                    except ImportError:
                        text = fp.read_text(encoding="utf-8", errors="ignore")
            else:
                text = fp.read_text(encoding="utf-8", errors="ignore")

            text = text.strip()
            if text:
                documents.append({
                    "source": str(fp),
                    "filename": fp.name,
                    "text": text
                })
        except Exception:
            continue

    return documents


def chunk_text(text: str, chunk_size: int = 500, overlap: int = 50) -> List[str]:
    """Chunk text into character-based overlapping segments."""
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


def main():
    parser = argparse.ArgumentParser(description="KubeMetal Data Ingestion Pipeline")
    parser.add_argument("--source-type", choices=["web", "rss", "hf", "huggingface", "local"], required=True, help="Data source type")
    parser.add_argument("--source-path", required=True, help="URL, HuggingFace dataset name, or local directory path")
    parser.add_argument("--collection", default="dataset_ingest", help="LanceDB collection/table name")
    parser.add_argument("--db-path", default="~/.kubemetal/lancedb", help="LanceDB root directory")
    parser.add_argument("--embedding-model", default="sentence-transformers/all-MiniLM-L6-v2", help="Embedding model name")
    parser.add_argument("--chunk-size", type=int, default=500, help="Chunk size in characters")
    parser.add_argument("--chunk-overlap", type=int, default=50, help="Chunk overlap in characters")
    parser.add_argument("--dvc-backup", action="store_true", help="Perform DVC backup & push to S3")
    parser.add_argument("--remote-url", default="http://127.0.0.1:8333", help="DVC S3 remote URL")
    parser.add_argument("--bucket", default="dvc-repo", help="DVC S3 bucket name")
    parser.add_argument("--access-key", default="seaweedfsadmin", help="DVC S3 access key")
    parser.add_argument("--secret-key", default="seaweedfsadmin", help="DVC S3 secret key")

    args = parser.parse_args()

    overall_start = time.time()
    dag_nodes = []

    # Node 1: extract
    n1_start = time.time()
    extracted_docs = []
    n1_status = "completed"
    n1_details = ""
    try:
        stype = args.source_type.lower()
        if stype in ["web", "rss"]:
            extracted_docs = extract_web_or_rss(args.source_path)
            n1_details = f"Extracted {len(extracted_docs)} item(s) from web/rss: {args.source_path}"
        elif stype in ["hf", "huggingface"]:
            extracted_docs = extract_hf_dataset(args.source_path)
            n1_details = f"Extracted {len(extracted_docs)} row(s) from HuggingFace dataset: {args.source_path}"
        elif stype == "local":
            extracted_docs = extract_local_folder(args.source_path)
            n1_details = f"Extracted {len(extracted_docs)} document(s) from local path: {args.source_path}"
        else:
            raise ValueError(f"지원하지 않는 데이터 소스 종류: {args.source_type}")
    except Exception as e:
        n1_status = "failed"
        n1_details = str(e)

    n1_duration = round(time.time() - n1_start, 3)
    dag_nodes.append({
        "node_id": "extract",
        "name": "Data Extraction",
        "status": n1_status,
        "duration_sec": n1_duration,
        "items_processed": len(extracted_docs),
        "details": n1_details
    })

    if n1_status == "failed" or not extracted_docs:
        err_msg = n1_details or "추출된 문서가 없습니다."
        print(json.dumps({
            "status": "error",
            "error": err_msg,
            "dataset_name": args.collection,
            "source_type": args.source_type,
            "source_path": args.source_path,
            "total_duration_sec": round(time.time() - overall_start, 3),
            "total_items_extracted": len(extracted_docs),
            "total_chunks_created": 0,
            "lancedb_collection": args.collection,
            "db_path": str(Path(args.db_path).expanduser().resolve()),
            "dvc_backed_up": False,
            "dag_nodes": dag_nodes
        }))
        sys.exit(1)

    # Node 2: clean_chunk
    n2_start = time.time()
    chunks_data = []
    n2_status = "completed"
    n2_details = ""
    try:
        for doc in extracted_docs:
            c_list = chunk_text(doc["text"], chunk_size=args.chunk_size, overlap=args.chunk_overlap)
            for idx, c_text in enumerate(c_list):
                chunks_data.append({
                    "id": f"{doc['filename']}_{idx}",
                    "source": doc["source"],
                    "filename": doc["filename"],
                    "chunk_index": idx,
                    "text": c_text
                })
        n2_details = f"Generated {len(chunks_data)} chunks (size={args.chunk_size}, overlap={args.chunk_overlap}) from {len(extracted_docs)} docs"
    except Exception as e:
        n2_status = "failed"
        n2_details = str(e)

    n2_duration = round(time.time() - n2_start, 3)
    dag_nodes.append({
        "node_id": "clean_chunk",
        "name": "Clean & Chunk",
        "status": n2_status,
        "duration_sec": n2_duration,
        "items_processed": len(chunks_data),
        "details": n2_details
    })

    if n2_status == "failed" or not chunks_data:
        err_msg = n2_details or "생성된 청크가 없습니다."
        print(json.dumps({
            "status": "error",
            "error": err_msg,
            "dataset_name": args.collection,
            "source_type": args.source_type,
            "source_path": args.source_path,
            "total_duration_sec": round(time.time() - overall_start, 3),
            "total_items_extracted": len(extracted_docs),
            "total_chunks_created": len(chunks_data),
            "lancedb_collection": args.collection,
            "db_path": str(Path(args.db_path).expanduser().resolve()),
            "dvc_backed_up": False,
            "dag_nodes": dag_nodes
        }))
        sys.exit(1)

    # Node 3: lancedb_index
    n3_start = time.time()
    n3_status = "completed"
    n3_details = ""
    db_path = Path(args.db_path).expanduser().resolve()
    db_path.mkdir(parents=True, exist_ok=True)

    try:
        import lancedb
        from sentence_transformers import SentenceTransformer
        
        model = SentenceTransformer(args.embedding_model)
        texts = [item["text"] for item in chunks_data]
        embeddings = model.encode(texts, show_progress_bar=False)

        for item, emb in zip(chunks_data, embeddings):
            item["vector"] = emb.tolist()

        db = lancedb.connect(str(db_path))
        existing_tables = db.list_tables()
        mode = "overwrite" if args.collection in existing_tables else "create"
        db.create_table(args.collection, data=chunks_data, mode=mode)
        n3_details = f"Indexed {len(chunks_data)} vectors into LanceDB collection '{args.collection}' at {db_path}"
    except ImportError:
        # Fallback if lancedb / sentence_transformers missing in python env
        fallback_json = db_path / f"{args.collection}_fallback.json"
        fallback_json.write_text(json.dumps(chunks_data, ensure_ascii=False, indent=2), encoding="utf-8")
        n3_details = f"LanceDB package not available in env. Saved {len(chunks_data)} chunks metadata to {fallback_json}"
    except Exception as e:
        n3_status = "failed"
        n3_details = f"LanceDB indexing error: {e}"

    n3_duration = round(time.time() - n3_start, 3)
    dag_nodes.append({
        "node_id": "lancedb_index",
        "name": "LanceDB Indexing",
        "status": n3_status,
        "duration_sec": n3_duration,
        "items_processed": len(chunks_data),
        "details": n3_details
    })

    # Node 4: dvc_backup
    n4_start = time.time()
    n4_status = "skipped"
    n4_details = "DVC backup not requested"
    dvc_backed_up = False

    if args.dvc_backup and n3_status == "completed":
        try:
            dvc_bin = get_dvc_bin()
            work_dir = db_path

            # Check / init DVC
            dvc_dir = work_dir / ".dvc"
            if not dvc_dir.exists():
                res = subprocess.run([dvc_bin, "init", "--no-scm"], cwd=work_dir, capture_output=True, text=True)
                if res.returncode != 0:
                    subprocess.run([dvc_bin, "init"], cwd=work_dir, capture_output=True, text=True)

            remote_name = "seaweedfs"
            s3_uri = f"s3://{args.bucket}"
            subprocess.run([dvc_bin, "remote", "add", "-f", "-d", remote_name, s3_uri], cwd=work_dir, capture_output=True)
            subprocess.run([dvc_bin, "remote", "modify", remote_name, "endpointurl", args.remote_url], cwd=work_dir, capture_output=True)
            subprocess.run([dvc_bin, "remote", "modify", remote_name, "access_key_id", args.access_key], cwd=work_dir, capture_output=True)
            subprocess.run([dvc_bin, "remote", "modify", remote_name, "secret_access_key", args.secret_key], cwd=work_dir, capture_output=True)
            subprocess.run([dvc_bin, "remote", "modify", remote_name, "use_ssl", "false"], cwd=work_dir, capture_output=True)

            # dvc add table folder if exists
            target_item = f"{args.collection}.lance"
            if not (work_dir / target_item).exists():
                target_item = "."

            subprocess.run([dvc_bin, "add", target_item], cwd=work_dir, capture_output=True, text=True)
            push_res = subprocess.run([dvc_bin, "push", "-r", remote_name], cwd=work_dir, capture_output=True, text=True)

            if push_res.returncode == 0:
                n4_status = "completed"
                dvc_backed_up = True
                n4_details = f"DVC backed up '{target_item}' to {s3_uri} ({args.remote_url})"
            else:
                n4_status = "failed"
                n4_details = f"DVC push failed: {push_res.stderr.strip() or push_res.stdout.strip()}"
        except Exception as e:
            n4_status = "failed"
            n4_details = f"DVC backup exception: {e}"

    n4_duration = round(time.time() - n4_start, 3)
    dag_nodes.append({
        "node_id": "dvc_backup",
        "name": "DVC S3 Backup",
        "status": n4_status,
        "duration_sec": n4_duration,
        "items_processed": 1 if dvc_backed_up else 0,
        "details": n4_details
    })

    total_duration = round(time.time() - overall_start, 3)

    print(json.dumps({
        "status": "ok" if n3_status == "completed" else "error",
        "dataset_name": args.collection,
        "source_type": args.source_type,
        "source_path": args.source_path,
        "total_duration_sec": total_duration,
        "total_items_extracted": len(extracted_docs),
        "total_chunks_created": len(chunks_data),
        "lancedb_collection": args.collection,
        "db_path": str(db_path),
        "dvc_backed_up": dvc_backed_up,
        "dag_nodes": dag_nodes
    }))


if __name__ == "__main__":
    main()
