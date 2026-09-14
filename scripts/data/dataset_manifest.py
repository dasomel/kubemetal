"""
KubeMetal Dataset Provenance Manifest & Overwrite Guard (Issue #14, D39)
Provides deterministic dataset versioning, immutable provenance manifest creation,
and destructive overwrite protection for LanceDB collections.
"""

from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

MANIFEST_SCHEMA_VERSION = 1
DEFAULT_INDEX_SCHEMA_VERSION = 1
MANIFEST_FILENAME = "dataset-manifest.json"


class CollectionOverwriteError(Exception):
    """Raised when an existing collection would be overwritten by a different dataset version."""

    def __init__(self, collection: str, existing_version: str, incoming_version: str):
        self.collection = collection
        self.existing_version = existing_version
        self.incoming_version = incoming_version
        super().__init__(
            f"Cannot overwrite collection '{collection}': existing dataset version is "
            f"'{existing_version}', but incoming dataset version is '{incoming_version}'. "
            f"Pass --overwrite-existing to proceed."
        )


def hash_text_content(text: str) -> str:
    """Compute sha256 hex digest for text content."""
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def normalize_source_path(source_type: str, source_path: str) -> str:
    """
    Normalize source path/URI based on source type.
    Local/file source types are expanded and resolved to absolute canonical paths.
    URL-type sources (web, rss, hf, huggingface) retain their URI identity without filesystem resolution.
    """
    stype = str(source_type).strip().lower()
    if stype in ("local", "file"):
        return str(Path(source_path).expanduser().resolve())
    return str(source_path).strip()


def compute_document_entries(raw_documents: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    """
    Extract deterministic per-document metadata and content hashes.
    Sort by (source, filename, sha256) to eliminate filesystem traversal non-determinism.
    """
    entries = []
    for doc in raw_documents:
        source = str(doc.get("source", "")).strip()
        filename = str(doc.get("filename", "")).strip()
        text = str(doc.get("text", ""))
        entries.append({
            "source": source,
            "filename": filename,
            "sha256": hash_text_content(text),
            "char_count": len(text),
            "byte_count": len(text.encode("utf-8")),
        })
    # Sort deterministically so identical document sets produce identical manifests
    entries.sort(key=lambda d: (d["source"], d["filename"], d["sha256"]))
    return entries


def compute_dataset_version(
    source_type: str,
    source_path: str,
    doc_entries: List[Dict[str, Any]],
    chunk_size: int,
    chunk_overlap: int,
    embedding_model: str,
    index_schema_version: int = DEFAULT_INDEX_SCHEMA_VERSION,
) -> str:
    """
    Derive deterministic dataset_version hash from normalized inputs.
    Any change in source content, chunk settings, embedding model, or schema version
    will produce a different hash.
    """
    norm_source_type = str(source_type).strip().lower()
    norm_source_path = normalize_source_path(norm_source_type, source_path)

    # Sort documents deterministically by (source, filename, sha256) so identical document sets
    # produce identical hashes regardless of caller input ordering or traversal sequence.
    sorted_documents = sorted(
        [
            {
                "source": str(d.get("source", "")).strip(),
                "filename": str(d.get("filename", "")).strip(),
                "sha256": str(d.get("sha256", "")).strip(),
            }
            for d in doc_entries
        ],
        key=lambda d: (d["source"], d["filename"], d["sha256"]),
    )

    canonical_input = {
        "source_type": norm_source_type,
        "source_path": norm_source_path,
        "documents": sorted_documents,
        "chunking": {
            "chunk_size": int(chunk_size),
            "chunk_overlap": int(chunk_overlap),
            "strategy": "character_overlap",
        },
        "embedding": {
            "model_id": str(embedding_model).strip(),
        },
        "index": {
            "schema_version": int(index_schema_version),
        },
    }
    canonical_bytes = json.dumps(
        canonical_input, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")
    return hashlib.sha256(canonical_bytes).hexdigest()


def build_dataset_manifest(
    dataset_version: str,
    source_type: str,
    source_path: str,
    doc_entries: List[Dict[str, Any]],
    chunk_size: int,
    chunk_overlap: int,
    total_chunks: int,
    embedding_model_id: str,
    collection_name: str,
    index_schema_version: int = DEFAULT_INDEX_SCHEMA_VERSION,
    vector_dimension: Optional[int] = None,
    embedding_model_version: Optional[str] = None,
    embedding_runtime: Optional[str] = None,
    embedding_device: Optional[str] = None,
    index_backend: str = "lancedb",
    created_at: Optional[str] = None,
) -> Dict[str, Any]:
    """
    Build immutable dataset provenance manifest dictionary.
    D22 compliance: never invent measurements. Unmeasured fields are explicitly None.
    """
    if created_at is None:
        created_at = datetime.now(timezone.utc).isoformat()

    norm_source_type = str(source_type).strip().lower()
    norm_source_path = normalize_source_path(norm_source_type, source_path)

    return {
        "schema_version": MANIFEST_SCHEMA_VERSION,
        "dataset_version": dataset_version,
        "created_at": created_at,
        "source": {
            "source_type": norm_source_type,
            "source_path": norm_source_path,
            "total_documents": len(doc_entries),
            "documents": doc_entries,
        },
        "chunking": {
            "chunk_size": int(chunk_size),
            "chunk_overlap": int(chunk_overlap),
            "splitting_strategy": "character_overlap",
            "total_chunks": int(total_chunks),
        },
        "embedding": {
            "model_id": embedding_model_id,
            "model_version": embedding_model_version,
            "runtime": embedding_runtime,
            "device": embedding_device,
        },
        "index": {
            "backend": index_backend,
            "collection_name": collection_name,
            "index_schema_version": int(index_schema_version),
            "vector_dimension": vector_dimension,
        },
    }


def get_lancedb_table_names(db: Any) -> List[str]:
    """Safely list table names from a LanceDB connection across library versions."""
    try:
        tables = db.list_tables()
        if hasattr(tables, "tables"):
            return list(tables.tables)
        if isinstance(tables, (list, tuple, set)):
            return list(tables)
    except Exception:
        pass
    if hasattr(db, "table_names"):
        try:
            return list(db.table_names())
        except Exception:
            pass
    return []


def get_existing_dataset_version(
    db_path: Path, collection_name: str, lancedb_db: Any = None
) -> Optional[str]:
    """
    Inspect existing collection to discover its dataset_version.
    Checks LanceDB table records, manifest files on disk, or fallback JSON.
    Returns None if the collection does not exist.
    """
    db_path = Path(db_path).expanduser().resolve()

    # 1. Check LanceDB table if connection provided
    if lancedb_db is not None:
        try:
            table_names = get_lancedb_table_names(lancedb_db)
            if collection_name in table_names:
                table = lancedb_db.open_table(collection_name)
                if "dataset_version" in table.schema.names:
                    versions = table.to_arrow().column("dataset_version").to_pylist()
                    versions = {str(version) for version in versions if version}
                    if len(versions) == 1:
                        return versions.pop()
                    if len(versions) > 1:
                        return "mixed-versions"
        except Exception:
            pass

    # 2. Check manifest files on disk
    manifest_candidates = [
        db_path / f"{collection_name}.lance" / MANIFEST_FILENAME,
        db_path / f"{collection_name}.manifest.json",
        db_path / f"{collection_name}.dataset-manifest.json",
    ]
    for candidate in manifest_candidates:
        if candidate.is_file():
            try:
                data = json.loads(candidate.read_text(encoding="utf-8"))
                version = data.get("dataset_version")
                if version:
                    return str(version)
            except Exception:
                pass

    # 3. Check fallback JSON file
    fallback_file = db_path / f"{collection_name}_fallback.json"
    if fallback_file.is_file():
        try:
            items = json.loads(fallback_file.read_text(encoding="utf-8"))
            if isinstance(items, list) and items:
                version = items[0].get("dataset_version")
                if version:
                    return str(version)
        except Exception:
            pass

    # 4. If table directory or fallback file exists but no version found, mark as legacy
    table_dir = db_path / f"{collection_name}.lance"
    if table_dir.is_dir() or fallback_file.is_file():
        return "legacy-unversioned"

    return None


def check_overwrite_guard(
    db_path: Path,
    collection_name: str,
    incoming_version: str,
    overwrite_existing: bool,
    lancedb_db: Any = None,
) -> Tuple[bool, Optional[str]]:
    """
    Enforce destructive overwrite guard.
    - If collection doesn't exist: allowed.
    - If existing collection has the same dataset_version: allowed idempotently.
    - If existing collection has a different dataset_version:
        - allowed only if overwrite_existing is True.
        - otherwise raises CollectionOverwriteError.
    """
    existing_version = get_existing_dataset_version(db_path, collection_name, lancedb_db)
    if existing_version is None:
        return True, None

    if existing_version == incoming_version:
        return True, existing_version

    if overwrite_existing:
        return True, existing_version

    raise CollectionOverwriteError(collection_name, existing_version, incoming_version)


def write_dataset_manifest(
    db_path: Path,
    collection_name: str,
    manifest: Dict[str, Any],
) -> Tuple[Path, List[Path]]:
    """
    Write dataset-manifest.json next to the collection.
    - Inside collection directory: <db_path>/<collection>.lance/dataset-manifest.json (if dir exists)
    - Alongside collection: <db_path>/<collection>.manifest.json
    Returns (primary_path, all_written_paths). The adjacent copy keeps discovery
    working for legacy tooling while avoiding a misleading multi-collection root file.
    """
    db_path = Path(db_path).expanduser().resolve()
    db_path.mkdir(parents=True, exist_ok=True)
    serialized = json.dumps(manifest, ensure_ascii=False, indent=2)

    written: List[Path] = []
    lance_dir = db_path / f"{collection_name}.lance"
    if lance_dir.is_dir():
        table_manifest = lance_dir / MANIFEST_FILENAME
        table_manifest.write_text(serialized, encoding="utf-8")
        written.append(table_manifest)

    collection_manifest = db_path / f"{collection_name}.manifest.json"
    collection_manifest.write_text(serialized, encoding="utf-8")
    written.append(collection_manifest)

    primary_path = written[0]
    return primary_path, written
