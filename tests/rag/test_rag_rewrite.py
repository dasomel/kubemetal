"""Unit and regression tests for opt-in local query rewrite (#38 B2). Stdlib only, no sockets."""

import argparse
import contextlib
import decimal
import io
import json
import math
import socket
import sys
import tempfile
import unittest
import urllib.error
import urllib.request
from pathlib import Path
from unittest.mock import MagicMock, Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts" / "rag"))
import rag_host
from rag_host import (
    DEFAULT_REWRITE_MAX_CHARS,
    DEFAULT_REWRITE_MODEL,
    DEFAULT_REWRITE_TIMEOUT_S,
    DEFAULT_REWRITE_URL,
    REWRITE_OUTCOMES,
    _NoRedirectHandler,
    cmd_query,
    default_rewrite_http_client,
    exact_search,
    lexical_search,
    retrieval_score,
    rewrite_query,
    rrf_fuse,
    validate_rewrite_timeout,
    validate_rewrite_url,
)


def openai_response(content):
    return {"choices": [{"message": {"role": "assistant", "content": content}}]}


class FakeClock:
    def __init__(self, steps=0.0):
        if isinstance(steps, (int, float)):
            self._steps = [float(steps)]
        else:
            self._steps = [float(s) for s in steps]
        self._index = 0

    def advance(self, delta_s: float) -> float:
        current = self._steps[min(self._index, len(self._steps) - 1)]
        self._steps = [current + float(delta_s)]
        self._index = 0
        return self._steps[0]

    def __call__(self):
        value = self._steps[min(self._index, len(self._steps) - 1)]
        self._index += 1
        return value


class _FakeSocket:
    def __init__(self, history):
        self._history = history

    def settimeout(self, timeout_s: float):
        self._history.append(timeout_s)


class _FakeRawStream:
    def __init__(self, sock):
        self._sock = sock


class _FakeBufferedFp:
    def __init__(self, sock):
        self.raw = _FakeRawStream(sock)


class FakeStreamingResponse:
    """Socket-free streaming HTTP response stub that drips byte chunks and advances a FakeClock."""

    def __init__(self, chunks, *, clock=None, chunk_delay_s=0.0):
        self._chunks = [bytes(c) for c in chunks]
        self._clock = clock
        self._chunk_delay_s = float(chunk_delay_s)
        self.read_calls = 0
        self.read_sizes = []
        self.timeout_history = []
        self.fp = _FakeBufferedFp(_FakeSocket(self.timeout_history))
        self.closed = False

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        self.close()
        return False

    def read(self, amt=None):
        self.read_calls += 1
        self.read_sizes.append(amt)
        if self._clock is not None and self._chunk_delay_s > 0.0:
            self._clock.advance(self._chunk_delay_s)
        if not self._chunks:
            return b""
        chunk = self._chunks.pop(0)
        if amt is not None and amt >= 0 and len(chunk) > amt:
            head, tail = chunk[:amt], chunk[amt:]
            self._chunks.insert(0, tail)
            return head
        return chunk

    def close(self):
        self.closed = True


class UrlValidationTests(unittest.TestCase):
    def test_accepts_ipv4_loopback_and_normalizes_chat_completions_path(self):
        self.assertEqual(
            validate_rewrite_url("http://127.0.0.1:8080/v1/chat/completions"),
            "http://127.0.0.1:8080/v1/chat/completions",
        )
        self.assertEqual(
            validate_rewrite_url("http://127.0.0.1:8080"),
            "http://127.0.0.1:8080/v1/chat/completions",
        )
        self.assertEqual(
            validate_rewrite_url("http://127.0.0.1:8080/"),
            "http://127.0.0.1:8080/v1/chat/completions",
        )
        self.assertEqual(
            validate_rewrite_url("http://127.0.0.1:8080/v1"),
            "http://127.0.0.1:8080/v1/chat/completions",
        )
        self.assertEqual(
            validate_rewrite_url("http://127.0.0.1:18080/v1/chat/completions"),
            "http://127.0.0.1:18080/v1/chat/completions",
        )

    def test_rejects_localhost_ipv6_non_loopback_and_non_http_urls(self):
        rejected_urls = [
            "",
            "   ",
            None,
            "http://localhost:8080/v1/chat/completions",
            "http://localhost/v1/chat/completions",
            "http://[::1]:8080/v1/chat/completions",
            "http://0.0.0.0:8080/v1/chat/completions",
            "http://127.0.0.2:8080/v1/chat/completions",
            "http://127.0.0.1.:8080/v1/chat/completions",
            "http://192.168.5.2:8080/v1/chat/completions",
            "http://10.0.0.1:8080/v1/chat/completions",
            "http://host.lima.internal:8080/v1/chat/completions",
            "https://127.0.0.1:8080/v1/chat/completions",
            "https://api.openai.com/v1/chat/completions",
            "file:///tmp/socket",
            "ftp://127.0.0.1:8080/v1/chat/completions",
            "http://user:secret@127.0.0.1:8080/v1/chat/completions",
            "http://127.0.0.1:0/v1/chat/completions",
            "http://127.0.0.1:70000/v1/chat/completions",
            "http://127.0.0.1:8080/v1/chat/completions?redirect=1",
            "http://127.0.0.1:8080/v1/chat/completions#frag",
            "http://127.0.0.1:8080/v1/chat completions",
        ]
        for bad_url in rejected_urls:
            with self.subTest(url=bad_url):
                with self.assertRaises(ValueError):
                    validate_rewrite_url(bad_url)
                client = Mock()
                with self.assertRaises(ValueError):
                    rewrite_query(
                        "how does thermal pressure affect training pause",
                        enabled=True,
                        url=bad_url,
                        http_client=client,
                    )
                client.assert_not_called()

    def test_default_http_client_enforces_validation_timeout_and_no_redirect_without_socket(self):
        client_open = MagicMock()
        fake_resp = MagicMock()
        fake_resp.__enter__.return_value.read.side_effect = [
            json.dumps(openai_response("Metal GPU memory")).encode("utf-8"),
            b"",
        ]
        client_open.return_value = fake_resp

        with patch.object(rag_host._REWRITE_OPENER, "open", client_open):
            parsed = default_rewrite_http_client(
                "http://127.0.0.1:8080/v1/chat/completions",
                {"model": "default", "messages": []},
                1.25,
            )
        self.assertEqual(parsed["choices"][0]["message"]["content"], "Metal GPU memory")
        self.assertEqual(client_open.call_args.kwargs["timeout"], 1.25)

        with self.assertRaises(ValueError):
            default_rewrite_http_client("http://localhost:8080/v1/chat/completions", {}, 1.0)

        handler = _NoRedirectHandler()
        req = urllib.request.Request("http://127.0.0.1:8080/v1/chat/completions")
        with self.assertRaises(urllib.error.URLError):
            handler.redirect_request(req, None, 302, "Found", {}, "http://example.com/")

    def test_slow_drip_response_past_deadline_aborts_early_and_reports_timeout(self):
        query = "find models suitable for local VLM inference"
        raw_body = json.dumps(openai_response("Qwen2-VL local VLM inference")).encode("utf-8")
        # Split body into 5 small drips; each drip takes 0.20s against a 0.50s total deadline.
        step = max(1, len(raw_body) // 5)
        drips = [raw_body[i : i + step] for i in range(0, len(raw_body), step)]
        self.assertGreaterEqual(len(drips), 5)

        clock = FakeClock(10.0)
        fake_resp = FakeStreamingResponse(drips, clock=clock, chunk_delay_s=0.20)
        client_open = MagicMock(return_value=fake_resp)

        with patch.object(rag_host._REWRITE_OPENER, "open", client_open):
            trace = rewrite_query(
                query,
                enabled=True,
                timeout_s=0.50,
                clock=clock,
            )

        # Drip 1 (0.20s), Drip 2 (0.40s), Drip 3 (0.60s > 0.50s deadline) -> stops after 3 reads.
        self.assertEqual(fake_resp.read_calls, 3)
        self.assertTrue(fake_resp.closed)
        self.assertEqual(len(fake_resp.timeout_history), 3)
        self.assertAlmostEqual(fake_resp.timeout_history[0], 0.50, places=6)
        self.assertAlmostEqual(fake_resp.timeout_history[1], 0.30, places=6)
        self.assertAlmostEqual(fake_resp.timeout_history[2], 0.10, places=6)
        self.assertEqual(trace["outcome"], "timeout")
        self.assertEqual(trace["reason"], "timeout")
        self.assertEqual(trace["original_query"], query)
        self.assertEqual(trace["effective_query"], query)
        self.assertIsNone(trace["rewritten_query"])
        self.assertAlmostEqual(trace["latency_ms"], 600.0, places=3)

        # If connect + headers alone exhaust the deadline, body read() is never called at all.
        header_clock = FakeClock([20.0, 20.55])
        header_resp = FakeStreamingResponse(drips)
        with patch.object(rag_host._REWRITE_OPENER, "open", MagicMock(return_value=header_resp)):
            header_trace = rewrite_query(
                query,
                enabled=True,
                timeout_s=0.50,
                clock=header_clock,
            )
        self.assertEqual(header_resp.read_calls, 0)
        self.assertTrue(header_resp.closed)
        self.assertEqual(header_trace["outcome"], "timeout")
        self.assertEqual(header_trace["effective_query"], query)

    def test_oversized_body_and_oversized_rewrite_with_fake_response_and_clock(self):
        query = "how does thermal pressure affect training pause"

        # 1. HTTP body exceeds 64 KiB cap -> aborts as soon as cumulative bytes > 65536 (outcome='error').
        clock_err = FakeClock(0.0)
        twenty_chunks = [b"x" * 4096 for _ in range(20)]  # 80 KiB total
        oversized_resp = FakeStreamingResponse(twenty_chunks, clock=clock_err, chunk_delay_s=0.01)
        with patch.object(rag_host._REWRITE_OPENER, "open", MagicMock(return_value=oversized_resp)):
            err_trace = rewrite_query(
                query,
                enabled=True,
                timeout_s=2.0,
                clock=clock_err,
            )
        # 16 * 4096 == 65536; the 17th chunk crosses 64 KiB and aborts immediately without reading 18..20.
        self.assertEqual(oversized_resp.read_calls, 17)
        self.assertTrue(oversized_resp.closed)
        self.assertEqual(err_trace["outcome"], "error")
        self.assertEqual(err_trace["reason"], "request_error")
        self.assertEqual(err_trace["original_query"], query)
        self.assertEqual(err_trace["effective_query"], query)
        self.assertIsNone(err_trace["rewritten_query"])
        self.assertIn("64 KiB", err_trace["error"])

        # 2. HTTP body is within 64 KiB, but rewritten text exceeds max_chars -> outcome='rejected' ('too_long').
        clock_rej = FakeClock(5.0)
        long_rewrite = "thermalState " * 30  # 390 chars > DEFAULT_REWRITE_MAX_CHARS (256)
        encoded_long = json.dumps(openai_response(long_rewrite)).encode("utf-8")
        mid = len(encoded_long) // 2
        long_resp = FakeStreamingResponse(
            [encoded_long[:mid], encoded_long[mid:]],
            clock=clock_rej,
            chunk_delay_s=0.02,
        )
        with patch.object(rag_host._REWRITE_OPENER, "open", MagicMock(return_value=long_resp)):
            rej_trace = rewrite_query(
                query,
                enabled=True,
                timeout_s=2.0,
                max_chars=DEFAULT_REWRITE_MAX_CHARS,
                clock=clock_rej,
            )
        self.assertTrue(long_resp.closed)
        self.assertEqual(rej_trace["outcome"], "rejected")
        self.assertEqual(rej_trace["reason"], "too_long")
        self.assertEqual(rej_trace["original_query"], query)
        self.assertEqual(rej_trace["effective_query"], query)

    def test_normal_chunked_body_within_deadline_applies_rewrite_with_fake_response_and_clock(self):
        query = "저번에 학습했던 qwen 이미지 모델 어디 있지?"
        rewritten = "Qwen2-VL-2B-Instruct-4bit vision fine-tuning model"
        encoded = json.dumps(openai_response(f"  {rewritten}\n")).encode("utf-8")
        chunks = [encoded[:25], encoded[25:55], encoded[55:]]

        clock = FakeClock(100.0)
        fake_resp = FakeStreamingResponse(chunks, clock=clock, chunk_delay_s=0.05)
        with patch.object(rag_host._REWRITE_OPENER, "open", MagicMock(return_value=fake_resp)):
            trace = rewrite_query(
                query,
                enabled=True,
                timeout_s=1.0,
                clock=clock,
            )

        # 3 payload chunks + 1 EOF read = 4 read calls, 0.20s elapsed (< 1.0s deadline).
        self.assertEqual(fake_resp.read_calls, 4)
        self.assertTrue(fake_resp.closed)
        self.assertEqual(trace["outcome"], "applied")
        self.assertEqual(trace["reason"], "applied")
        self.assertEqual(trace["original_query"], query)
        self.assertEqual(trace["rewritten_query"], rewritten)
        self.assertEqual(trace["effective_query"], rewritten)
        self.assertAlmostEqual(trace["latency_ms"], 200.0, places=3)
        self.assertIsNone(trace["error"])

    def test_rejects_non_positive_non_finite_or_non_numeric_timeout_at_config_time(self):
        self.assertEqual(validate_rewrite_timeout(0.25), 0.25)
        self.assertEqual(validate_rewrite_timeout(2), 2.0)

        invalid_timeouts = [
            ("nan", float("nan")),
            ("neg_nan", -float("nan")),
            ("math_nan", math.nan),
            ("pos_inf", float("inf")),
            ("neg_inf", float("-inf")),
            ("math_inf", math.inf),
            ("zero_int", 0),
            ("zero_float", 0.0),
            ("neg_zero_float", -0.0),
            ("negative_int", -1),
            ("negative_float", -0.001),
            ("none", None),
            ("bool_true", True),
            ("bool_false", False),
            ("str_numeric", "2.0"),
            ("str_nan", "nan"),
            ("str_inf", "inf"),
            ("str_alpha", "abc"),
            ("decimal_nan", decimal.Decimal("NaN")),
            ("list", [1.0]),
            ("dict", {"timeout_s": 1.0}),
            ("object", object()),
            ("overflow_int", 10**400),
        ]
        for label, bad_timeout in invalid_timeouts:
            with self.subTest(case=label):
                with self.assertRaisesRegex(ValueError, "positive finite number"):
                    validate_rewrite_timeout(bad_timeout)

                with self.assertRaisesRegex(ValueError, "positive finite number"):
                    default_rewrite_http_client(
                        "http://127.0.0.1:8080/v1/chat/completions",
                        {"model": "default", "messages": []},
                        bad_timeout,
                    )

                for enabled in (True, False):
                    for mode in ("auto", "hybrid", "exact"):
                        custom_client = Mock(
                            side_effect=AssertionError(
                                "http_client must never receive an invalid timeout_s"
                            )
                        )
                        with self.assertRaisesRegex(ValueError, "positive finite number"):
                            rewrite_query(
                                "how does thermal pressure affect training pause",
                                enabled=enabled,
                                mode=mode,
                                timeout_s=bad_timeout,
                                http_client=custom_client,
                            )
                        custom_client.assert_not_called()

                        # Default HTTP client path must also raise ValueError at config time
                        # instead of catching it and returning outcome == "error".
                        with self.assertRaisesRegex(ValueError, "positive finite number"):
                            rewrite_query(
                                "how does thermal pressure affect training pause",
                                enabled=enabled,
                                mode=mode,
                                timeout_s=bad_timeout,
                            )


class RewriteOutcomesAndSkippingTests(unittest.TestCase):
    def test_disabled_by_default_never_calls_http_client(self):
        client = Mock(side_effect=AssertionError("http_client must not be called when disabled"))
        trace = rewrite_query(
            "how does thermal pressure affect training pause",
            http_client=client,
        )
        client.assert_not_called()
        self.assertEqual(trace["outcome"], "disabled")
        self.assertFalse(trace["enabled"])
        self.assertTrue(trace["skipped"])
        self.assertEqual(trace["original_query"], "how does thermal pressure affect training pause")
        self.assertEqual(trace["effective_query"], "how does thermal pressure affect training pause")
        self.assertIsNone(trace["rewritten_query"])
        self.assertIsNone(trace["latency_ms"])

    def test_exact_and_identifier_queries_skip_rewrite_even_when_enabled(self):
        exact_queries = [
            ('"Apple Silicon"', "quoted_phrase"),
            ("'mlx_lm'", "quoted_phrase"),
            ("scripts/rag/rag_host.py", "path_separator"),
            ("C:\\kubemetal\\models", "path_separator"),
            ("v1.2.3", "version_string"),
            ("4bit", "version_string"),
            ("pod", "k8s_resource"),
            ("kubectl get daemonset", "k8s_resource"),
            ("validate_retrieval_mode", "symbol_or_casing"),
            ("config.yaml", "symbol_or_casing"),
            ("SeaweedFS", "symbol_or_casing"),
            ("D26", "short_token_with_digit_or_symbol"),
            ("port 5001", "short_token_with_digit_or_symbol"),
            ("mlflow-deployment", "short_token_with_digit_or_symbol"),
            ("   ", "empty_query"),
        ]
        for query, expected_rule in exact_queries:
            with self.subTest(query=query):
                client = Mock(side_effect=AssertionError("exact query must skip HTTP call"))
                trace = rewrite_query(query, enabled=True, http_client=client)
                client.assert_not_called()
                self.assertEqual(trace["outcome"], "disabled")
                self.assertTrue(trace["enabled"])
                self.assertTrue(trace["skipped"])
                self.assertEqual(trace["reason"], f"exact_query:{expected_rule}")
                self.assertEqual(trace["original_query"], query)
                self.assertEqual(trace["effective_query"], query)
                self.assertIsNone(trace["rewritten_query"])

        # Explicit exact mode also skips natural-language strings without touching HTTP
        client = Mock(side_effect=AssertionError("exact mode must skip HTTP call"))
        trace = rewrite_query(
            "how does thermal pressure affect training pause",
            enabled=True,
            mode="exact",
            http_client=client,
        )
        client.assert_not_called()
        self.assertEqual(trace["outcome"], "disabled")
        self.assertTrue(trace["skipped"])
        self.assertEqual(trace["reason"], "exact_mode")
        self.assertEqual(trace["effective_query"], "how does thermal pressure affect training pause")

    def test_applied_outcome_records_rewritten_and_original_query(self):
        query = "저번에 학습했던 qwen 이미지 모델 어디 있지?"
        rewritten = "Qwen2-VL-2B-Instruct-4bit vision fine-tuning model"
        client = Mock(return_value=openai_response(f"  {rewritten}\n"))
        clock = FakeClock([10.0, 10.018])

        trace = rewrite_query(
            query,
            enabled=True,
            url="http://127.0.0.1:8080",
            model="mlx-community/Qwen2.5-3B-Instruct-4bit",
            timeout_s=2.0,
            http_client=client,
            clock=clock,
        )
        client.assert_called_once()
        called_url, called_payload, called_timeout = client.call_args.args
        self.assertEqual(called_url, "http://127.0.0.1:8080/v1/chat/completions")
        self.assertEqual(called_payload["model"], "mlx-community/Qwen2.5-3B-Instruct-4bit")
        self.assertEqual(called_timeout, 2.0)

        self.assertEqual(trace["outcome"], "applied")
        self.assertIn(trace["outcome"], REWRITE_OUTCOMES)
        self.assertFalse(trace["skipped"])
        self.assertEqual(trace["original_query"], query)
        self.assertEqual(trace["rewritten_query"], rewritten)
        self.assertEqual(trace["effective_query"], rewritten)
        self.assertAlmostEqual(trace["latency_ms"], 18.0, places=3)
        self.assertIsNone(trace["error"])

    def test_timeout_outcome_via_exception_and_fake_clock_preserves_original_query(self):
        query = "find models suitable for local VLM inference"
        timeout_exceptions = [
            TimeoutError("request timed out"),
            socket.timeout("socket timed out"),
            urllib.error.URLError(socket.timeout("urlopen timed out")),
        ]
        for exc in timeout_exceptions:
            with self.subTest(exc= repr(exc)):
                client = Mock(side_effect=exc)
                trace = rewrite_query(
                    query,
                    enabled=True,
                    timeout_s=0.5,
                    http_client=client,
                    clock=FakeClock([1.0, 1.5]),
                )
                self.assertEqual(trace["outcome"], "timeout")
                self.assertEqual(trace["reason"], "timeout")
                self.assertEqual(trace["original_query"], query)
                self.assertEqual(trace["effective_query"], query)
                self.assertIsNone(trace["rewritten_query"])
                self.assertIsNotNone(trace["error"])

        # Fake clock exceeds timeout_s even though callable returned a rewrite candidate
        late_client = Mock(return_value=openai_response("VLM local inference model"))
        late_trace = rewrite_query(
            query,
            enabled=True,
            timeout_s=0.25,
            http_client=late_client,
            clock=FakeClock([5.0, 5.40]),
        )
        self.assertEqual(late_trace["outcome"], "timeout")
        self.assertEqual(late_trace["reason"], "deadline_exceeded")
        self.assertEqual(late_trace["original_query"], query)
        self.assertEqual(late_trace["effective_query"], query)
        self.assertIsNone(late_trace["rewritten_query"])

    def test_error_outcome_on_connection_or_malformed_response_never_reports_applied(self):
        query = "how does thermal pressure affect training pause"
        error_callables = [
            Mock(side_effect=urllib.error.URLError("Connection refused")),
            Mock(side_effect=ConnectionRefusedError("refused")),
            Mock(side_effect=RuntimeError("server crashed")),
            Mock(return_value={}),
            Mock(return_value={"choices": []}),
            Mock(return_value={"choices": [{"message": {"content": None}}]}),
            Mock(return_value=None),
            Mock(return_value=12345),
        ]
        for idx, client in enumerate(error_callables):
            with self.subTest(case=idx):
                trace = rewrite_query(query, enabled=True, http_client=client)
                self.assertEqual(trace["outcome"], "error")
                self.assertNotEqual(trace["outcome"], "applied")
                self.assertEqual(trace["original_query"], query)
                self.assertEqual(trace["effective_query"], query)
                self.assertIsNone(trace["rewritten_query"])
                self.assertTrue(trace["error"])

    def test_rejected_outcomes_empty_too_long_and_unchanged_preserve_original_query(self):
        query = "how does thermal pressure affect training pause"

        # 1. empty
        for empty_text in ("", "   \n\t  "):
            with self.subTest(empty_text=repr(empty_text)):
                trace = rewrite_query(
                    query,
                    enabled=True,
                    http_client=Mock(return_value=openai_response(empty_text)),
                )
                self.assertEqual(trace["outcome"], "rejected")
                self.assertEqual(trace["reason"], "empty")
                self.assertEqual(trace["original_query"], query)
                self.assertEqual(trace["effective_query"], query)
                self.assertIsNone(trace["rewritten_query"])

        # 2. too_long
        long_text = "keyword " * 40  # 320 chars > DEFAULT_REWRITE_MAX_CHARS (256)
        trace_long = rewrite_query(
            query,
            enabled=True,
            max_chars=DEFAULT_REWRITE_MAX_CHARS,
            http_client=Mock(return_value=openai_response(long_text)),
        )
        self.assertEqual(trace_long["outcome"], "rejected")
        self.assertEqual(trace_long["reason"], "too_long")
        self.assertEqual(trace_long["original_query"], query)
        self.assertEqual(trace_long["effective_query"], query)

        # 3. unchanged (exact, extra whitespace, or casing-only difference)
        for same_text in (
            query,
            f"  {query}  \n",
            "How Does Thermal Pressure Affect Training Pause",
        ):
            with self.subTest(same_text=same_text):
                trace_same = rewrite_query(
                    query,
                    enabled=True,
                    http_client=Mock(return_value=openai_response(same_text)),
                )
                self.assertEqual(trace_same["outcome"], "rejected")
                self.assertEqual(trace_same["reason"], "unchanged")
                self.assertEqual(trace_same["original_query"], query)
                self.assertEqual(trace_same["effective_query"], query)


class CmdQueryRewriteIntegrationTests(unittest.TestCase):
    def setUp(self):
        self.chunks = [
            {
                "id": "vlm_0",
                "text": "Qwen2-VL-2B-Instruct-4bit vision fine-tuning adapter guide",
                "filename": "vlm.md",
                "source": "/docs/vlm.md",
                "chunk_index": 0,
            },
            {
                "id": "thermal_0",
                "text": "NSProcessInfo thermalState serious pauses training automatically",
                "filename": "thermal.md",
                "source": "/docs/thermal.md",
                "chunk_index": 0,
            },
        ]

    def _run_cmd_query(self, **overrides):
        table = Mock()
        table.to_arrow.return_value.to_pylist.return_value = self.chunks

        def search(vector):
            # Score higher if vector[0] == 1.0 (vlm) vs vector[1] == 1.0 (thermal)
            ordered = self.chunks if vector[0] >= vector[1] else list(reversed(self.chunks))
            builder = Mock()
            builder.limit.side_effect = lambda k: Mock(
                to_list=lambda: [dict(row, _distance=0.1 * (i + 1)) for i, row in enumerate(ordered[:k])]
            )
            return builder

        table.search.side_effect = search
        sdk = Mock()
        sdk.connect.return_value.list_tables.return_value = ["default"]
        sdk.connect.return_value.open_table.return_value = table

        encoded_queries = []

        def encode(text, **kwargs):
            encoded_queries.append(text)
            vec = [1.0, 0.0] if "Qwen2-VL" in text else [0.0, 1.0]
            return Mock(tolist=lambda: vec)

        embed_model = Mock(encode=Mock(side_effect=encode))

        with tempfile.TemporaryDirectory() as db_path:
            params = {
                "db_path": db_path,
                "collection": "default",
                "query": "저번에 학습했던 이미지 모델 가이드 찾아줘",
                "top_k": 2,
                "model": "mock-embed",
                "mode": "lexical",
            }
            params.update(overrides)
            args = argparse.Namespace(**params)
            output = io.StringIO()
            with patch.dict(sys.modules, {"lancedb": sdk}), \
                    patch.object(rag_host, "get_embedding_model", return_value=embed_model), \
                    contextlib.redirect_stdout(output):
                cmd_query(args)
        return json.loads(output.getvalue()), encoded_queries

    def test_applied_rewrite_is_used_for_lexical_and_hybrid_retrieval(self):
        # Without rewrite, Korean conversational query has 0 lexical overlap with English chunk
        off_resp, _ = self._run_cmd_query(rewrite=False, mode="lexical")
        self.assertEqual(off_resp["rewrite"]["outcome"], "disabled")
        self.assertEqual(off_resp["results"], [])

        # With applied rewrite, rewritten query retrieves vlm.md while preserving original query
        client = Mock(return_value=openai_response("Qwen2-VL-2B-Instruct-4bit vision fine-tuning"))
        on_resp, _ = self._run_cmd_query(
            rewrite=True,
            mode="lexical",
            rewrite_http_client=client,
        )
        self.assertEqual(on_resp["query"], "저번에 학습했던 이미지 모델 가이드 찾아줘")
        self.assertEqual(on_resp["rewrite"]["outcome"], "applied")
        self.assertEqual(
            on_resp["rewrite"]["original_query"],
            "저번에 학습했던 이미지 모델 가이드 찾아줘",
        )
        self.assertEqual(
            on_resp["rewrite"]["effective_query"],
            "Qwen2-VL-2B-Instruct-4bit vision fine-tuning",
        )
        self.assertEqual(len(on_resp["results"]), 1)
        self.assertEqual(on_resp["results"][0]["filename"], "vlm.md")

        # In auto/hybrid mode, dense encoding also receives the rewritten query
        auto_resp, encoded = self._run_cmd_query(
            rewrite=True,
            mode="auto",
            rewrite_http_client=client,
        )
        self.assertEqual(auto_resp["resolved_mode"], "hybrid")
        self.assertEqual(auto_resp["rewrite"]["outcome"], "applied")
        self.assertEqual(encoded, ["Qwen2-VL-2B-Instruct-4bit vision fine-tuning"])
        self.assertEqual(auto_resp["results"][0]["filename"], "vlm.md")

    def test_fallback_outcomes_use_original_query_for_retrieval(self):
        original = "thermalState serious pauses training"
        fallback_clients = [
            ("timeout", Mock(side_effect=TimeoutError("timed out"))),
            ("error", Mock(side_effect=urllib.error.URLError("connection refused"))),
            ("rejected", Mock(return_value=openai_response(""))),
            ("rejected", Mock(return_value=openai_response("x" * 300))),
            ("rejected", Mock(return_value=openai_response(original))),
        ]
        for expected_outcome, client in fallback_clients:
            with self.subTest(outcome=expected_outcome, client=client):
                resp, encoded = self._run_cmd_query(
                    query=original,
                    mode="hybrid",
                    rewrite=True,
                    rewrite_http_client=client,
                )
                self.assertEqual(resp["rewrite"]["outcome"], expected_outcome)
                self.assertEqual(resp["query"], original)
                self.assertEqual(resp["rewrite"]["original_query"], original)
                self.assertEqual(resp["rewrite"]["effective_query"], original)
                self.assertEqual(encoded, [original])
                self.assertEqual(resp["results"][0]["filename"], "thermal.md")

    def test_exact_query_in_cmd_query_skips_rewrite_and_keeps_original(self):
        client = Mock(side_effect=AssertionError("HTTP client must not be called for exact query"))
        resp, _ = self._run_cmd_query(
            query="vlm.md",
            mode="auto",
            rewrite=True,
            rewrite_http_client=client,
        )
        client.assert_not_called()
        self.assertEqual(resp["resolved_mode"], "lexical")
        self.assertEqual(resp["rule"], "symbol_or_casing")
        self.assertEqual(resp["rewrite"]["outcome"], "disabled")
        self.assertTrue(resp["rewrite"]["skipped"])
        self.assertEqual(resp["results"][0]["filename"], "vlm.md")

    def test_rewrite_off_retrieval_results_are_byte_identical_to_pre_rewrite_baseline(self):
        cases = [
            ("exact", "vlm.md"),
            ("lexical", "thermalState serious pauses"),
            ("dense", "thermalState serious pauses"),
            ("hybrid", "thermalState serious pauses"),
            ("auto", "vlm.md"),
            ("auto", "thermalState serious pauses training"),
        ]
        for mode, query in cases:
            with self.subTest(mode=mode, query=query):
                resp, _ = self._run_cmd_query(query=query, mode=mode)
                self.assertEqual(resp["rewrite"]["outcome"], "disabled")

                # Recompute baseline results directly using the pre-rewrite functions
                resolved_mode, rule_name = (
                    rag_host.classify_query(query) if mode == "auto" else (mode, "explicit")
                )
                dense_hits = [
                    dict(row, _distance=0.1 * (i + 1))
                    for i, row in enumerate(reversed(self.chunks))
                ]
                if resolved_mode == "exact":
                    raw_hits = exact_search(self.chunks, query, 2)
                elif resolved_mode == "lexical":
                    raw_hits = lexical_search(self.chunks, query, 2)
                elif resolved_mode == "dense":
                    raw_hits = dense_hits[:2]
                else:
                    lex_hits = lexical_search(self.chunks, query, 50)
                    raw_hits = rrf_fuse(lex_hits, dense_hits, 2)

                expected_results = []
                for r in raw_hits:
                    item = {
                        "id": r.get("id"),
                        "text": r.get("text"),
                        "filename": r.get("filename"),
                        "source": r.get("source"),
                        "chunk_index": r.get("chunk_index", 0),
                        "score": retrieval_score(r),
                        "mode": r.get("mode", resolved_mode),
                        "resolved_mode": resolved_mode,
                        "rule": rule_name,
                    }
                    if "provenance" in r:
                        item["provenance"] = r["provenance"]
                    expected_results.append(item)

                self.assertEqual(
                    json.dumps(resp["results"], sort_keys=True, ensure_ascii=False).encode("utf-8"),
                    json.dumps(expected_results, sort_keys=True, ensure_ascii=False).encode("utf-8"),
                )

    def test_cli_flags_default_off_and_accept_opt_in_rewrite_options(self):
        with patch.object(sys, "argv", ["rag_host.py", "query", "--query", "hello world"]), \
                patch.object(rag_host, "cmd_query") as mocked_query:
            rag_host.main()
            parsed = mocked_query.call_args.args[0]
            self.assertFalse(parsed.rewrite)
            self.assertEqual(parsed.rewrite_url, DEFAULT_REWRITE_URL)
            self.assertEqual(parsed.rewrite_model, DEFAULT_REWRITE_MODEL)
            self.assertEqual(parsed.rewrite_timeout, DEFAULT_REWRITE_TIMEOUT_S)
            self.assertEqual(parsed.rewrite_max_chars, DEFAULT_REWRITE_MAX_CHARS)

        with patch.object(
            sys,
            "argv",
            [
                "rag_host.py",
                "query",
                "--query",
                "hello world",
                "--rewrite",
                "--rewrite-url",
                "http://127.0.0.1:18080/v1/chat/completions",
                "--rewrite-model",
                "local-mlx",
                "--rewrite-timeout",
                "0.75",
                "--rewrite-max-chars",
                "128",
            ],
        ), patch.object(rag_host, "cmd_query") as mocked_query:
            rag_host.main()
            parsed = mocked_query.call_args.args[0]
            self.assertTrue(parsed.rewrite)
            self.assertEqual(parsed.rewrite_url, "http://127.0.0.1:18080/v1/chat/completions")
            self.assertEqual(parsed.rewrite_model, "local-mlx")
            self.assertEqual(parsed.rewrite_timeout, 0.75)
            self.assertEqual(parsed.rewrite_max_chars, 128)

        for bad_cli_timeout in ("nan", "NaN", "inf", "-inf", "0", "0.0", "-1", "-0.25", "abc"):
            with self.subTest(cli_timeout=bad_cli_timeout):
                err_buf = io.StringIO()
                with patch.object(
                    sys,
                    "argv",
                    [
                        "rag_host.py",
                        "query",
                        "--query",
                        "hello world",
                        f"--rewrite-timeout={bad_cli_timeout}",
                    ],
                ), patch.object(rag_host, "cmd_query") as mocked_query, contextlib.redirect_stderr(
                    err_buf
                ):
                    with self.assertRaises(SystemExit) as ctx:
                        rag_host.main()
                    self.assertEqual(ctx.exception.code, 2)
                    mocked_query.assert_not_called()
                    self.assertIn("positive finite number", err_buf.getvalue())


if __name__ == "__main__":
    unittest.main()
