import sqlite3
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts" / "rag"))
from rag_host import ensure_fts5, lexical_search


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


if __name__ == "__main__":
    unittest.main()
