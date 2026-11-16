"""Tests for opt-in Python source structure-aware chunker (issue #38)."""

import sys
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "scripts" / "rag"))

from rag_host import chunk_records, chunk_text
from source_chunking import (
    SourceChunkResult,
    chunk_python_source,
    chunk_text as ref_chunk_text,
)


def reference_fixed_chunk(text: str, chunk_size: int = 500, overlap: int = 50):
    """Frozen copy of the pre-#38-chunking fixed chunker."""
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
        start += chunk_size - overlap
    return chunks


class DefaultPathUnchangedTests(unittest.TestCase):
    def test_default_records_for_python_match_fixed(self):
        py_path = REPO / "scripts" / "rag" / "chunking.py"
        text = py_path.read_text(encoding="utf-8")
        expected = [
            {
                "id": f"{py_path.name}_{i}",
                "source": str(py_path),
                "filename": py_path.name,
                "chunk_index": i,
                "text": c,
            }
            for i, c in enumerate(reference_fixed_chunk(text))
        ]
        self.assertEqual(chunk_records(py_path, text), expected)
        self.assertEqual(chunk_records(py_path, text, "fixed"), expected)

    def test_heading_chunking_leaves_python_as_fixed(self):
        py_path = REPO / "scripts" / "rag" / "chunking.py"
        text = py_path.read_text(encoding="utf-8")
        self.assertEqual(
            chunk_records(py_path, text, "heading"),
            chunk_records(py_path, text, "fixed"),
        )

    def test_python_chunking_leaves_markdown_as_fixed(self):
        md_path = REPO / "docs" / "04-architecture.md"
        text = md_path.read_text(encoding="utf-8")
        records = chunk_records(md_path, text, "python")
        self.assertEqual(
            [{k: v for k, v in row.items() if k not in ("chunking_status", "fallback_reason")}
             for row in records],
            chunk_records(md_path, text, "fixed"),
        )
        self.assertTrue(all(row["chunking_status"] == "fixed" for row in records))


class DecoratorAndAsyncTests(unittest.TestCase):
    def test_single_and_stacked_function_decorators_preserved(self):
        code = (
            "@dec1\n"
            "@dec2(param='val')\n"
            "def target_func(a: int) -> int:\n"
            "    '''Docstring.'''\n"
            "    return a + 1\n"
        )
        res = chunk_python_source(code, chunk_size=500, overlap=50)
        self.assertTrue(res.structured)
        self.assertIsNone(res.fallback_reason)
        self.assertEqual(len(res), 1)
        self.assertIn("@dec1", res[0])
        self.assertIn("@dec2(param='val')", res[0])
        self.assertIn("def target_func(a: int) -> int:", res[0])

    def test_class_decorator_preserved(self):
        code = (
            "@dataclass\n"
            "class SampleClass:\n"
            "    name: str\n"
            "    val: int = 0\n"
        )
        res = chunk_python_source(code, chunk_size=500, overlap=50)
        self.assertTrue(res.structured)
        self.assertEqual(len(res), 1)
        self.assertIn("@dataclass", res[0])
        self.assertIn("class SampleClass:", res[0])

    def test_async_function_preserved(self):
        code = (
            "async def fetch_data(url: str) -> dict:\n"
            "    '''Asynchronous fetcher.'''\n"
            "    return {'status': 200}\n"
        )
        res = chunk_python_source(code, chunk_size=500, overlap=50)
        self.assertTrue(res.structured)
        self.assertEqual(len(res), 1)
        self.assertTrue(res[0].startswith("async def fetch_data"))

    def test_async_function_with_decorator_preserved(self):
        code = (
            "@retry(limit=3)\n"
            "async def resilient_call():\n"
            "    return True\n"
        )
        res = chunk_python_source(code, chunk_size=500, overlap=50)
        self.assertTrue(res.structured)
        self.assertEqual(len(res), 1)
        self.assertIn("@retry(limit=3)", res[0])
        self.assertIn("async def resilient_call():", res[0])


class NestedDefinitionsAndModuleBodyTests(unittest.TestCase):
    def test_nested_functions_preserved_inside_enclosing_function(self):
        code = (
            "def outer(x: int) -> int:\n"
            "    def inner_helper(y: int) -> int:\n"
            "        return y * 2\n"
            "    return inner_helper(x) + 1\n"
        )
        res = chunk_python_source(code, chunk_size=500, overlap=50)
        self.assertTrue(res.structured)
        self.assertEqual(len(res), 1)
        self.assertIn("def outer", res[0])
        self.assertIn("def inner_helper", res[0])

    def test_nested_class_inside_function_preserved(self):
        code = (
            "def factory():\n"
            "    class Product:\n"
            "        pass\n"
            "    return Product()\n"
        )
        res = chunk_python_source(code, chunk_size=500, overlap=50)
        self.assertTrue(res.structured)
        self.assertEqual(len(res), 1)
        self.assertIn("class Product:", res[0])

    def test_module_body_preamble_and_epilogue_preserved(self):
        code = (
            "# Top header comment\n"
            "'''Module docstring.'''\n"
            "import os\n"
            "import sys\n\n"
            "GLOBAL_CONST = 100\n\n"
            "def run_task():\n"
            "    return GLOBAL_CONST\n\n"
            "if __name__ == '__main__':\n"
            "    sys.exit(run_task())\n"
        )
        res = chunk_python_source(code, chunk_size=500, overlap=50)
        self.assertTrue(res.structured)
        self.assertEqual(len(res), 3)

        # Chunk 0: Module preamble (header comment, docstring, imports, constant)
        self.assertIn("Module docstring", res[0])
        self.assertIn("GLOBAL_CONST = 100", res[0])

        # Chunk 1: Function definition
        self.assertIn("def run_task():", res[1])

        # Chunk 2: Trailing module epilogue
        self.assertIn("if __name__ == '__main__':", res[2])

    def test_interleaved_module_statements_between_functions(self):
        code = (
            "def first():\n"
            "    pass\n\n"
            "# Middle configuration\n"
            "MID_VAR = 'middle'\n\n"
            "def second():\n"
            "    pass\n"
        )
        res = chunk_python_source(code, chunk_size=500, overlap=50)
        self.assertTrue(res.structured)
        self.assertEqual(len(res), 3)
        self.assertIn("def first", res[0])
        self.assertIn("MID_VAR = 'middle'", res[1])
        self.assertIn("def second", res[2])


class LargeFunctionAndClassDecompositionTests(unittest.TestCase):
    def test_large_function_split_within_budget(self):
        body_lines = "\n".join(f"    var_{i} = {i} * 100" for i in range(100))
        code = f"def huge_function():\n{body_lines}\n"
        self.assertGreater(len(code), 1000)

        res = chunk_python_source(code, chunk_size=500, overlap=50)
        self.assertTrue(res.structured)
        self.assertGreater(len(res), 2)
        for chunk in res:
            self.assertLessEqual(len(chunk), 500)
            self.assertTrue(chunk.strip())

    def test_large_class_decomposes_at_method_boundaries(self):
        code = (
            "class ComplexService:\n"
            "    '''Service documentation.'''\n"
            "    CONFIG = {'enabled': True}\n\n"
            "    def method_alpha(self):\n"
            "        '''Alpha method documentation.'''\n"
            "        result = [x * 2 for x in range(20)]\n"
            "        return result\n\n"
            "    def method_beta(self):\n"
            "        '''Beta method documentation.'''\n"
            "        data = {'status': 'ok', 'count': 42}\n"
            "        return data\n"
        )
        # Choose budget small enough that full class (~380 chars) doesn't fit,
        # but preamble and each method fit individually (< 160 chars)
        budget = 160
        res = chunk_python_source(code, chunk_size=budget, overlap=30)
        self.assertTrue(res.structured)
        self.assertGreaterEqual(len(res), 3)

        # First chunk should have class preamble
        self.assertIn("class ComplexService:", res[0])
        self.assertIn("CONFIG = {'enabled': True}", res[0])

        # Subsequent chunks should contain methods intact
        joined = "\n===\n".join(res)
        self.assertIn("def method_alpha", joined)
        self.assertIn("def method_beta", joined)

        for chunk in res:
            self.assertLessEqual(len(chunk), budget)

    def test_large_module_body_split_within_budget(self):
        code = "\n".join(f"GLOBAL_{i} = 'constant_value_{i}'" for i in range(60))
        self.assertGreater(len(code), 1200)
        res = chunk_python_source(code, chunk_size=400, overlap=50)
        self.assertTrue(res.structured)
        self.assertGreater(len(res), 3)
        for chunk in res:
            self.assertLessEqual(len(chunk), 400)


class SyntaxErrorAndD22FallbackTests(unittest.TestCase):
    def test_syntax_error_falls_back_to_fixed_and_records_reason(self):
        bad_code = (
            "def broken_function(:\n"
            "    this is not valid python code\n"
            "    at all\n"
        )
        res = chunk_python_source(bad_code, chunk_size=100, overlap=20)
        # D22: Must NOT claim structure analysis succeeded
        self.assertFalse(res.structured)
        self.assertFalse(res.is_structured)

        # D22: Must explicitly record the fallback reason
        self.assertIsNotNone(res.fallback_reason)
        self.assertIn("SyntaxError", res.fallback_reason)

        # Must fall back to fixed chunking output
        expected_fallback = chunk_text(bad_code, chunk_size=100, overlap=20)
        self.assertEqual(list(res), expected_fallback)

    def test_chunk_records_syntax_error_fallback_contracts(self):
        bad_code = "def syntax_err(:\n    pass\n" * 10
        fpath = Path("/tmp/broken.py")

        records = chunk_records(fpath, bad_code, chunking="python")
        # Must indicate fallback on record list
        self.assertFalse(getattr(records, "structured", True))
        self.assertIn("SyntaxError", getattr(records, "fallback_reason", ""))

        # Must use marker "" (fixed format: broken.py_0, broken.py_1)
        fixed_records = chunk_records(fpath, bad_code, "fixed")
        self.assertEqual([r["id"] for r in records], [r["id"] for r in fixed_records])
        self.assertEqual([r["text"] for r in records], [r["text"] for r in fixed_records])
        for r in records:
            self.assertNotIn("_p", r["id"])


class DeterminismAndIdentityTests(unittest.TestCase):
    def test_deterministic_output_across_repeated_runs(self):
        code = (
            "import os\n\n"
            "@decorator\n"
            "def sample():\n"
            "    return os.getpid()\n"
        )
        run1 = chunk_python_source(code)
        run2 = chunk_python_source(code)
        self.assertEqual(list(run1), list(run2))
        self.assertEqual(run1.structured, run2.structured)
        self.assertEqual(run1.fallback_reason, run2.fallback_reason)

    def test_python_chunk_ids_never_collide_with_fixed_ids(self):
        code = (
            "import math\n\n"
            "def calculate(r: float) -> float:\n"
            "    return math.pi * r ** 2\n"
        )
        fpath = Path("/tmp/math_calc.py")
        fixed_ids = {r["id"] for r in chunk_records(fpath, code, "fixed")}
        python_records = chunk_records(fpath, code, "python")
        python_ids = [r["id"] for r in python_records]

        # No duplicate IDs within python mode
        self.assertEqual(len(python_ids), len(set(python_ids)))
        # Python chunk IDs carry 'p' marker, distinct from fixed IDs
        self.assertFalse(fixed_ids & set(python_ids))
        for cid in python_ids:
            self.assertIn("_p", cid)

    def test_source_chunking_alias_matches_python_mode(self):
        code = "def greet():\n    return 'hello'\n"
        fpath = Path("/tmp/test_alias.py")
        self.assertEqual(
            chunk_records(fpath, code, "source"),
            chunk_records(fpath, code, "python"),
        )

    def test_empty_and_whitespace_input(self):
        self.assertEqual(list(chunk_python_source("")), [])
        self.assertEqual(list(chunk_python_source("   \n\n\t  ")), ["   \n\n\t  "])

    def test_comments_only_file(self):
        code = "# Just comment line 1\n# Just comment line 2\n"
        res = chunk_python_source(code)
        self.assertTrue(res.structured)
        self.assertEqual(len(res), 1)
        self.assertIn("Just comment line 1", res[0])


if __name__ == "__main__":
    unittest.main()
