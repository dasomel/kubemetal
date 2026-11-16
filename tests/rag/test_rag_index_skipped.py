"""Per-file indexing failures must survive the CLI/IPC boundary (#38)."""
import contextlib
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts" / "rag"))
from rag_host import cmd_index


class IndexSkippedTests(unittest.TestCase):
    def run_index(self, root, mode="fixed", failure=None, all_failed=False):
        db = Mock()
        db.list_tables.return_value = []
        model = Mock()
        model.encode.side_effect = lambda texts, **kw: [Mock(tolist=lambda: [1.0]) for _ in texts]
        args = SimpleNamespace(docs_dir=str(root), db_path=str(root / "db"),
                               collection="docs", model="fake", chunking=mode)
        original = Path.read_text
        def read(path, *args, **kwargs):
            if failure and (all_failed or path.name == "bad.py"):
                raise failure
            return original(path, *args, **kwargs)
        output = io.StringIO()
        code = 0
        with patch.dict(sys.modules, {"lancedb": SimpleNamespace(connect=lambda _: db)}), \
                patch("rag_host.get_embedding_model", return_value=model), \
                patch.object(Path, "read_text", read), contextlib.redirect_stdout(output):
            try:
                cmd_index(args)
            except SystemExit as exc:
                code = exc.code
        return output.getvalue(), code, db

    def test_unreadable_file_in_every_mode(self):
        for mode in ("fixed", "heading", "python", "source"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp).resolve()
                (root / "good.md").write_text("# Heading\n\nUseful text\n")
                (root / "bad.py").write_text("pass\n")
                output, code, db = self.run_index(root, mode, PermissionError("denied"))
                result = json.loads(output)
                self.assertEqual(code, 0)
                self.assertEqual(result["status"], "ok")
                self.assertEqual(result.get("skipped_files"), [
                    {"source": str(root / "bad.py"), "reason": "PermissionError: denied"}
                ], output)
                self.assertTrue(result["partial"])
                self.assertEqual(result["skipped_count"], 1)
                self.assertEqual(result["indexed_docs"], 1)
                self.assertEqual(result["total_chunks"], 1)
                rows = db.create_table.call_args.kwargs["data"]
                self.assertEqual([r["filename"] for r in rows], ["good.md"])
                if mode in ("python", "source"):
                    self.assertEqual(result["error_files"], result["skipped_count"])
                    failed = next(f for f in result["file_chunking"] if f["source"].endswith("bad.py"))
                    self.assertEqual(failed["fallback_reason"], result["skipped_files"][0]["reason"])

    def test_all_files_fail_with_reasons_without_embedding_or_storage(self):
        for mode in ("fixed", "heading", "python", "source"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp).resolve()
                for name in ("good.md", "bad.py"):
                    (root / name).write_text("text")
                output, code, db = self.run_index(root, mode, OSError("read failed"), True)
                result = json.loads(output)
                self.assertEqual(code, 1)
                self.assertEqual(result["status"], "error")
                self.assertEqual(result.get("skipped_count"), 2, output)
                self.assertEqual({Path(f["source"]).name for f in result["skipped_files"]}, {"good.md", "bad.py"})
                self.assertTrue(all(f["reason"] == "OSError: read failed" for f in result["skipped_files"]))
                db.create_table.assert_not_called()

    def test_success_bytes_and_rows_unchanged_except_explicit_empty_summary(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            source = root / "good.md"
            source.write_text("# Heading\n\nUseful text\n")
            output, code, db = self.run_index(root)
            baseline = {"status": "ok", "collection": "docs", "indexed_docs": 1,
                        "total_chunks": 1, "db_path": str(root / "db")}
            self.assertEqual(code, 0)
            self.assertEqual(output, json.dumps({**baseline, "partial": False,
                "skipped_files": [], "skipped_count": 0}) + "\n")
            self.assertEqual(db.create_table.call_args.kwargs["data"], [{
                "id": "good.md_0", "source": str(source), "filename": "good.md",
                "chunk_index": 0, "text": "# Heading\n\nUseful text", "vector": [1.0]}])

    def test_unicode_read_error_is_reported(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            (root / "good.md").write_text("text")
            (root / "bad.py").write_text("pass")
            output, _, _ = self.run_index(root, failure=UnicodeError("decode failed"))
            self.assertEqual(json.loads(output).get("skipped_count"), 1, output)

    def test_chunk_read_errors_are_reported(self):
        from rag_host import chunk_records
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            (root / "good.md").write_text("text")
            (root / "bad.py").write_text("pass")
            def chunk(path, content, mode):
                if path.name == "bad.py":
                    raise OSError("chunk input unavailable")
                return chunk_records(path, content, mode)
            with patch("rag_host.chunk_records", side_effect=chunk):
                output, code, _ = self.run_index(root)
            result = json.loads(output)
            self.assertEqual(code, 0)
            self.assertEqual(result["skipped_files"], [{"source": str(root / "bad.py"),
                "reason": "OSError: chunk input unavailable"}])

    def test_programming_errors_propagate(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            (root / "good.md").write_text("text")
            with patch("rag_host.chunk_records", side_effect=TypeError("bug")):
                with self.assertRaisesRegex(TypeError, "bug"):
                    self.run_index(root)
