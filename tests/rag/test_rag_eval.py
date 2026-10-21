import math
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts" / "rag"))
from eval_retrieval import (aggregate, build_chunks, evaluate, ndcg_at_k,
                           percentile_nearest_rank, ranked_docs, recall_at_k, reciprocal_rank)
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

    def test_ndcg_penalizes_late_and_missing_relevant_documents(self):
        self.assertEqual(ndcg_at_k(["a", "b"], ["a", "b"], 2), 1.0)
        self.assertAlmostEqual(ndcg_at_k(["x", "a", "b"], ["a", "b"], 3),
                               (1 / math.log2(3) + 0.5) / (1 + 1 / math.log2(3)))
        self.assertAlmostEqual(ndcg_at_k(["a"], ["a", "b"], 3),
                               1 / (1 + 1 / math.log2(3)))
        self.assertEqual(ndcg_at_k([], ["a"], 3), 0.0)
        self.assertEqual(ndcg_at_k(["a"], [], 3), 0.0)
        self.assertEqual(ndcg_at_k(["x", "a"], ["a"], 1), 0.0)

    def test_latency_percentiles_use_observed_nearest_rank(self):
        self.assertEqual(percentile_nearest_rank([4, 1, 3, 2], 0.5), 2)
        self.assertEqual(percentile_nearest_rank([4, 1, 3, 2], 0.95), 4)
        self.assertEqual(percentile_nearest_rank([7], 0.95), 7)

    def test_ranked_docs_collapses_chunks_keeping_first(self):
        results = [{"filename": "a"}, {"filename": "a"}, {"filename": "b"}, {"filename": "a"}]
        self.assertEqual(ranked_docs(results), ["a", "b"])


class HarnessTests(unittest.TestCase):
    def test_latency_measures_search_before_rank_processing(self):
        def search(chunks, query, limit):
            clock.assert_called_once()
            return [{"filename": "a"}]

        with patch("eval_retrieval.time.perf_counter_ns", side_effect=[1_000_000, 3_500_000]) as clock:
            rows = evaluate([], [{"id": "q", "type": "x", "text": "a", "relevant": ["a"]}],
                            search, [1])
        self.assertEqual(rows[0]["latency_ms"], 2.5)
        self.assertEqual(aggregate(rows, [1])["ALL"]["latency_p95_ms"], 2.5)

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
        self.assertEqual({k: table["x"][k] for k in ("n", "recall@1", "recall@2", "mrr")},
                         {"n": 2, "recall@1": 0.75, "recall@2": 0.75, "mrr": 1.0})
        self.assertEqual(table["x"]["ndcg@1"], 1.0)
        self.assertAlmostEqual(table["x"]["ndcg@2"], (1 + 1 / (1 + 1 / math.log2(3))) / 2)
        self.assertGreaterEqual(table["x"]["latency_p50_ms"], 0)
        self.assertGreaterEqual(table["x"]["latency_p95_ms"], table["x"]["latency_p50_ms"])
        # y: q3 all 0; q4 r@2=1 -> r@2=.5
        self.assertEqual(table["y"]["recall@2"], 0.5)
        self.assertEqual(table["ALL"]["n"], 4)


if __name__ == "__main__":
    unittest.main()
