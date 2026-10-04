import argparse
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts" / "rag"))
from rag_host import classify_query, cmd_query


class QueryClassifierTests(unittest.TestCase):
    def test_classifier_rule_table(self):
        cases = [
            # 1. Edge cases: empty and whitespace -> lexical (empty_query)
            ("", "lexical", "empty_query"),
            ("   \t\n  ", "lexical", "empty_query"),

            # 2. Quoted phrases -> lexical (quoted_phrase)
            ('"Apple Silicon"', "lexical", "quoted_phrase"),
            ("'mlx_lm'", "lexical", "quoted_phrase"),
            ('search for "exact phrase" in docs', "lexical", "quoted_phrase"),

            # 3. Path separators (/ and \) -> lexical (path_separator)
            ("scripts/rag/rag_host.py", "lexical", "path_separator"),
            ("/etc/hosts", "lexical", "path_separator"),
            ("docs/01-proposal.md", "lexical", "path_separator"),
            ("C:\\kubemetal\\models", "lexical", "path_separator"),

            # 4. Version strings -> lexical (version_string)
            ("v1.2.3", "lexical", "version_string"),
            ("0.34.0", "lexical", "version_string"),
            ("v2", "lexical", "version_string"),
            ("v1alpha1", "lexical", "version_string"),
            ("4bit", "lexical", "version_string"),
            ("bf16", "lexical", "version_string"),

            # 5. Kubernetes resource names -> lexical (k8s_resource)
            ("pod", "lexical", "k8s_resource"),
            ("deployments", "lexical", "k8s_resource"),
            ("svc", "lexical", "k8s_resource"),
            ("networkpolicies", "lexical", "k8s_resource"),
            ("kubectl get daemonset", "lexical", "k8s_resource"),
            ("serviceaccount", "lexical", "k8s_resource"),

            # 6. Symbol or casing (dot, underscore, camelCase/PascalCase) -> lexical (symbol_or_casing)
            ("validate_retrieval_mode", "lexical", "symbol_or_casing"),
            ("rag_host", "lexical", "symbol_or_casing"),
            ("config.yaml", "lexical", "symbol_or_casing"),
            ("rag.rs", "lexical", "symbol_or_casing"),
            ("camelCase", "lexical", "symbol_or_casing"),
            ("LanceDB", "lexical", "symbol_or_casing"),
            ("SeaweedFS", "lexical", "symbol_or_casing"),

            # 7. Short 1-3 token queries with digits or symbols -> lexical (short_token_with_digit_or_symbol)
            ("port 5001", "lexical", "short_token_with_digit_or_symbol"),
            ("gpu #0", "lexical", "short_token_with_digit_or_symbol"),
            ("k=60", "lexical", "short_token_with_digit_or_symbol"),
            ("error 404", "lexical", "short_token_with_digit_or_symbol"),
            ("D10", "lexical", "short_token_with_digit_or_symbol"),
            ("@theme", "lexical", "short_token_with_digit_or_symbol"),
            ("8080", "lexical", "short_token_with_digit_or_symbol"),

            # 7b. Review fix MED-1: trailing/leading sentence punctuation (? ! : . , ;) is not a
            # symbol, so short natural-language questions keep the dense leg (hybrid).
            # "Error: connection refused" / "MLflow 배포?" carry only sentence punctuation and no
            # digit, '_', in-word '-'/'.'/'/' or camelCase, so nothing marks them identifier-like.
            ("Error: connection refused", "hybrid", "natural_language"),
            # A leading interrogative makes it a question even though D26 carries a digit; the
            # asker wants the D26 explanation (semantic), not only chunks that spell "D26".
            ("what is D26", "hybrid", "natural_language"),
            ("MLflow 배포?", "hybrid", "natural_language"),
            # Korean question tails are the interrogative lead of a head-final language: the id
            # token ("D26이") carries a digit but the asker wants the explanation (issue #38 LOW).
            ("D26이 뭐야?", "hybrid", "natural_language"),
            ("D26이 뭐야", "hybrid", "natural_language"),
            ("D10 브릿지가 뭔가요?", "hybrid", "natural_language"),
            # A bare id or an id with a Korean particle but no question tail stays identifier-like.
            ("D26이", "lexical", "short_token_with_digit_or_symbol"),
            # A single plain all-caps word is a word, not an id (no digit/symbol/casing transition).
            ("README", "hybrid", "natural_language"),
            # In-word hyphen -> identifier-like slug; exact match matters.
            ("mlflow-deployment", "lexical", "short_token_with_digit_or_symbol"),
            # Path (rule 2) and version (rule 4) keep priority over the short-token rule.
            ("v0.2.0", "lexical", "version_string"),
            # Bare all-caps+digit id with no question lead stays identifier-like.
            ("D26", "lexical", "short_token_with_digit_or_symbol"),
            # 'externalname' is a K8s kind (rule 5), so the mixed query stays lexical.
            ("D26 ExternalName", "lexical", "k8s_resource"),
            # Sentence-final period must not trigger the dot/symbol rule.
            ("connection refused.", "hybrid", "natural_language"),

            # 7c. LOW: apostrophes inside a phrase are not quotes; only double quotes or a
            # whole token wrapped in paired single quotes are.
            ("don't stop, it's fine", "hybrid", "natural_language"),
            ("don't it's", "hybrid", "natural_language"),

            # 8. Unicode / Korean queries
            ("애플 실리콘 메모리 최적화 방법", "hybrid", "natural_language"),
            ("통합 메모리 아키텍처 구조", "hybrid", "natural_language"),
            ("메모리_관리", "lexical", "symbol_or_casing"),
            ("메모리#1", "lexical", "short_token_with_digit_or_symbol"),

            # 9. Natural language / multi-word semantic queries -> hybrid (natural_language)
            ("Apple Silicon Metal GPU memory optimization tips for MLX", "hybrid", "natural_language"),
            ("how does thermal pressure affect training pause", "hybrid", "natural_language"),
            ("explain hybrid retrieval with RRF rank fusion", "hybrid", "natural_language"),
            ("colima vz virtiofs performance advantages", "hybrid", "natural_language"),
            ("semantic search", "hybrid", "natural_language"),

            # 10. Very long query (>20 words) -> hybrid (natural_language)
            (
                "In KubeMetal the control plane and compute plane are strictly split such that "
                "Kubernetes manages container workloads in a Colima Linux VM while all Metal MLX compute "
                "execution runs as native macOS host processes because Metal GPU cannot be passed through to Linux VMs.",
                "hybrid",
                "natural_language",
            ),
        ]

        self.assertGreaterEqual(len(cases), 15)
        for query, expected_mode, expected_rule in cases:
            with self.subTest(query=query):
                mode, rule = classify_query(query)
                self.assertEqual(
                    mode,
                    expected_mode,
                    f"Query {query!r} resolved to mode {mode!r} (rule: {rule!r}), expected {expected_mode!r}",
                )
                self.assertEqual(
                    rule,
                    expected_rule,
                    f"Query {query!r} matched rule {rule!r}, expected {expected_rule!r}",
                )


class FakeArrowTable:
    def __init__(self, data):
        self._data = data

    def to_pylist(self):
        return self._data


class FakeLanceTable:
    def __init__(self, data):
        self._data = data

    def to_arrow(self):
        return FakeArrowTable(self._data)

    def search(self, vector):
        # Emulate vector search returning list of matching dicts
        table_self = self
        class QueryBuilder:
            def __init__(self):
                self._k = None

            def limit(self, k):
                self._k = k
                return self

            def to_list(self):
                return [{"id": d["id"], "text": d["text"], "filename": d["filename"],
                         "source": d["source"], "chunk_index": d["chunk_index"],
                         "_distance": 0.15} for d in table_self._data[:self._k]]
        return QueryBuilder()


class FakeLanceDbConnection:
    def __init__(self, table_name, data):
        self.table_name = table_name
        self.data = data

    def list_tables(self):
        return [self.table_name]

    def open_table(self, name):
        if name != self.table_name:
            raise KeyError(f"No table {name}")
        return FakeLanceTable(self.data)


class ResolvedModeReportingTests(unittest.TestCase):
    def setUp(self):
        self.sample_chunks = [
            {
                "id": "config_0",
                "text": "Port forwarding configuration for seaweedfs and mlflow",
                "filename": "config.yaml",
                "source": "config.yaml",
                "chunk_index": 0,
            },
            {
                "id": "mlx_0",
                "text": "Apple Silicon Metal GPU memory optimization tips for MLX",
                "filename": "mlx.md",
                "source": "mlx.md",
                "chunk_index": 0,
            },
        ]

    def test_auto_mode_lexical_reporting_never_calls_dense(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            fake_conn = FakeLanceDbConnection("default", self.sample_chunks)
            fake_lancedb = MagicMock()
            fake_lancedb.connect.return_value = fake_conn

            args = argparse.Namespace(
                command="query",
                db_path=temp_dir,
                collection="default",
                query="config.yaml",
                top_k=2,
                model="mock-embedding-model",
                mode="auto",
            )

            stdout_buf = io.StringIO()
            with patch.dict("sys.modules", {"lancedb": fake_lancedb}), \
                 patch("rag_host.get_embedding_model") as mock_get_model, \
                 patch("sys.stdout", stdout_buf):
                cmd_query(args)

            # Dense embedding model must NOT be called for lexical-classified query in auto mode
            mock_get_model.assert_not_called()

            output = json.loads(stdout_buf.getvalue())
            self.assertEqual(output["status"], "ok")
            self.assertEqual(output["resolved_mode"], "lexical")
            self.assertEqual(output["rule"], "symbol_or_casing")
            self.assertGreater(len(output["results"]), 0)
            for res in output["results"]:
                self.assertEqual(res["mode"], "lexical")
                self.assertEqual(res["resolved_mode"], "lexical")
                self.assertEqual(res["rule"], "symbol_or_casing")

    def test_auto_mode_hybrid_reporting_and_dense_failure_surfaces(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            fake_conn = FakeLanceDbConnection("default", self.sample_chunks)
            fake_lancedb = MagicMock()
            fake_lancedb.connect.return_value = fake_conn

            args = argparse.Namespace(
                command="query",
                db_path=temp_dir,
                collection="default",
                query="Apple Silicon Metal memory optimization",
                top_k=2,
                model="mock-embedding-model",
                mode="auto",
            )

            # When dense fails, auto mode with hybrid routing must FAIL LOUD (D22-D25), never silently fall back
            with patch.dict("sys.modules", {"lancedb": fake_lancedb}), \
                 patch("rag_host.get_embedding_model", side_effect=RuntimeError("sentence-transformers missing")):
                with self.assertRaisesRegex(RuntimeError, "sentence-transformers missing"):
                    cmd_query(args)

    def test_auto_mode_hybrid_reporting_success(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            fake_conn = FakeLanceDbConnection("default", self.sample_chunks)
            fake_lancedb = MagicMock()
            fake_lancedb.connect.return_value = fake_conn

            mock_model = MagicMock()
            mock_model.encode.return_value = MagicMock(tolist=lambda: [0.1, 0.2, 0.3])

            args = argparse.Namespace(
                command="query",
                db_path=temp_dir,
                collection="default",
                query="Apple Silicon Metal memory optimization",
                top_k=2,
                model="mock-embedding-model",
                mode="auto",
            )

            stdout_buf = io.StringIO()
            with patch.dict("sys.modules", {"lancedb": fake_lancedb}), \
                 patch("rag_host.get_embedding_model", return_value=mock_model), \
                 patch("sys.stdout", stdout_buf):
                cmd_query(args)

            output = json.loads(stdout_buf.getvalue())
            self.assertEqual(output["status"], "ok")
            self.assertEqual(output["resolved_mode"], "hybrid")
            self.assertEqual(output["rule"], "natural_language")
            self.assertGreater(len(output["results"]), 0)
            for res in output["results"]:
                self.assertEqual(res["mode"], "hybrid")
                self.assertEqual(res["resolved_mode"], "hybrid")
                self.assertEqual(res["rule"], "natural_language")
                self.assertIn("provenance", res)

    def test_explicit_modes_report_resolved_mode_and_explicit_rule(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            fake_conn = FakeLanceDbConnection("default", self.sample_chunks)
            fake_lancedb = MagicMock()
            fake_lancedb.connect.return_value = fake_conn

            for explicit_mode in ("lexical", "dense", "hybrid"):
                with self.subTest(mode=explicit_mode):
                    mock_model = MagicMock()
                    mock_model.encode.return_value = MagicMock(tolist=lambda: [0.1, 0.2, 0.3])

                    args = argparse.Namespace(
                        command="query",
                        db_path=temp_dir,
                        collection="default",
                        query="test query",
                        top_k=2,
                        model="mock-embedding-model",
                        mode=explicit_mode,
                    )

                    stdout_buf = io.StringIO()
                    with patch.dict("sys.modules", {"lancedb": fake_lancedb}), \
                         patch("rag_host.get_embedding_model", return_value=mock_model), \
                         patch("sys.stdout", stdout_buf):
                        cmd_query(args)

                    output = json.loads(stdout_buf.getvalue())
                    self.assertEqual(output["status"], "ok")
                    self.assertEqual(output["resolved_mode"], explicit_mode)
                    self.assertEqual(output["rule"], "explicit")
                    for res in output["results"]:
                        self.assertEqual(res["resolved_mode"], explicit_mode)
                        self.assertEqual(res["rule"], "explicit")

    def test_auto_mode_zero_hit_still_reports_top_level_routing(self):
        # MED-2: routing must ride at the top level so a search with 0 hits still explains itself.
        with tempfile.TemporaryDirectory() as temp_dir:
            fake_lancedb = MagicMock()
            fake_lancedb.connect.return_value = FakeLanceDbConnection("default", self.sample_chunks)
            args = argparse.Namespace(
                command="query", db_path=temp_dir, collection="default",
                query="zzz_no_such_identifier", top_k=2, model="mock-embedding-model", mode="auto",
            )
            stdout_buf = io.StringIO()
            with patch.dict("sys.modules", {"lancedb": fake_lancedb}), \
                 patch("rag_host.get_embedding_model") as mock_get_model, \
                 patch("sys.stdout", stdout_buf):
                cmd_query(args)

            mock_get_model.assert_not_called()
            output = json.loads(stdout_buf.getvalue())
            self.assertEqual(output["status"], "ok")
            self.assertEqual(output["results"], [])
            self.assertEqual(output["resolved_mode"], "lexical")
            self.assertEqual(output["rule"], "symbol_or_casing")


if __name__ == "__main__":
    unittest.main()
