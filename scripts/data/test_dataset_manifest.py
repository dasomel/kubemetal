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
    normalize_source_path,
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

    def test_dataset_version_local_path_normalization(self):
        """Relative, absolute, and trailing-slash paths to identical local content produce identical dataset_version."""
        data_dir = self.tmp_dir / "corpus"
        data_dir.mkdir(parents=True)
        (data_dir / "sample.txt").write_text("Deterministic content for normalization test.", encoding="utf-8")

        abs_path = str(data_dir.resolve())
        trailing_slash_path = f"{abs_path}/"
        rel_path = os.path.relpath(abs_path, start=os.getcwd())

        v_abs = compute_dataset_version(
            source_type="local",
            source_path=abs_path,
            doc_entries=self.doc_entries,
            chunk_size=500,
            chunk_overlap=50,
            embedding_model="sentence-transformers/all-MiniLM-L6-v2",
        )
        v_slash = compute_dataset_version(
            source_type="local",
            source_path=trailing_slash_path,
            doc_entries=self.doc_entries,
            chunk_size=500,
            chunk_overlap=50,
            embedding_model="sentence-transformers/all-MiniLM-L6-v2",
        )
        v_rel = compute_dataset_version(
            source_type="local",
            source_path=rel_path,
            doc_entries=self.doc_entries,
            chunk_size=500,
            chunk_overlap=50,
            embedding_model="sentence-transformers/all-MiniLM-L6-v2",
        )

        self.assertEqual(v_abs, v_slash)
        self.assertEqual(v_abs, v_rel)

    def test_dataset_version_url_not_resolved(self):
        """URL-type sources (web, rss, hf, huggingface) must preserve their URI identity and not be path-resolved."""
        web_url = "https://example.com/dataset/v1"
        self.assertEqual(normalize_source_path("web", web_url), web_url)
        self.assertEqual(normalize_source_path("rss", web_url), web_url)
        self.assertEqual(normalize_source_path("hf", "wikitext"), "wikitext")
        self.assertEqual(normalize_source_path("huggingface", "wikitext"), "wikitext")

        v_web = compute_dataset_version(
            source_type="web",
            source_path=web_url,
            doc_entries=self.doc_entries,
            chunk_size=500,
            chunk_overlap=50,
            embedding_model="sentence-transformers/all-MiniLM-L6-v2",
        )
        self.assertIsInstance(v_web, str)
        self.assertEqual(len(v_web), 64)

    def test_compute_dataset_version_document_order_determinism(self):
        """compute_dataset_version must produce identical hash even if doc_entries are provided in arbitrary order."""
        v_forward = compute_dataset_version(**self.default_params)

        reversed_entries = list(reversed(self.doc_entries))
        params_reversed = dict(self.default_params)
        params_reversed["doc_entries"] = reversed_entries
        v_reversed = compute_dataset_version(**params_reversed)

        self.assertEqual(v_forward, v_reversed)

    def test_overwrite_guard_live_schema_inspection_rescue(self):
        """When manifest file is missing but LanceDB table carries dataset_version, live schema inspection rescues version."""
        version = compute_dataset_version(**self.default_params)

        # Create a mock .lance directory without manifest files
        table_dir = self.tmp_dir / "rescue_col.lance"
        table_dir.mkdir(parents=True)

        # Disk-only check without connection falls back to legacy-unversioned and rejects
        with self.assertRaises(CollectionOverwriteError) as ctx:
            check_overwrite_guard(
                db_path=self.tmp_dir,
                collection_name="rescue_col",
                incoming_version=version,
                overwrite_existing=False,
                lancedb_db=None,
            )
        self.assertIn("legacy-unversioned", str(ctx.exception))

        # Mock LanceDB connection that returns the live dataset_version from table schema
        class MockColumn:
            def to_pylist(self):
                return [version, version]

        class MockArrowTable:
            def column(self, name):
                return MockColumn()

        class MockTable:
            schema = type("Schema", (), {"names": ["dataset_version", "vector", "text"]})()

            def to_arrow(self):
                return MockArrowTable()

        class MockDb:
            def list_tables(self):
                return ["rescue_col"]

            def open_table(self, name):
                return MockTable()

        # With active connection, the live schema is inspected and the version matches
        allowed, existing = check_overwrite_guard(
            db_path=self.tmp_dir,
            collection_name="rescue_col",
            incoming_version=version,
            overwrite_existing=False,
            lancedb_db=MockDb(),
        )
        self.assertTrue(allowed)
        self.assertEqual(existing, version)

    def test_manifest_write_failure_demoted_to_warning(self):
        """Failure to write manifest must not fail indexing; demoted to warning and exits 0 (D37/D39)."""
        import contextlib
        import io
        from unittest.mock import patch
        import ingest_host

        data_dir = self.tmp_dir / "ingest_source"
        data_dir.mkdir(parents=True)
        (data_dir / "test.txt").write_text("Hello KubeMetal ingest test", encoding="utf-8")

        target_db = self.tmp_dir / "lancedb_target"
        target_db.mkdir(parents=True)

        test_args = [
            "ingest_host.py",
            "--source-type", "local",
            "--source-path", str(data_dir),
            "--collection", "warning_test_col",
            "--db-path", str(target_db),
        ]

        stdout_buf = io.StringIO()
        stderr_buf = io.StringIO()

        with patch("sys.argv", test_args), \
             patch("ingest_host.write_dataset_manifest", side_effect=OSError("Disk quota exceeded")), \
             contextlib.redirect_stdout(stdout_buf), \
             contextlib.redirect_stderr(stderr_buf):
            # ingest_host.main() will raise SystemExit(1) on failure, or return cleanly on ok
            ingest_host.main()

        output_str = stdout_buf.getvalue()
        payload = json.loads(output_str)

        self.assertEqual(payload["status"], "ok")
        self.assertIsNone(payload["error"])
        self.assertIsNone(payload["manifest_path"])
        self.assertIsNotNone(payload["warning"])
        self.assertIn("Disk quota exceeded", payload["warning"])

        # Check DAG node 3 (lancedb_index) status is completed with warning in details
        lancedb_node = next(n for n in payload["dag_nodes"] if n["node_id"] == "lancedb_index")
        self.assertEqual(lancedb_node["status"], "completed")
        self.assertIn("warning", lancedb_node["details"])

        # Check stderr logged the warning matching D37 MLX convention
        stderr_str = stderr_buf.getvalue()
        self.assertIn("Ingestion warning: failed to write dataset manifest", stderr_str)


if __name__ == "__main__":
    unittest.main()
