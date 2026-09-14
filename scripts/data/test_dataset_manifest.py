"""
Unit tests for dataset manifest and overwrite guard (Issue #14, D39).
Verifies:
- deterministic id stability across runs
- id sensitivity to content, chunk size, chunk overlap, embedding model, and schema version
- destructive overwrite guard (refusal by default, idempotent convergence for same version, permission with flag)
- manifest schema and disk persistence
"""

import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))

from dataset_manifest import (
    CollectionOverwriteError,
    MANIFEST_SCHEMA_VERSION,
    build_dataset_manifest,
    check_overwrite_guard,
    compute_dataset_version,
    compute_document_entries,
    get_existing_dataset_version,
    hash_text_content,
    write_dataset_manifest,
)


class TestDatasetManifest(unittest.TestCase):
    def setUp(self):
        self.tmp_dir = Path(tempfile.mkdtemp(prefix="kubemetal-manifest-test-"))
        self.raw_docs = [
            {
                "source": "file:///data/alpha.txt",
                "filename": "alpha.txt",
                "text": "The quick brown fox jumps over the lazy dog.",
            },
            {
                "source": "file:///data/beta.txt",
                "filename": "beta.txt",
                "text": "Apple Silicon unified memory enables high throughput local ML workloads.",
            },
        ]
        self.doc_entries = compute_document_entries(self.raw_docs)
        self.default_params = {
            "source_type": "local",
            "source_path": "/data",
            "doc_entries": self.doc_entries,
            "chunk_size": 500,
            "chunk_overlap": 50,
            "embedding_model": "sentence-transformers/all-MiniLM-L6-v2",
            "index_schema_version": 1,
        }

    def tearDown(self):
        shutil.rmtree(self.tmp_dir, ignore_errors=True)

    def test_deterministic_id_stability(self):
        """Identical inputs and config must yield the exact same dataset_version."""
        v1 = compute_dataset_version(**self.default_params)
        v2 = compute_dataset_version(**self.default_params)
        self.assertEqual(v1, v2)
        self.assertEqual(len(v1), 64, "SHA-256 hex digest should be 64 characters")

        # Shuffled document input order must still yield the same version (deterministic sorting)
        shuffled_raw = list(reversed(self.raw_docs))
        shuffled_entries = compute_document_entries(shuffled_raw)
        v3 = compute_dataset_version(
            source_type=self.default_params["source_type"],
            source_path=self.default_params["source_path"],
            doc_entries=shuffled_entries,
            chunk_size=self.default_params["chunk_size"],
            chunk_overlap=self.default_params["chunk_overlap"],
            embedding_model=self.default_params["embedding_model"],
            index_schema_version=self.default_params["index_schema_version"],
        )
        self.assertEqual(v1, v3)

    def test_id_changes_on_content_change(self):
        """Changing even a single character of document content must produce a different id."""
        baseline = compute_dataset_version(**self.default_params)

        mutated_docs = [
            {
                "source": "file:///data/alpha.txt",
                "filename": "alpha.txt",
                "text": "The quick brown fox jumps over the lazy dog!",  # . -> !
            },
            self.raw_docs[1],
        ]
        mutated_entries = compute_document_entries(mutated_docs)
        params = dict(self.default_params)
        params["doc_entries"] = mutated_entries
        changed = compute_dataset_version(**params)

        self.assertNotEqual(baseline, changed)

    def test_id_changes_on_chunk_size_change(self):
        """Changing chunk_size must produce a different dataset_version."""
        baseline = compute_dataset_version(**self.default_params)
        params = dict(self.default_params)
        params["chunk_size"] = 250
        changed = compute_dataset_version(**params)
        self.assertNotEqual(baseline, changed)

    def test_id_changes_on_chunk_overlap_change(self):
        """Changing chunk_overlap must produce a different dataset_version."""
        baseline = compute_dataset_version(**self.default_params)
        params = dict(self.default_params)
        params["chunk_overlap"] = 20
        changed = compute_dataset_version(**params)
        self.assertNotEqual(baseline, changed)

    def test_id_changes_on_embedding_model_change(self):
        """Changing embedding_model must produce a different dataset_version."""
        baseline = compute_dataset_version(**self.default_params)
        params = dict(self.default_params)
        params["embedding_model"] = "BAAI/bge-small-en-v1.5"
        changed = compute_dataset_version(**params)
        self.assertNotEqual(baseline, changed)

    def test_id_changes_on_schema_version_change(self):
        """Changing index_schema_version must produce a different dataset_version."""
        baseline = compute_dataset_version(**self.default_params)
        params = dict(self.default_params)
        params["index_schema_version"] = 2
        changed = compute_dataset_version(**params)
        self.assertNotEqual(baseline, changed)

    def test_manifest_schema_and_persistence(self):
        """Manifest dictionary adheres to schema requirements and serializes properly."""
        version = compute_dataset_version(**self.default_params)
        manifest = build_dataset_manifest(
            dataset_version=version,
            source_type="local",
            source_path="/data",
            doc_entries=self.doc_entries,
            chunk_size=500,
            chunk_overlap=50,
            total_chunks=10,
            embedding_model_id="sentence-transformers/all-MiniLM-L6-v2",
            collection_name="test_col",
            vector_dimension=384,
            embedding_model_version=None,
            embedding_runtime="sentence_transformers_5.6.0",
            embedding_device="cpu",
            index_backend="lancedb",
        )

        self.assertEqual(manifest["schema_version"], MANIFEST_SCHEMA_VERSION)
        self.assertEqual(manifest["dataset_version"], version)
        self.assertIn("created_at", manifest)
        self.assertEqual(manifest["source"]["total_documents"], 2)
        self.assertEqual(manifest["chunking"]["chunk_size"], 500)
        self.assertEqual(manifest["embedding"]["model_id"], "sentence-transformers/all-MiniLM-L6-v2")
        self.assertEqual(manifest["index"]["collection_name"], "test_col")
        self.assertEqual(manifest["index"]["vector_dimension"], 384)

        # Write manifest to disk alongside a mock .lance directory
        lance_dir = self.tmp_dir / "test_col.lance"
        lance_dir.mkdir(parents=True)
        primary_path, written = write_dataset_manifest(self.tmp_dir, "test_col", manifest)

        self.assertTrue(primary_path.is_file())
        self.assertTrue((lance_dir / "dataset-manifest.json").is_file())
        self.assertTrue((self.tmp_dir / "test_col.manifest.json").is_file())
        self.assertFalse((self.tmp_dir / "dataset-manifest.json").exists())

        # Verify reading back existing version
        read_version = get_existing_dataset_version(self.tmp_dir, "test_col")
        self.assertEqual(read_version, version)

    def test_overwrite_guard_new_collection_allowed(self):
        """Non-existing collection allows ingestion without error."""
        version = compute_dataset_version(**self.default_params)
        allowed, existing = check_overwrite_guard(
            db_path=self.tmp_dir,
            collection_name="new_col",
            incoming_version=version,
            overwrite_existing=False,
        )
        self.assertTrue(allowed)
        self.assertIsNone(existing)

    def test_overwrite_guard_same_version_idempotent(self):
        """Re-ingesting the exact same dataset version must be idempotent and allowed."""
        version = compute_dataset_version(**self.default_params)
        manifest = build_dataset_manifest(
            dataset_version=version,
            source_type="local",
            source_path="/data",
            doc_entries=self.doc_entries,
            chunk_size=500,
            chunk_overlap=50,
            total_chunks=10,
            embedding_model_id="sentence-transformers/all-MiniLM-L6-v2",
            collection_name="idempotent_col",
        )
        write_dataset_manifest(self.tmp_dir, "idempotent_col", manifest)

        allowed, existing = check_overwrite_guard(
            db_path=self.tmp_dir,
            collection_name="idempotent_col",
            incoming_version=version,
            overwrite_existing=False,
        )
        self.assertTrue(allowed)
        self.assertEqual(existing, version)

    def test_overwrite_guard_refuses_different_version_without_flag(self):
        """Overwriting with a different dataset version must refuse by default and cite both versions."""
        v1 = compute_dataset_version(**self.default_params)
        manifest = build_dataset_manifest(
            dataset_version=v1,
            source_type="local",
            source_path="/data",
            doc_entries=self.doc_entries,
            chunk_size=500,
            chunk_overlap=50,
            total_chunks=10,
            embedding_model_id="sentence-transformers/all-MiniLM-L6-v2",
            collection_name="guarded_col",
        )
        write_dataset_manifest(self.tmp_dir, "guarded_col", manifest)

        # Incoming version has different chunk size
        params = dict(self.default_params)
        params["chunk_size"] = 200
        v2 = compute_dataset_version(**params)

        with self.assertRaises(CollectionOverwriteError) as ctx:
            check_overwrite_guard(
                db_path=self.tmp_dir,
                collection_name="guarded_col",
                incoming_version=v2,
                overwrite_existing=False,
            )

        err_msg = str(ctx.exception)
        self.assertIn("guarded_col", err_msg)
        self.assertIn(v1, err_msg)
        self.assertIn(v2, err_msg)
        self.assertIn("--overwrite-existing", err_msg)

    def test_overwrite_guard_permits_different_version_with_flag(self):
        """Overwriting with a different dataset version succeeds when overwrite_existing=True."""
        v1 = compute_dataset_version(**self.default_params)
        manifest = build_dataset_manifest(
            dataset_version=v1,
            source_type="local",
            source_path="/data",
            doc_entries=self.doc_entries,
            chunk_size=500,
            chunk_overlap=50,
            total_chunks=10,
            embedding_model_id="sentence-transformers/all-MiniLM-L6-v2",
            collection_name="flag_col",
        )
        write_dataset_manifest(self.tmp_dir, "flag_col", manifest)

        params = dict(self.default_params)
        params["chunk_size"] = 200
        v2 = compute_dataset_version(**params)

        allowed, existing = check_overwrite_guard(
            db_path=self.tmp_dir,
            collection_name="flag_col",
            incoming_version=v2,
            overwrite_existing=True,
        )
        self.assertTrue(allowed)
        self.assertEqual(existing, v1)


if __name__ == "__main__":
    unittest.main()
