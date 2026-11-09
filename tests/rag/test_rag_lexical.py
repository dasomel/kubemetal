import argparse
import contextlib
import io
import json
import sqlite3
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts" / "rag"))
import rag_host
from rag_host import ensure_fts5, lexical_search, rrf_fuse


class LexicalRetrievalTests(unittest.TestCase):
    def test_exact_identifier_ranks_matching_chunk_first(self):
        chunks = [
            {"text": "General notes about language models", "filename": "notes.md"},
            {"text": "Use Qwen2-VL-2B-Instruct-4bit for vision", "filename": "model.md"},
        ]
        with tempfile.TemporaryDirectory() as _:
            results = lexical_search(chunks, "Qwen2-VL-2B-Instruct-4bit", 2)
        self.assertEqual(results[0]["filename"], "model.md")
        self.assertEqual(results[0]["mode"], "lexical")

    def test_extra_words_rank_relevant_chunk_first(self):
        chunks = [
            {"text": "Memory optimization for other accelerators", "filename": "other.md"},
            {"text": "Apple Silicon Metal GPU memory optimization tips for MLX", "filename": "mlx.md"},
        ]
        results = lexical_search(chunks, "Apple Silicon Metal memory optimization", 2)
        self.assertEqual(results[0]["filename"], "mlx.md")

    def test_reordered_terms_rank_relevant_chunk_first(self):
        chunks = [
            {"text": "Memory optimization for other accelerators", "filename": "other.md"},
            {"text": "Apple Silicon Metal GPU memory optimization tips for MLX", "filename": "mlx.md"},
        ]
        results = lexical_search(chunks, "memory Metal Apple", 2)
        self.assertEqual(results[0]["filename"], "mlx.md")

    def test_operator_looking_queries_do_not_crash(self):
        chunks = [{"text": "NEAR words and OR operators", "filename": "operators.md"}]
        self.assertEqual(lexical_search(chunks, "*", 2), [])
        for query in ("NEAR", "*", "OR", "column:foo", '"'):
            with self.subTest(query=query):
                results = lexical_search(chunks, query, 2)
                self.assertTrue(all(result["mode"] == "lexical" for result in results))

    def test_missing_fts5_fails_explicitly(self):
        def unavailable(_):
            raise sqlite3.OperationalError("no such module: fts5")

        with self.assertRaisesRegex(RuntimeError, "SQLite FTS5 is unavailable"):
            ensure_fts5(unavailable)

    def test_rrf_tie_breaks_deterministically_by_chunk_id(self):
        results = rrf_fuse(
            [{"id": "z", "score": -1.0}, {"id": "a", "score": -2.0}],
            [{"id": "a", "_distance": 0.1}, {"id": "z", "_distance": 0.2}],
            2,
        )
        self.assertEqual([result["id"] for result in results], ["a", "z"])
        self.assertAlmostEqual(results[0]["score"], 1 / 61 + 1 / 62)

    def test_rrf_deduplicates_by_chunk_id_and_keeps_first_rank(self):
        results = rrf_fuse(
            [{"id": "same", "score": -1.0}, {"id": "same", "score": -2.0}],
            [{"id": "same", "_distance": 0.1}],
            3,
        )
        self.assertEqual(len(results), 1)
        self.assertEqual(results[0]["provenance"]["lexical"], {"rank": 1, "score": -1.0})
        self.assertEqual(results[0]["provenance"]["dense"], {"rank": 1, "score": 0.1})

    def test_rrf_returns_dense_hits_when_lexical_is_empty(self):
        results = rrf_fuse([], [{"id": "dense-only", "_distance": 0.25}], 3)
        self.assertEqual(results[0]["id"], "dense-only")
        self.assertEqual(results[0]["provenance"]["retrievers"], ["dense"])
        self.assertIsNone(results[0]["provenance"]["lexical"])


    def test_rrf_keeps_same_named_files_from_different_directories_apart(self):
        a = {"id": "README.md_0", "source": "/a/README.md", "chunk_index": 0, "text": "a"}
        b = {"id": "README.md_0", "source": "/b/README.md", "chunk_index": 0, "text": "b"}
        results = rrf_fuse([a, b], [], 5)
        self.assertEqual(sorted(r["text"] for r in results), ["a", "b"])

class ExactRetrievalTests(unittest.TestCase):
    def setUp(self):
        self.chunks = [
            {"id": "same_1", "filename": "문서.md", "source": "/b/문서.md", "chunk_index": 1, "text": "last"},
            {"id": "same_0", "filename": "문서.md", "source": "/a/문서.md", "chunk_index": 0, "text": "first"},
            {"id": "same_0", "filename": "문서.md", "source": "/b/문서.md", "chunk_index": 0, "text": "middle"},
            {"id": "other", "filename": "other.md", "source": "/other.md", "chunk_index": 0, "text": "문서.md"},
        ]

    def query(self, query, top_k=10, chunks=None):
        table = Mock()
        table.to_arrow.return_value.to_pylist.return_value = self.chunks if chunks is None else chunks
        db = Mock()
        db.list_tables.return_value = ["default"]
        db.open_table.return_value = table
        sdk = Mock()
        sdk.connect.return_value = db
        with tempfile.TemporaryDirectory() as db_path:
            args = argparse.Namespace(db_path=db_path, collection="default", query=query,
                                      top_k=top_k, model="must-not-load", mode="exact")
            output = io.StringIO()
            with patch.dict(sys.modules, {"lancedb": sdk}), \
                    patch.object(rag_host, "get_embedding_model", side_effect=AssertionError("embedding forbidden")), \
                    patch.object(rag_host, "lexical_search", side_effect=AssertionError("lexical forbidden")), \
                    contextlib.redirect_stdout(output):
                rag_host.cmd_query(args)
        table.search.assert_not_called()
        return json.loads(output.getvalue())

    def test_filename_matches_all_sources_in_chunk_order_before_limit(self):
        response = self.query("문서.md", top_k=2)
        self.assertEqual(response["resolved_mode"], "exact")
        self.assertEqual(response["rule"], "explicit")
        self.assertEqual([r["text"] for r in response["results"]], ["first", "middle"])
        self.assertTrue(all(r["mode"] == "exact" and r["score"] == 1.0 for r in response["results"]))

    def test_source_matches_only_that_path(self):
        self.assertEqual([r["text"] for r in self.query("/b/문서.md")["results"]], ["middle", "last"])

    def test_no_partial_case_normalization_text_or_operator_matches(self):
        for query in ("문서", " 문서.md", "문서.md ", "OTHER.md", "../b/문서.md", "last", "*", "' OR 1=1 --"):
            with self.subTest(query=query):
                response = self.query(query)
                self.assertEqual(response["results"], [])
                self.assertEqual(response["resolved_mode"], "exact")
        self.assertEqual(self.query("문서.md", chunks=[])["results"], [])
        self.assertEqual(self.query("문서.md", top_k=0)["results"], [])
        self.assertEqual(self.query("문서.md", top_k=-1)["results"], [])

    def test_order_is_independent_of_storage_order_with_id_tiebreak(self):
        chunks = self.chunks + [dict(self.chunks[1], id="aaa", text="tie first")]
        expected = self.query("문서.md", chunks=chunks)
        self.assertEqual(expected, self.query("문서.md", chunks=list(reversed(chunks))))
        self.assertEqual(expected["results"][0]["id"], "aaa")

    def test_none_metadata_and_missing_keys_do_not_crash(self):
        chunks = [
            {"id": None, "filename": "none.md", "source": None, "chunk_index": None, "text": "no meta"},
            {"id": "c1", "filename": "none.md", "source": "/a", "chunk_index": 0, "text": "with meta"},
        ]
        res = self.query("none.md", chunks=chunks)
        self.assertEqual(len(res["results"]), 2)
        self.assertEqual(res["results"][0]["text"], "no meta")
        self.assertEqual(res["results"][0]["chunk_index"], 0)

    def test_storage_failure_surfaces_instead_of_empty_success(self):
        with patch.object(rag_host, "lancedb_table_names", side_effect=RuntimeError("storage unavailable")):
            with self.assertRaisesRegex(RuntimeError, "storage unavailable"):
                self.query("문서.md")

    def test_cli_accepts_exact_and_preserves_dense_default(self):
        for mode in ([], ["--mode", "exact"]):
            with self.subTest(mode=mode), \
                    patch.object(sys, "argv", ["rag_host.py", "query", "--query", "doc.md", *mode]), \
                    patch.object(rag_host, "cmd_query") as query:
                rag_host.main()
                self.assertEqual(query.call_args.args[0].mode, "exact" if mode else "dense")


if __name__ == "__main__":
    unittest.main()
