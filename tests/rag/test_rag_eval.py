import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts" / "rag"))
from eval_retrieval import aggregate, build_chunks, evaluate, ranked_docs, recall_at_k, reciprocal_rank
from rag_host import lexical_search


class MetricTests(unittest.TestCase):
    def test_recall_at_k_exact(self):
        ranked = ["a", "b", "c", "d"]
        self.assertEqual(recall_at_k(ranked, ["b", "d"], 1), 0.0)
        self.assertEqual(recall_at_k(ranked, ["b", "d"], 2), 0.5)
        self.assertEqual(recall_at_k(ranked, ["b", "d"], 4), 1.0)

    def test_reciprocal_rank_exact(self):
        self.assertEqual(reciprocal_rank(["a", "b", "c"], ["c", "b"]), 0.5)
        self.assertEqual(reciprocal_rank(["a", "b"], ["z"]), 0.0)

    def test_ranked_docs_collapses_chunks_keeping_first(self):
        results = [{"filename": "a"}, {"filename": "a"}, {"filename": "b"}, {"filename": "a"}]
        self.assertEqual(ranked_docs(results), ["a", "b"])


class HarnessTests(unittest.TestCase):
    def test_lexical_fixture_corpus_exact_metrics(self):
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / "a.md").write_text("zebra stripes are unique")
            (Path(tmp) / "b.md").write_text("giraffe necks are long")
            (Path(tmp) / "skip.md").write_text("zebra giraffe")
            chunks = build_chunks(tmp, exclude={"skip.md"})
        self.assertEqual([c["id"] for c in chunks], ["a.md#0", "b.md#0"])
        queries = [
            {"id": "q1", "type": "x", "text": "zebra", "relevant": ["a.md"]},
            {"id": "q2", "type": "x", "text": "giraffe", "relevant": ["a.md", "b.md"]},
            {"id": "q3", "type": "y", "text": "unmatched", "relevant": ["a.md"]},
            {"id": "q4", "type": "y", "text": "zebra giraffe", "relevant": ["b.md"]},
        ]
        rows = evaluate(chunks, queries, lexical_search, [1, 2])
        self.assertEqual(rows[0]["ranked"], ["a.md"])
        self.assertEqual(rows[1]["ranked"], ["b.md"])
        self.assertEqual(rows[2]["ranked"], [])
        self.assertEqual(sorted(rows[3]["ranked"]), ["a.md", "b.md"])  # bm25 tie: order unasserted
        table = aggregate(rows, [1, 2])
        # x: q1 r@1=1 rr=1; q2 r@1=.5 r@2=.5 rr=1 -> r@1=.75 r@2=.75 mrr=1
        self.assertEqual(table["x"], {"n": 2, "recall@1": 0.75, "recall@2": 0.75, "mrr": 1.0})
        # y: q3 all 0; q4 r@2=1 -> r@2=.5
        self.assertEqual(table["y"]["recall@2"], 0.5)
        self.assertEqual(table["ALL"]["n"], 4)


if __name__ == "__main__":
    unittest.main()
