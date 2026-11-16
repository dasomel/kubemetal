"""Source preservation and operator-visible indexing provenance (#38)."""

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
from rag_host import chunk_records, cmd_index
from source_chunking import chunk_python_source


class SourceIntegrityTests(unittest.TestCase):
    def test_nonoverlapping_chunks_reconstruct_source(self):
        fixtures = [
            "# header\n\n# detached\n\n@dec\ndef f():\n    return 1\n\n# tail\n",
            "import os\n\n# gap\n\n@first\n@second(1)\nasync def f():\n"
            "    def inner():\n        return 2\n    return inner()\n\nx = 3",
            "@dec\nclass C:\n    value = 1\n\n    @cached\n    def m(self):\n"
            "        return self.value\n\n    other = 2\n\n    class Nested:\n"
            "        def n(self):\n            return 3\n\n# end\n",
            "# comments only\n\n# last\n",
            "def f():\r\n    return 1\r\n\r\n",
            "x = 1; y = 2\n\n",
            "class C:\n    label = 'hello\u2028world'\n    def m(self):\n        pass\n",
            "\fdef first():\n    pass\n\fdef second():\n    pass\n",
            "   \n\n\t  ",
            "text = 'unicode: \u2028 and \v'\n\fdef f():\n    pass\n",
            "\n  \t\n",
            "def f():\r    return 1\r\r",
        ]
        for source in fixtures:
            for budget in (17, 80, 500):
                with self.subTest(source=source, budget=budget):
                    chunks = chunk_python_source(source, chunk_size=budget, overlap=0)
                    self.assertTrue(chunks.structured)
                    self.assertEqual("".join(chunks), source)
                    self.assertTrue(all(len(c) <= budget for c in chunks))
                    self.assertEqual(chunks, chunk_python_source(source, budget, 0))
                    records = chunk_records(Path("fixture.py"), source, "python")
                    self.assertEqual(records, chunk_records(Path("fixture.py"), source, "python"))
                    self.assertEqual(len({r["id"] for r in records}), len(records))

    def test_parenthesized_method_decorator_stays_with_method(self):
        method = "    @(\n        decorate\n    )\n    def m(self):\n        pass\n"
        source = "class C:\n    label = '" + "x" * 90 + "'\n" + method
        chunks = chunk_python_source(source, chunk_size=80, overlap=0)
        self.assertIn(method, chunks)
        self.assertEqual("".join(chunks), source)

    def test_invalid_window_is_rejected_before_fallback(self):
        for source in ("def broken(:", "pass", ""):
            for size, overlap in ((0, 0), (5, 5), (5, -1)):
                with self.subTest(source=source, size=size, overlap=overlap):
                    with self.assertRaises(ValueError):
                        chunk_python_source(source, size, overlap)

    def test_index_reports_and_stores_per_file_analysis(self):
        for mode in ("python", "source", "fixed", "heading"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                sources = {"a.txt": "plain text", "bad.py": "def broken(:\n", "good.py": "def f():\n    pass\n", "empty.py": ""}
                for name, source in sources.items():
                    (root / name).write_text(source)
                db = Mock()
                db.list_tables.return_value = []
                model = Mock()
                model.encode.side_effect = lambda texts, **kw: [Mock(tolist=lambda: [1.0]) for _ in texts]
                args = SimpleNamespace(docs_dir=tmp, db_path=str(root / "db"), collection="docs", model="fake", chunking=mode)
                output = io.StringIO()
                with patch.dict(sys.modules, {"lancedb": SimpleNamespace(connect=lambda _: db)}), patch("rag_host.get_embedding_model", return_value=model), contextlib.redirect_stdout(output):
                    cmd_index(args)
                summary = json.loads(output.getvalue())
                rows = db.create_table.call_args.kwargs["data"]
                if mode in ("fixed", "heading"):
                    self.assertNotIn("file_chunking", summary)
                    self.assertTrue(all("chunking_status" not in r for r in rows))
                    continue
                by_file = {Path(f["source"]).name: f for f in summary["file_chunking"]}
                self.assertEqual(by_file["bad.py"]["chunking_status"], "fallback")
                self.assertIn("SyntaxError", by_file["bad.py"]["fallback_reason"])
                self.assertEqual(by_file["good.py"]["chunking_status"], "structured")
                self.assertEqual(by_file["empty.py"]["chunking_status"], "structured")
                self.assertEqual(by_file["a.txt"]["chunking_status"], "fixed")
                self.assertEqual(summary["fallback_files"], 1)
                for row in rows:
                    for key in ("chunking_status", "fallback_reason"):
                        self.assertEqual(row[key], by_file[row["filename"]][key])
                self.assertEqual(chunk_records(root / "good.py", sources["good.py"], mode), chunk_records(root / "good.py", sources["good.py"], mode))
