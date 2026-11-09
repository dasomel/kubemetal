import argparse
import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts" / "rag"))
import rag_host


class NullableMetadataTests(unittest.TestCase):
    def test_hybrid_ties_sort_null_and_known_sources_without_changing_metadata(self):
        rows = [
            {"id": "a", "source": None, "chunk_index": 0, "text": None, "filename": None},
            {"id": "b", "source": "/docs/doc.md", "chunk_index": 0},
        ]
        results = rag_host.rrf_fuse(rows, list(reversed(rows)), 10)
        self.assertEqual([row["id"] for row in results], ["a", "b"])
        self.assertIsNone(results[0]["source"])

    def test_query_preserves_null_and_missing_metadata_in_all_modes(self):
        chunks = [
            {"id": "a", "filename": "doc.md", "source": None, "text": None, "chunk_index": 0},
            {"id": "b", "filename": None, "source": "/docs/doc.md", "text": None, "chunk_index": 0},
            {"id": "c", "filename": "doc.md", "chunk_index": 0},
        ]
        for mode, query in (("exact", "doc.md"), ("lexical", "doc.md"),
                            ("dense", "doc.md"), ("hybrid", "doc.md"),
                            ("auto", "doc.md"), ("auto", "explain document metadata")):
            with self.subTest(mode=mode, query=query), tempfile.TemporaryDirectory() as db_path:
                table = Mock()
                table.to_arrow.return_value.to_pylist.return_value = chunks
                table.search.return_value.limit.return_value.to_list.return_value = chunks
                sdk = Mock()
                sdk.connect.return_value.list_tables.return_value = ["default"]
                sdk.connect.return_value.open_table.return_value = table
                args = argparse.Namespace(db_path=db_path, collection="default", query=query,
                                          top_k=10, model="mock", mode=mode)
                output = io.StringIO()
                with patch.dict(sys.modules, {"lancedb": sdk}), \
                        patch.object(rag_host, "get_embedding_model", return_value=Mock()), \
                        contextlib.redirect_stdout(output):
                    rag_host.cmd_query(args)
                results = json.loads(output.getvalue())["results"]
                self.assertIn("a", [row["id"] for row in results])
                self.assertIn("c", [row["id"] for row in results])
                for row in results:
                    original = next(chunk for chunk in chunks if chunk["id"] == row["id"])
                    for field in ("text", "filename", "source"):
                        self.assertEqual(row[field], original.get(field))


if __name__ == "__main__":
    unittest.main()
