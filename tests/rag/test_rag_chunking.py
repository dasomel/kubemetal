import sys
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "scripts" / "rag"))
from chunking import chunk_markdown_headings
from eval_retrieval import build_chunks
from rag_host import chunk_records, chunk_text


def reference_chunk_text(text, chunk_size=500, overlap=50):
    """Frozen copy of the pre-#38-chunking fixed chunker; the default path must equal it."""
    chunks, start, text_len = [], 0, len(text)
    while start < text_len:
        end = min(start + chunk_size, text_len)
        chunk = text[start:end].strip()
        if chunk:
            chunks.append(chunk)
        if end >= text_len:
            break
        start += chunk_size - overlap
    return chunks


class DefaultPathUnchanged(unittest.TestCase):
    def test_fixed_chunker_equals_frozen_reference_on_real_docs(self):
        docs = sorted((REPO / "docs").glob("*.md"))
        self.assertGreater(len(docs), 5)
        for path in docs:
            text = path.read_text(encoding="utf-8")
            self.assertEqual(chunk_text(text), reference_chunk_text(text), path.name)

    def test_default_records_match_original_inline_format(self):
        path = REPO / "docs" / "04-architecture.md"
        text = path.read_text(encoding="utf-8")
        expected = [{"id": f"{path.name}_{i}", "source": str(path), "filename": path.name,
                     "chunk_index": i, "text": c}
                    for i, c in enumerate(reference_chunk_text(text))]
        self.assertEqual(chunk_records(path, text), expected)
        self.assertEqual(chunk_records(path, text, "fixed"), expected)


class HeadingChunker(unittest.TestCase):
    def test_nested_headings_carry_path(self):
        body = lambda name: f"body of {name} " + "." * 60  # above chunk_size // 4: no tiny merge
        text = (f"# A\n\n{body('a')}\n\n## B\n\n{body('b')}\n\n### C\n\n{body('c')}\n\n"
                f"## D\n\n{body('d')}\n")
        chunks = chunk_markdown_headings(text, chunk_size=200)
        joined = "\n".join(chunks)
        self.assertIn("[A > B > C]\nbody of c", joined)
        self.assertIn("[A > D]\nbody of d", joined)
        self.assertNotIn("[A > B > D]", joined)  # sibling pops the deeper path

    def test_no_headings_has_no_prefix(self):
        self.assertEqual(chunk_markdown_headings("just a line\n\nanother para"),
                         ["just a line\n\nanother para"])

    def test_empty_and_whitespace_input(self):
        self.assertEqual(chunk_markdown_headings(""), [])
        self.assertEqual(chunk_markdown_headings("\n\n  \n"), [])

    def test_empty_section_dropped_but_kept_in_child_path(self):
        text = "# Parent\n\n## Child\n\n" + "child body " * 12 + "\n"
        chunks = chunk_markdown_headings(text, chunk_size=200)
        self.assertEqual(len(chunks), 1)
        self.assertTrue(chunks[0].startswith("[Parent > Child]\n"))

    def test_fence_with_hash_lines_is_not_split(self):
        fence = "```bash\n# not a heading\n## also not\necho hi\n```"
        text = f"# Real\n\nbefore\n\n{fence}\n\nafter\n"
        chunks = chunk_markdown_headings(text, chunk_size=500)
        self.assertEqual(len(chunks), 1)
        self.assertIn(fence, chunks[0])
        self.assertFalse(any(c.startswith("[Real > not a heading") for c in chunks))

    def test_tilde_fence_and_blank_lines_inside_fence_stay_atomic(self):
        fence = "~~~\nline one\n\n\n# still code\n~~~"
        chunks = chunk_markdown_headings(f"# T\n\n{'x' * 150}\n\n{fence}\n", chunk_size=200)
        self.assertTrue(any(fence in c for c in chunks))

    def test_unterminated_fence_kept_whole(self):
        chunks = chunk_markdown_headings("# T\n\n```\n# code\nmore\n", chunk_size=500)
        self.assertEqual(len(chunks), 1)
        self.assertIn("```\n# code\nmore", chunks[0])

    def test_table_not_split_even_when_over_budget(self):
        table = "\n".join(f"| k{i} | v{i} |" for i in range(40))
        chunks = chunk_markdown_headings(f"# T\n\nlead\n\n{table}\n\ntail\n", chunk_size=120)
        self.assertTrue(any(table in c for c in chunks))  # atomic, over budget by design (D-H2)

    def test_setext_headings_are_not_headings(self):
        text = "Title\n=====\n\nbody\n\nSub\n---\n\nmore"
        chunks = chunk_markdown_headings(text)
        self.assertEqual(len(chunks), 1)
        self.assertFalse(chunks[0].startswith("["))

    def test_hash_without_space_and_indented_code_are_not_headings(self):
        chunks = chunk_markdown_headings("#nospace\n\n    # indented code\n\ntext")
        self.assertFalse(any(c.startswith("[") for c in chunks))

    def test_closing_hashes_and_empty_heading(self):
        chunks = chunk_markdown_headings("## Title ##\n\nbody text here that is long enough" + "!" * 120)
        self.assertTrue(chunks[0].startswith("[Title]\n"))

    def test_tiny_sections_merge_keeping_later_heading_inline(self):
        text = "# A\n\nx\n\n# B\n\ny\n\n# C\n\n" + "z" * 300
        chunks = chunk_markdown_headings(text, chunk_size=400)
        self.assertEqual(len(chunks), 1)
        self.assertEqual(chunks[0].split("\n")[0], "[A]")
        self.assertIn("# B\ny", chunks[0])

    def test_tiny_section_not_merged_when_it_would_overflow(self):
        text = "# A\n\nx\n\n# B\n\n" + "z" * 190
        chunks = chunk_markdown_headings(text, chunk_size=200)
        self.assertEqual(chunks[0], "[A]\nx")
        self.assertTrue(chunks[1].startswith("[B]\n"))

    def test_oversized_section_split_within_max_size(self):
        text = "# Big\n\n" + " ".join(f"word{i}" for i in range(400))
        chunks = chunk_markdown_headings(text, chunk_size=500, overlap=50)
        self.assertGreater(len(chunks), 3)
        for chunk in chunks:
            self.assertLessEqual(len(chunk), 500)
            self.assertTrue(chunk.startswith("[Big]\n"))

    def test_deep_path_prefix_is_capped(self):
        title = "T" * 400
        chunks = chunk_markdown_headings(f"# {title}\n\n" + "body " * 100, chunk_size=500)
        for chunk in chunks:
            self.assertLessEqual(len(chunk), 500)

    def test_deterministic(self):
        text = (REPO / "docs" / "04-architecture.md").read_text(encoding="utf-8")
        self.assertEqual(chunk_markdown_headings(text), chunk_markdown_headings(text))

    def test_real_docs_respect_size_cap_outside_atomic_blocks(self):
        for path in sorted((REPO / "docs").glob("*.md")):
            for chunk in chunk_markdown_headings(path.read_text(encoding="utf-8")):
                self.assertTrue(chunk.strip())
                if len(chunk) > 500:  # only fences / tables may exceed (D-H2)
                    self.assertTrue("```" in chunk or "~~~" in chunk or "|" in chunk, path.name)


class HeadingIdentity(unittest.TestCase):
    def test_heading_ids_never_collide_with_fixed_ids(self):
        path = REPO / "docs" / "04-architecture.md"
        text = path.read_text(encoding="utf-8")
        fixed = {r["id"] for r in chunk_records(path, text, "fixed")}
        heading = [r["id"] for r in chunk_records(path, text, "heading")]
        self.assertEqual(len(heading), len(set(heading)))
        self.assertFalse(fixed & set(heading))

    def test_heading_flag_only_affects_markdown(self):
        py = Path("/tmp/x.py")
        self.assertEqual(chunk_records(py, "# c\n" * 50, "heading"), chunk_records(py, "# c\n" * 50, "fixed"))

    def test_build_chunks_modes(self):
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / "a.md").write_text("# H\n\nbody")
            fixed = build_chunks(tmp, chunking="fixed")
            heading = build_chunks(tmp, chunking="heading")
        self.assertEqual([c["id"] for c in fixed], ["a.md#0"])
        self.assertEqual([c["id"] for c in heading], ["a.md#h0"])
        self.assertEqual(heading[0]["text"], "[H]\nbody")


if __name__ == "__main__":
    unittest.main()
