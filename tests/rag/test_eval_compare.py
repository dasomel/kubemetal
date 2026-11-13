"""Stdlib deterministic SDK/embedding doubles; never real dense-model evidence."""
import argparse
import contextlib
import io
import json
import math
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts" / "rag"))
import eval_compare as compare
import rag_host

FIXTURE = Path(__file__).resolve().parents[2] / "scripts/rag/eval/fixtures"


class Vector(list):
    def tolist(self):
        return list(self)


class Embedding:
    """Tiny deterministic token-count vectors, only for testing call plumbing."""
    def encode(self, text, **kwargs):
        if isinstance(text, list):
            return [self.encode(t) for t in text]
        return Vector([text.lower().count(t) for t in ("host", "storage", "path", "service")])


class CompareTests(unittest.TestCase):
    def setUp(self):
        self.chunks, self.queries, self.hashes = compare.fixture_snapshot(FIXTURE)

    def response(self, indices=(0,)):
        return {"status": "ok", "resolved_mode": "dense", "rule": "explicit",
                "results": [dict(self.chunks[i]) for i in indices]}

    def test_hand_ranked_metrics_and_duplicate_collapse(self):
        query = dict(id="hand", type="manual", relevant=[self.chunks[1]["filename"],
                                                       self.chunks[2]["filename"]])
        second_chunk = dict(self.chunks[0], id="duplicate-doc_1", chunk_index=1)
        response = self.response((0, 1, 2))
        response["results"].insert(1, second_chunk)
        row = compare.score(response, query, self.chunks + [second_chunk])
        self.assertEqual(row["ranked"], [c["filename"] for c in self.chunks[:3]])
        self.assertEqual(row["recall"][1], 0)
        self.assertEqual(row["recall"][3], 1)
        self.assertEqual(row["rr"], 0.5)
        expected = (1 / math.log2(3) + 1 / math.log2(4)) / (1 + 1 / math.log2(3))
        self.assertAlmostEqual(row["ndcg"][3], expected)

    def test_empty_results_are_measured_zero_not_an_error(self):
        row = compare.score(self.response(()), self.queries[0], self.chunks)
        self.assertEqual(row["rr"], 0)
        self.assertEqual(row["recall"][5], 0)

    def test_errors_unknown_and_missing_metadata_rejected(self):
        bad = [{"status": "error", "error": "dense failed"}, {"status": "ok"}]
        for field in ("filename", "source", "text", "chunk_index", "id"):
            for value in (None, "wrong"):
                response = self.response()
                response["results"][0][field] = value
                bad.append(response)
            response = self.response()
            del response["results"][0][field]
            bad.append(response)
        for response in bad:
            with self.subTest(response=response), self.assertRaises(ValueError):
                compare.score(response, self.queries[0], self.chunks)

    def sdk(self):
        table = Mock()
        table.to_arrow.return_value.to_pylist.return_value = self.chunks
        def search(vector):
            rows = sorted(self.chunks, key=lambda c: sum((a-b)**2 for a, b in
                          zip(Embedding().encode(c["text"]), vector)))
            builder = Mock()
            builder.limit.side_effect = lambda k: Mock(to_list=lambda: rows[:k])
            return builder
        table.search.side_effect = search
        sdk = Mock()
        sdk.connect.return_value.list_tables.return_value = ["fixture"]
        sdk.connect.return_value.open_table.return_value = table
        return sdk, table

    def test_production_all_modes_same_index_schedule_and_raw_recompute(self):
        sdk, table = self.sdk()
        with tempfile.TemporaryDirectory() as db_path, patch.dict(sys.modules, {"lancedb": sdk}):
            args = dict(db_path=db_path, collection="fixture", model="stub", top_k=len(self.chunks))
            cold = lambda request: dict(compare.invoke(request, Embedding()), wall_ms=1)
            result = compare.measure(self.chunks, self.queries, 5, args, Embedding(), cold)
        self.assertEqual(result["status"], "ok")
        self.assertEqual(len(result["raw"]), len(self.queries) * 5 * 5)
        for mode in compare.MODES:
            self.assertEqual(result["report"][mode]["ALL"]["n"], len(self.queries) * 5)
        for query in self.queries:
            first_modes = [r["mode"] for r in result["raw"] if r["query_id"] == query["id"]][::5]
            self.assertEqual(set(first_modes), set(compare.MODES))
        self.assertTrue(all(call.args == ("fixture",) for call in sdk.connect.return_value.open_table.call_args_list))
        self.assertTrue(table.search.called)
        saved = json.loads(json.dumps(result))
        self.assertEqual(compare.summarize(saved["raw"], self.queries, self.chunks), result["report"])

    def test_one_mode_failure_never_overall_success(self):
        def invoke(request, model):
            if request["mode"] == "dense":
                raise RuntimeError("embedding unavailable")
            return dict(response=self.response(()), query_ms=1)
        with patch.object(compare, "invoke", side_effect=invoke):
            result = compare.measure(self.chunks, self.queries[:1], 1, {}, None,
                                     lambda request: dict(response=self.response(())))
        self.assertEqual(result["status"], "error")
        self.assertEqual(result["report"]["dense"]["ALL"]["n"], 0)
        self.assertIn("embedding unavailable", next(r for r in result["raw"] if r["mode"] == "dense")["error"])

    def test_subprocess_failure_and_system_exit_surface(self):
        with patch.object(compare, "invoke", return_value=dict(response=self.response(()), query_ms=1)):
            def cold(request):
                raise SystemExit(1)
            result = compare.measure(self.chunks, self.queries[:1], 1, {}, None, cold)
        self.assertEqual(result["status"], "error")
        self.assertTrue(all(r["status"] == "error" for r in result["raw"]))
        with patch.object(compare.subprocess, "run", return_value=Mock(returncode=1, stderr="failure", stdout="")):
            with self.assertRaisesRegex(RuntimeError, "failure"):
                compare.cold_query({})

    def test_fixture_hashes_stable_and_detect_edits(self):
        import shutil
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "fixture"
            shutil.copytree(FIXTURE, root)
            self.assertEqual(compare.fixture_snapshot(root)[2], self.hashes)
            (root / "docs/loader.py").write_text("changed")
            changed = compare.fixture_snapshot(root)[2]
            self.assertNotEqual(changed["corpus"], self.hashes["corpus"])
            self.assertNotEqual(changed["chunks"], self.hashes["chunks"])
            data = json.loads((root / "queries.json").read_text())
            data["queries"][0]["relevant"] = ["unknown"]
            (root / "queries.json").write_text(json.dumps(data))
            with self.assertRaisesRegex(ValueError, "Invalid qrels"):
                compare.fixture_snapshot(root)

    def test_missing_snapshot_is_unavailable_nonzero_json(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "evidence.json"
            args = argparse.Namespace(fixture=FIXTURE, model_path=Path(directory)/"missing",
                                      output=output, repeats=5)
            self.assertEqual(compare.run(args), 1)
            result = json.loads(output.read_text())
            self.assertEqual(result["status"], "unavailable")
            self.assertEqual(result["raw"], [])
            self.assertIn("FileNotFoundError", result["error"])

    def test_prepare_once_real_table_validation_and_unavailable(self):
        for failure in (None, "table", "dependency", "empty_model", "model_file", "changed_index", "changed_model", "embedding_count"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                model_path = root / "model"
                model_path.mkdir()
                if failure != "empty_model":
                    (model_path / "config.json").write_text("{}")
                if failure == "model_file":
                    model_path = model_path / "config.json"
                args = argparse.Namespace(fixture=FIXTURE, model_path=model_path,
                                          repeats=1, output=root/"evidence.json")
                sdk = Mock()
                table = sdk.connect.return_value.create_table.return_value
                records = []
                def create(*a, **kw):
                    records.extend(kw["data"])
                    return table
                sdk.connect.return_value.create_table.side_effect = create
                table.to_arrow.return_value.to_pylist.side_effect = lambda: [] if failure == "table" else records
                if failure == "changed_index":
                    type(table).version = property(lambda obj: len(records) + table.to_arrow.call_count)
                    # Explicit version mutation after measurement, independent of SDK inputs.
                else:
                    table.version = 7
                sdk.connect.return_value.list_tables.return_value = ["fixture"]
                sdk.connect.return_value.open_table.return_value = table
                table.search.return_value.limit.return_value.to_list.side_effect = lambda: records
                def cold(request):
                    if failure == "changed_model":
                        (model_path / "config.json").write_text("changed")
                    if failure == "changed_index":
                        table.to_arrow()  # simulate a concurrently changed table version
                    return dict(compare.invoke(request, Embedding()), wall_ms=1)
                embedding = Embedding()
                if failure == "embedding_count":
                    embedding.encode = lambda *a, **kw: []
                with patch.dict(sys.modules, {"lancedb": sdk}), \
                        patch.object(compare, "load_model", return_value=embedding) as load, \
                        patch.object(compare, "cold_query", side_effect=cold), \
                        patch.object(compare.importlib.metadata, "version", side_effect=
                                     RuntimeError("missing dependency") if failure == "dependency" else lambda name: "stub"):
                    code = compare.run(args)
                evidence = json.loads(args.output.read_text())
                if failure is None:
                    self.assertEqual(code, 0)
                    self.assertEqual(evidence["status"], "ok")
                    self.assertEqual(evidence["index_version"], 7)
                    self.assertEqual(load.call_count, 1)
                    self.assertEqual(sdk.connect.return_value.create_table.call_count, 1)
                else:
                    self.assertEqual(code, 1)
                    self.assertEqual(evidence["status"], "unavailable")

    def test_mismatched_mode_rejected_for_warm_and_cold(self):
        for cold_wrong in (False, True):
            def invoke(request, model):
                response = self.response(())
                if cold_wrong:
                    response.update(resolved_mode=request["mode"], rule="explicit")
                return dict(response=response, query_ms=1)
            with patch.object(compare, "invoke", side_effect=invoke):
                result = compare.measure(self.chunks, self.queries[:1], 1, {}, None,
                                         lambda request: dict(response=self.response(())))
            self.assertEqual(result["status"], "error")
            self.assertIn("routing metadata", next(r for r in result["raw"] if r["mode"] == "exact")["error"])

    def test_invalid_fixture_and_cli_arguments_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args = argparse.Namespace(fixture=root, model_path=root, repeats=5, output=root/"evidence.json")
            self.assertEqual(compare.run(args), 1)
            self.assertEqual(json.loads(args.output.read_text())["status"], "unavailable")
            (root/"docs").mkdir()
            with self.assertRaisesRegex(ValueError, "nonempty"):
                compare.fixture_snapshot(root)
            (root/"docs/doc.md").write_text("text")
            (root/"queries.json").write_text('{"queries": []}')
            with self.assertRaisesRegex(ValueError, "Empty chunks/queries"):
                compare.fixture_snapshot(root)
        with patch.object(sys, "argv", ["eval_compare.py"]), \
                contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as raised:
            compare.main()
        self.assertEqual(raised.exception.code, 2)

    def test_production_exit_preserves_diagnostic(self):
        def fail(args):
            print('{"status":"error","error":"missing collection"}')
            raise SystemExit(1)
        with patch.object(rag_host, "cmd_query", side_effect=fail):
            with self.assertRaisesRegex(RuntimeError, "missing collection"):
                compare.invoke({}, None)

    def test_local_cpu_model_only_and_worker_load_separation(self):
        sdk = Mock()
        with patch.dict(sys.modules, {"sentence_transformers": sdk}):
            compare.load_model(Path("snapshot"))
        sdk.SentenceTransformer.assert_called_once_with("snapshot", device="cpu",
                                                       local_files_only=True, trust_remote_code=False)
        for mode, should_load in (("exact", False), ("dense", True), ("auto", True)):
            with patch.object(compare, "load_model") as load, \
                    patch.object(compare, "invoke", return_value={"response": self.response(), "query_ms": 2}), \
                    contextlib.redirect_stdout(io.StringIO()) as output:
                compare.worker(dict(query="why is host compute separate", mode=mode, model="stub"))
            self.assertEqual(load.called, should_load)
            self.assertGreaterEqual(json.loads(output.getvalue())["model_load_ms"], 0)


if __name__ == "__main__":
    unittest.main()
