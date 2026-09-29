import sqlite3
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts" / "rag"))
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

if __name__ == "__main__":
    unittest.main()
