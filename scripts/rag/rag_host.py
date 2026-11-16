#!/usr/bin/env python3
"""
KubeMetal Local RAG & DVC Helper Script (Phase 4c)
Handles document chunking, embedding generation, LanceDB indexing/querying,
and DVC dataset versioning with SeaweedFS S3 remote.
"""

import argparse
import http.client
import json
import math
import os
import re
import shutil
import socket
import sys
import subprocess
import sqlite3
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

RRF_K = 60
HYBRID_CANDIDATE_COUNT = 50
DEFAULT_REWRITE_URL = "http://127.0.0.1:8080/v1/chat/completions"
DEFAULT_REWRITE_MODEL = "default"
DEFAULT_REWRITE_TIMEOUT_S = 2.0
DEFAULT_REWRITE_MAX_CHARS = 256
REWRITE_MAX_RESPONSE_BYTES = 65536
REWRITE_READ_CHUNK_BYTES = 4096
REWRITE_PROMPT_VERSION = "v1"
REWRITE_OUTCOMES = ("disabled", "applied", "timeout", "error", "rejected")
REWRITE_SYSTEM_PROMPT = (
    "Rewrite the user search query into concise retrieval keywords for KubeMetal technical "
    "documentation and code search. Preserve exact identifiers, model names, ports, decision "
    "IDs (e.g. D1, D10, D26), and filenames verbatim. Output only the rewritten query text "
    "on a single line with no quotes or explanation."
)

K8S_RESOURCE_NAMES = {
    "pod", "pods", "service", "services", "svc", "deployment", "deployments",
    "deploy", "daemonset", "daemonsets", "ds", "statefulset", "statefulsets",
    "sts", "configmap", "configmaps", "cm", "secret", "secrets", "ingress",
    "ingresses", "ing", "namespace", "namespaces", "ns", "node", "nodes",
    "no", "persistentvolume", "persistentvolumes", "pv", "persistentvolumeclaim",
    "persistentvolumeclaims", "pvc", "serviceaccount", "serviceaccounts", "sa",
    "crd", "crds", "customresourcedefinition", "customresourcedefinitions",
    "job", "jobs", "cronjob", "cronjobs", "cj", "networkpolicy", "networkpolicies",
    "netpol", "endpointslice", "endpointslices", "endpoints", "ep",
    "k8s", "kubectl", "kubelet", "k3s", "externalname",
}

_VERSION_RE = re.compile(
    r"\b(v\d+(?:alpha\d+|beta\d+)?|\d+\.\d+(?:\.\d+)*[a-zA-Z0-9_\-]*|[0-9]+-?bit|bf16|fp16)\b",
    re.IGNORECASE,
)
_CAMEL_CASE_RE = re.compile(r"\b[a-zA-Z]*[a-z][A-Z][a-zA-Z0-9]*\b")
# Only double quotes (anywhere) or a whole query wrapped in paired single quotes count as a
# quoted phrase; an apostrophe inside "don't ... it's" is just an apostrophe.
_QUOTED_RE = re.compile(r'"[^"]+"|\'[^\']+\'')
# Sentence punctuation and wrappers at a token's edge are prose, not identifier symbols.
_EDGE_PUNCT = "?!:;.,()[]{}\"'`"
_QUESTION_LEADS = {
    "what", "why", "how", "when", "where", "which", "who", "whom", "whose",
    "is", "are", "does", "do", "did", "can", "should", "explain", "describe",
}
# Korean is head-final: the interrogative closes the query ("D26이 뭐야?"), so it is matched
# as a trailing token. Closed set of whole tokens (not substrings) to keep routing explainable.
_KOREAN_QUESTION_TAILS = {
    "뭐야", "뭐지", "뭐냐", "뭐예요", "뭐에요", "뭔가", "뭔가요", "뭔데", "무엇", "무엇인가", "무엇인가요",
    "무엇이야", "무엇이에요", "어떻게", "왜", "어떤가요", "어때", "어때요",
}


def classify_query(query: str) -> tuple[str, str]:
    """Deterministic, local, explainable rule classifier for RAG auto mode.

    Maps identifier-like queries to 'lexical' and natural-language / semantic
    queries to 'hybrid'. Returns (resolved_mode, rule_name).
    """
    if not query or not query.strip():
        return ("lexical", "empty_query")

    trimmed = query.strip()

    # 1. Quoted phrases -> exact lexical match
    if _QUOTED_RE.fullmatch(trimmed) or ('"' in trimmed and _QUOTED_RE.search(trimmed)):
        return ("lexical", "quoted_phrase")

    # 2. Path separators (/ or \\) -> filepath or URI identifier
    if "/" in trimmed or "\\" in trimmed:
        return ("lexical", "path_separator")

    raw_tokens = trimmed.split()

    # 3. Multi-word natural-language / semantic queries (> 3 tokens) without path/quotes
    if len(raw_tokens) > 3:
        return ("hybrid", "natural_language")

    # Edge-stripped tokens: "Error:" / "배포?" / "refused." lose their sentence punctuation.
    stripped = [t.strip(_EDGE_PUNCT) for t in raw_tokens]

    # 4. Version strings (v1.2.3, 0.34.0, v2, 4bit, bf16)
    if _VERSION_RE.search(trimmed):
        return ("lexical", "version_string")

    # 5. Kubernetes resource names / kinds (for short 1-3 token queries)
    if any(w.lower() in K8S_RESOURCE_NAMES for w in stripped if w):
        return ("lexical", "k8s_resource")

    # 6. A leading interrogative is a question even when it names an id ("what is D26").
    if stripped and stripped[0].lower() in _QUESTION_LEADS:
        return ("hybrid", "natural_language")

    # 6b. A trailing Korean interrogative ("D26이 뭐야?") is a question for the same reason.
    if any(t in _KOREAN_QUESTION_TAILS for t in stripped[1:]):
        return ("hybrid", "natural_language")

    # 7. Identifier symbols or casing: underscore, in-word dot, or camelCase/PascalCase
    if any("_" in t or "." in t or _CAMEL_CASE_RE.search(t) for t in stripped):
        return ("lexical", "symbol_or_casing")

    # 8. Short 1-3 token queries with a digit or a symbol inside a token (D26, k=60,
    # mlflow-deployment, @theme). Apostrophes are prose; edge punctuation was stripped above.
    if any(c.isdigit() or not (c.isalpha() or c == "'" or c == "\u2019") for t in stripped for c in t):
        return ("lexical", "short_token_with_digit_or_symbol")

    # 9. Otherwise: 1-3 token semantic query without symbols/identifiers
    return ("hybrid", "natural_language")


class _NoRedirectHandler(urllib.request.HTTPRedirectHandler):
    """D-R1: refuse redirects on the local rewrite path so a loopback endpoint cannot
    bounce a request to a non-loopback host. Cost: no redirect support; pass the final
    127.0.0.1 URL directly."""

    def redirect_request(self, req, fp, code, msg, hdrs, newurl):
        raise urllib.error.URLError(f"Redirect blocked ({code}) for local rewrite endpoint")


_REWRITE_OPENER = urllib.request.build_opener(_NoRedirectHandler)


def validate_rewrite_url(url: str) -> str:
    """Allow only http://127.0.0.1[:port] OpenAI-compatible endpoints (D1/D21).

    Rejects localhost (resolves to ::1 on macOS), IPv6 loopback, 0.0.0.0, other
    127.x addresses, LAN/remote hosts, non-http schemes, credentials, and query/fragment.
    """
    if not isinstance(url, str) or not url.strip():
        raise ValueError("Rewrite URL must be a non-empty string.")
    trimmed = url.strip()
    if any(ch.isspace() for ch in trimmed):
        raise ValueError("Rewrite URL must not contain whitespace.")
    parsed = urllib.parse.urlparse(trimmed)
    if parsed.scheme != "http":
        raise ValueError(f"Rewrite URL must use http://127.0.0.1 (got scheme {parsed.scheme!r}).")
    if parsed.username is not None or parsed.password is not None:
        raise ValueError("Rewrite URL must not include credentials.")
    if parsed.hostname != "127.0.0.1":
        raise ValueError(
            f"Rewrite URL must target 127.0.0.1 only (never localhost or non-loopback; got {parsed.hostname!r})."
        )
    try:
        port = parsed.port
    except ValueError as exc:
        raise ValueError(f"Invalid port in rewrite URL: {trimmed!r}") from exc
    if port is None:
        port = 8080
        expected_netloc = "127.0.0.1"
    else:
        if not (1 <= port <= 65535):
            raise ValueError(f"Rewrite URL port out of range: {port}")
        expected_netloc = f"127.0.0.1:{port}"
    if parsed.netloc != expected_netloc:
        raise ValueError(f"Rewrite URL netloc must be {expected_netloc!r}, got {parsed.netloc!r}.")
    if parsed.params or parsed.query or parsed.fragment:
        raise ValueError("Rewrite URL must not contain params, query string, or fragment.")
    path = parsed.path or "/v1/chat/completions"
    if path == "/":
        path = "/v1/chat/completions"
    elif path.rstrip("/") == "/v1":
        path = "/v1/chat/completions"
    return f"http://127.0.0.1:{port}{path}"


def validate_rewrite_timeout(timeout_s: float) -> float:
    """Validate rewrite timeout_s as a positive finite number (int or float, excluding bool)."""
    if (
        timeout_s is None
        or isinstance(timeout_s, bool)
        or not isinstance(timeout_s, (int, float))
    ):
        raise ValueError("rewrite timeout_s must be a positive finite number.")
    try:
        value = float(timeout_s)
    except (TypeError, ValueError, OverflowError) as exc:
        raise ValueError("rewrite timeout_s must be a positive finite number.") from exc
    if not math.isfinite(value) or value <= 0.0:
        raise ValueError("rewrite timeout_s must be a positive finite number.")
    return value


def _parse_rewrite_timeout_arg(raw: str) -> float:
    """Argparse type validator for --rewrite-timeout."""
    try:
        parsed = float(raw)
    except (TypeError, ValueError, OverflowError) as exc:
        raise argparse.ArgumentTypeError("rewrite timeout_s must be a positive finite number.") from exc
    try:
        return validate_rewrite_timeout(parsed)
    except ValueError as exc:
        raise argparse.ArgumentTypeError(str(exc)) from exc


def _set_response_socket_timeout(response, timeout_s: float) -> None:
    """Tighten the underlying socket timeout to the remaining wall-clock budget."""
    clamped = max(0.001, float(timeout_s))
    settimeout = getattr(response, "settimeout", None)
    if callable(settimeout):
        settimeout(clamped)
        return
    fp = getattr(response, "fp", None)
    raw = getattr(fp, "raw", None) if fp is not None else None
    sock = getattr(raw, "_sock", None) if raw is not None else None
    sock_settimeout = getattr(sock, "settimeout", None) if sock is not None else None
    if callable(sock_settimeout):
        sock_settimeout(clamped)


def _read_response_chunk(response, chunk_size: int) -> bytes:
    """Read at most one bounded chunk; use single-syscall read1 on stdlib HTTPResponse."""
    if isinstance(response, http.client.HTTPResponse):
        return response.read1(chunk_size)
    return response.read(chunk_size)


def default_rewrite_http_client(
    endpoint: str,
    payload: dict,
    timeout_s: float,
    *,
    clock=time.perf_counter,
    started_at: float | None = None,
) -> dict:
    """POST an OpenAI chat-completions request to a validated 127.0.0.1 endpoint.

    D-R3: enforce a monotonic wall-clock deadline across connect/headers and bounded
    4 KiB body reads (capped at 64 KiB) rather than a static per-socket-op timeout, so
    a slow-drip response cannot hold the request past the deadline beyond one in-flight
    socket read operation.
    """
    validated_url = validate_rewrite_url(endpoint)
    validated_timeout_s = validate_rewrite_timeout(timeout_s)
    start = clock() if started_at is None else float(started_at)
    deadline = start + validated_timeout_s
    body = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        validated_url,
        data=body,
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    chunks: list[bytes] = []
    total_bytes = 0
    with _REWRITE_OPENER.open(request, timeout=validated_timeout_s) as response:
        remaining = deadline - clock()
        if remaining <= 0.0:
            raise TimeoutError(
                f"Rewrite request exceeded deadline of {validated_timeout_s}s after headers."
            )
        while True:
            _set_response_socket_timeout(response, remaining)
            chunk = _read_response_chunk(response, REWRITE_READ_CHUNK_BYTES)
            if not isinstance(chunk, (bytes, bytearray)):
                raise ValueError(
                    f"Rewrite response read must return bytes, got {type(chunk).__name__}."
                )
            remaining = deadline - clock()
            if not chunk:
                if remaining < 0.0:
                    raise TimeoutError(
                        f"Rewrite request exceeded deadline of {validated_timeout_s}s."
                    )
                break
            if remaining <= 0.0:
                raise TimeoutError(
                    f"Rewrite request exceeded deadline of {validated_timeout_s}s during body read."
                )
            total_bytes += len(chunk)
            if total_bytes > REWRITE_MAX_RESPONSE_BYTES:
                raise ValueError("Rewrite response body exceeds 64 KiB limit.")
            chunks.append(bytes(chunk))

    raw_bytes = b"".join(chunks)
    return json.loads(raw_bytes.decode("utf-8"))


def _is_timeout_error(exc: BaseException) -> bool:
    if isinstance(exc, (TimeoutError, socket.timeout)):
        return True
    if isinstance(exc, urllib.error.URLError) and isinstance(exc.reason, (TimeoutError, socket.timeout)):
        return True
    return False


def _extract_rewrite_content(raw_response) -> str:
    if isinstance(raw_response, str):
        return raw_response
    if not isinstance(raw_response, dict):
        raise ValueError(f"Expected dict or str from rewrite client, got {type(raw_response).__name__}.")
    choices = raw_response.get("choices")
    if not isinstance(choices, list) or not choices:
        raise ValueError("Rewrite response missing non-empty 'choices' list.")
    first = choices[0]
    if not isinstance(first, dict):
        raise ValueError("Rewrite response choice is not an object.")
    message = first.get("message")
    if not isinstance(message, dict):
        raise ValueError("Rewrite response choice missing 'message' object.")
    content = message.get("content")
    if not isinstance(content, str):
        raise ValueError("Rewrite response message 'content' must be a string.")
    return content


def rewrite_query(
    query: str,
    *,
    enabled: bool = False,
    url: str = DEFAULT_REWRITE_URL,
    model: str = DEFAULT_REWRITE_MODEL,
    timeout_s: float = DEFAULT_REWRITE_TIMEOUT_S,
    max_chars: int = DEFAULT_REWRITE_MAX_CHARS,
    mode: str = "auto",
    preclassified: tuple[str, str] | None = None,
    http_client=None,
    clock=time.perf_counter,
) -> dict:
    """Opt-in local query rewrite with bounded timeout and honest fallback (#38 B2).

    D-R2: outcomes are a closed set ('disabled', 'applied', 'timeout', 'error', 'rejected').
    Every non-applied outcome preserves and returns `original_query` as `effective_query`
    without inventing a rewrite (D22). Exact/identifier queries classified by `classify_query`
    as 'lexical' (or explicit `--mode exact`) skip the model call completely.
    """
    if url != DEFAULT_REWRITE_URL or enabled:
        endpoint = validate_rewrite_url(url)
    else:
        endpoint = None

    validated_timeout_s = validate_rewrite_timeout(timeout_s)
    if max_chars is None or isinstance(max_chars, bool) or not isinstance(max_chars, int) or max_chars < 1:
        raise ValueError("rewrite max_chars must be a positive integer.")

    if not enabled:
        return {
            "enabled": False,
            "outcome": "disabled",
            "reason": "disabled",
            "skipped": True,
            "original_query": query,
            "effective_query": query,
            "rewritten_query": None,
            "endpoint": None,
            "model": None,
            "prompt_version": REWRITE_PROMPT_VERSION,
            "timeout_s": None,
            "latency_ms": None,
            "error": None,
        }

    if mode == "exact":
        return {
            "enabled": True,
            "outcome": "disabled",
            "reason": "exact_mode",
            "skipped": True,
            "original_query": query,
            "effective_query": query,
            "rewritten_query": None,
            "endpoint": endpoint,
            "model": model,
            "prompt_version": REWRITE_PROMPT_VERSION,
            "timeout_s": validated_timeout_s,
            "latency_ms": None,
            "error": None,
        }

    classified_mode, classified_rule = (
        preclassified if preclassified is not None else classify_query(query)
    )
    if classified_mode == "lexical":
        return {
            "enabled": True,
            "outcome": "disabled",
            "reason": f"exact_query:{classified_rule}",
            "skipped": True,
            "original_query": query,
            "effective_query": query,
            "rewritten_query": None,
            "endpoint": endpoint,
            "model": model,
            "prompt_version": REWRITE_PROMPT_VERSION,
            "timeout_s": validated_timeout_s,
            "latency_ms": None,
            "error": None,
        }

    payload = {
        "model": model,
        "messages": [
            {"role": "system", "content": REWRITE_SYSTEM_PROMPT},
            {"role": "user", "content": query},
        ],
        "temperature": 0.0,
        "max_tokens": 64,
        "stream": False,
    }
    started = clock()
    try:
        if http_client is None:
            raw_response = default_rewrite_http_client(
                endpoint,
                payload,
                validated_timeout_s,
                clock=clock,
                started_at=started,
            )
        else:
            raw_response = http_client(endpoint, payload, validated_timeout_s)
        elapsed_s = max(0.0, clock() - started)
    except Exception as exc:
        elapsed_s = max(0.0, clock() - started)
        is_timeout = _is_timeout_error(exc) or elapsed_s > validated_timeout_s
        return {
            "enabled": True,
            "outcome": "timeout" if is_timeout else "error",
            "reason": "timeout" if is_timeout else "request_error",
            "skipped": False,
            "original_query": query,
            "effective_query": query,
            "rewritten_query": None,
            "endpoint": endpoint,
            "model": model,
            "prompt_version": REWRITE_PROMPT_VERSION,
            "timeout_s": validated_timeout_s,
            "latency_ms": elapsed_s * 1000.0,
            "error": f"{type(exc).__name__}: {exc}",
        }

    latency_ms = elapsed_s * 1000.0
    if elapsed_s > validated_timeout_s:
        return {
            "enabled": True,
            "outcome": "timeout",
            "reason": "deadline_exceeded",
            "skipped": False,
            "original_query": query,
            "effective_query": query,
            "rewritten_query": None,
            "endpoint": endpoint,
            "model": model,
            "prompt_version": REWRITE_PROMPT_VERSION,
            "timeout_s": validated_timeout_s,
            "latency_ms": latency_ms,
            "error": f"Rewrite exceeded timeout of {validated_timeout_s}s ({elapsed_s:.3f}s)",
        }

    try:
        content = _extract_rewrite_content(raw_response)
    except (ValueError, KeyError, TypeError) as exc:
        return {
            "enabled": True,
            "outcome": "error",
            "reason": "invalid_response",
            "skipped": False,
            "original_query": query,
            "effective_query": query,
            "rewritten_query": None,
            "endpoint": endpoint,
            "model": model,
            "prompt_version": REWRITE_PROMPT_VERSION,
            "timeout_s": validated_timeout_s,
            "latency_ms": latency_ms,
            "error": f"{type(exc).__name__}: {exc}",
        }

    stripped = content.strip()
    if not stripped:
        return {
            "enabled": True,
            "outcome": "rejected",
            "reason": "empty",
            "skipped": False,
            "original_query": query,
            "effective_query": query,
            "rewritten_query": None,
            "endpoint": endpoint,
            "model": model,
            "prompt_version": REWRITE_PROMPT_VERSION,
            "timeout_s": validated_timeout_s,
            "latency_ms": latency_ms,
            "error": None,
        }

    normalized = " ".join(stripped.split())
    if len(stripped) > max_chars or len(normalized) > max_chars:
        return {
            "enabled": True,
            "outcome": "rejected",
            "reason": "too_long",
            "skipped": False,
            "original_query": query,
            "effective_query": query,
            "rewritten_query": normalized,
            "endpoint": endpoint,
            "model": model,
            "prompt_version": REWRITE_PROMPT_VERSION,
            "timeout_s": validated_timeout_s,
            "latency_ms": latency_ms,
            "error": None,
        }

    original_normalized = " ".join(query.strip().split())
    if (
        normalized == query
        or normalized == query.strip()
        or normalized.casefold() == original_normalized.casefold()
    ):
        return {
            "enabled": True,
            "outcome": "rejected",
            "reason": "unchanged",
            "skipped": False,
            "original_query": query,
            "effective_query": query,
            "rewritten_query": normalized,
            "endpoint": endpoint,
            "model": model,
            "prompt_version": REWRITE_PROMPT_VERSION,
            "timeout_s": validated_timeout_s,
            "latency_ms": latency_ms,
            "error": None,
        }

    return {
        "enabled": True,
        "outcome": "applied",
        "reason": "applied",
        "skipped": False,
        "original_query": query,
        "effective_query": normalized,
        "rewritten_query": normalized,
        "endpoint": endpoint,
        "model": model,
        "prompt_version": REWRITE_PROMPT_VERSION,
        "timeout_s": validated_timeout_s,
        "latency_ms": latency_ms,
        "error": None,
    }


def lancedb_table_names(list_tables_result):
    """Normalize `LanceDBConnection.list_tables()` across API shapes.

    Older lancedb returned a plain list[str]. The installed 0.34.0 returns a
    pydantic `ListTablesResponse` with a `.tables` list[str] attribute; `in`
    against the response object itself is always False (it has no matching
    `__contains__`), so every query silently reported "collection not found"
    regardless of what was actually indexed (measured on this Mac). Prefer
    `.tables` when present so this keeps working across either shape instead
    of assuming one.
    """
    tables = getattr(list_tables_result, "tables", list_tables_result)
    return list(tables)

def ensure_fts5(connection_factory=sqlite3.connect):
    """Fail explicitly when this Python build lacks SQLite FTS5."""
    try:
        connection = connection_factory(":memory:")
        connection.execute("CREATE VIRTUAL TABLE fts_probe USING fts5(text)")
        connection.close()
    except sqlite3.Error as exc:
        raise RuntimeError(f"SQLite FTS5 is unavailable: {exc}") from exc

def exact_search(chunks, query, top_k):
    """Match stored filename/source literally; ties never depend on table row order."""
    # D22: no fuzzy fallback or embedding on a miss. A full metadata scan costs O(n);
    # a future metadata index can replace it while preserving this literal contract.
    matches = [item for item in chunks
               if item.get("filename") == query or item.get("source") == query]
    ranked = sorted(matches, key=lambda item: (
        item.get("source") or "",
        item.get("chunk_index") if item.get("chunk_index") is not None else 0,
        item.get("id") or "",
        item.get("filename") or "",
        item.get("text") or "",
    ))
    return [{"id": item.get("id"), "text": item.get("text"),
             "filename": item.get("filename"), "source": item.get("source"),
             "chunk_index": item.get("chunk_index") if item.get("chunk_index") is not None else 0,
             "score": 1.0, "mode": "exact"}
            for item in ranked[:max(0, top_k)]]

def lexical_search(chunks, query, top_k, connection_factory=sqlite3.connect):
    tokens = []
    token = []
    for char in query:
        if char.isalnum():
            token.append(char)
        elif token:
            tokens.append("".join(token))
            token = []
    if token:
        tokens.append("".join(token))
    if not tokens:
        return []

    ensure_fts5(connection_factory)
    with tempfile.TemporaryDirectory(prefix="kubemetal-rag-fts-") as temp_dir:
        # Rebuild from the available first slice; persistent indexing is intentionally deferred.
        connection = connection_factory(str(Path(temp_dir) / "chunks.sqlite3"))
        connection.execute("CREATE VIRTUAL TABLE chunks USING fts5(text, filename, source, tokenize='unicode61')")
        connection.executemany("INSERT INTO chunks(text, filename, source) VALUES (?, ?, ?)",
                               [(item.get("text"), item.get("filename"), item.get("source")) for item in chunks])
        # Quote each token so user input cannot introduce FTS operators or column filters.
        match_query = " OR ".join('"' + value.replace('"', '""') + '"' for value in tokens)
        rows = connection.execute(
            "SELECT rowid, bm25(chunks) FROM chunks WHERE chunks MATCH ? ORDER BY bm25(chunks) LIMIT ?",
            (match_query, top_k),
        ).fetchall()
        results = []
        for rowid, score in rows:
            item = chunks[rowid - 1]
            results.append({"id": item.get("id"), "text": item.get("text"), "filename": item.get("filename"),
                            "source": item.get("source"), "chunk_index": item.get("chunk_index", 0),
                            "score": float(score), "mode": "lexical"})
        connection.close()
        return results

def retrieval_score(result):
    """Return the backend-native score without normalizing incomparable scales."""
    return float(result.get("_distance", result.get("score", 0.0)))

def rrf_fuse(lexical_results, dense_results, top_k, k=RRF_K):
    """Fuse ranks and retain retriever-specific rank and score provenance."""
    fused = {}
    for retriever, results in (("lexical", lexical_results), ("dense", dense_results)):
        for rank, result in enumerate(results, start=1):
            chunk_id = result.get("id")
            if not isinstance(chunk_id, str) or not chunk_id:
                raise RuntimeError("Hybrid retrieval requires every result to have a chunk id.")

            # Ingest ids use the basename only, so same-named files collide; fuse on
            # the chunk's true identity (source path + index) instead.
            key = (result.get("source"), result.get("chunk_index", 0), chunk_id)
            entry = fused.setdefault(key, {
                "id": chunk_id,
                "text": result.get("text"),
                "filename": result.get("filename"),
                "source": result.get("source"),
                "chunk_index": result.get("chunk_index", 0),
                "score": 0.0,
                "hits": {},
            })
            # Preserve the first (best) rank if a backend returns a duplicate id.
            if retriever in entry["hits"]:
                continue
            entry["score"] += 1.0 / (k + rank)
            entry["hits"][retriever] = {"rank": rank, "score": retrieval_score(result)}

    ranked = sorted(fused.values(), key=lambda item: (-item["score"], item["source"] or "", item["chunk_index"], item["id"]))
    return [{
        "id": item["id"],
        "text": item["text"],
        "filename": item["filename"],
        "source": item["source"],
        "chunk_index": item["chunk_index"],
        "score": item["score"],
        "mode": "hybrid",
        "provenance": {
            "retrievers": [name for name in ("lexical", "dense") if name in item["hits"]],
            "lexical": item["hits"].get("lexical"),
            "dense": item["hits"].get("dense"),
        },
    } for item in ranked[:top_k]]

def get_dvc_bin() -> str:
    """
    Find dvc binary path in current Python environment or system PATH.
    """
    venv_bin = Path(sys.executable).parent / "dvc"
    if venv_bin.is_file():
        return str(venv_bin)
    found = shutil.which("dvc")
    if found:
        return found
    return "dvc"

def get_embedding_model(model_name: str):
    """
    Load embedding model using sentence-transformers.
    """
    try:
        from sentence_transformers import SentenceTransformer
        return SentenceTransformer(model_name)
    except ImportError:
        raise RuntimeError("sentence-transformers가 설치되지 않았습니다. setup_rag_env를 실행하세요.")

def chunk_text(text: str, chunk_size: int = 500, overlap: int = 50):
    """
    Simple text chunker with character limit and overlap.
    """
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
        start += (chunk_size - overlap)
    return chunks

def chunk_records(fpath: Path, content: str, chunking: str = "fixed"):
    """Index records for one file. "fixed" is the original format, byte for byte.

    "heading" applies only to Markdown (`#` is a comment in .py/.yaml); its ids carry an
    `h` marker (`name_h0`) so they can never collide with fixed ids (`name_0`).
    "python" (or "source") applies Python AST-based structure chunking to .py files; its ids carry a
    `p` marker (`name_p0`) when structured parsing succeeds. On syntax error, it falls back
    to fixed chunking with marker `""` and records the fallback reason (D22).
    """
    if chunking == "heading" and fpath.suffix.lower() == ".md":
        from chunking import chunk_markdown_headings
        file_chunks, marker = chunk_markdown_headings(content, chunk_size=500, overlap=50), "h"
    elif chunking in ("python", "source") and fpath.suffix.lower() == ".py":
        from source_chunking import chunk_python_source
        res = chunk_python_source(content, chunk_size=500, overlap=50)
        file_chunks = res
        marker = "p" if res.structured else ""
    else:
        file_chunks, marker = chunk_text(content, chunk_size=500, overlap=50), ""
    records = [{
        "id": f"{fpath.name}_{marker}{idx}",
        "source": str(fpath),
        "filename": fpath.name,
        "chunk_index": idx,
        "text": chunk
    } for idx, chunk in enumerate(file_chunks)]
    if chunking in ("python", "source"):
        # D22: Persist observed analysis state with every row, including mixed
        # file types; this adds schema fields only for the opt-in source mode.
        structured = getattr(file_chunks, "structured", None)
        metadata = {
            "chunking_status": ("structured" if structured else "fallback")
            if structured is not None else "fixed",
            "fallback_reason": getattr(file_chunks, "fallback_reason", None) or "",
        }
        for record in records:
            record.update(metadata)
        class ChunkRecordsList(list):
            pass
        wrapped = ChunkRecordsList(records)
        wrapped.chunking_metadata = metadata
        wrapped.structured = structured
        wrapped.fallback_reason = getattr(file_chunks, "fallback_reason", None)
        return wrapped
    return records

def cmd_index(args):
    docs_dir = Path(args.docs_dir).expanduser().resolve()
    db_path = Path(args.db_path).expanduser().resolve()
    collection = args.collection
    model_name = args.model

    if not docs_dir.exists():
        print(json.dumps({"status": "error", "error": f"문서 경로가 존재하지 않습니다: {docs_dir}"}))
        sys.exit(1)

    try:
        import lancedb
    except ImportError:
        print(json.dumps({"status": "error", "error": "lancedb가 설치되지 않았습니다. setup_rag_env를 실행하세요."}))
        sys.exit(1)

    # Collect files
    supported_exts = {".txt", ".md", ".json", ".csv", ".py", ".rs", ".rst", ".yaml", ".yml"}
    files = []
    if docs_dir.is_file():
        files.append(docs_dir)
    else:
        for root, _, filenames in os.walk(docs_dir):
            for fn in filenames:
                p = Path(root) / fn
                if p.suffix.lower() in supported_exts:
                    files.append(p)

    if not files:
        print(json.dumps({"status": "error", "error": f"인덱싱할 문서를 찾을 수 없습니다: {docs_dir}"}))
        sys.exit(1)

    # Extract text & chunk
    chunks_data = []
    doc_count = 0
    file_chunking = []
    for fpath in files:
        try:
            content = fpath.read_text(encoding="utf-8", errors="ignore")
            records = chunk_records(fpath, content, args.chunking)
            if args.chunking in ("python", "source"):
                file_chunking.append({"source": str(fpath), **records.chunking_metadata})
            if records:
                doc_count += 1
                chunks_data.extend(records)
        except Exception as exc:
            if args.chunking in ("python", "source"):
                file_chunking.append({
                    "source": str(fpath),
                    "chunking_status": "error",
                    "fallback_reason": f"{type(exc).__name__}: {exc}",
                })
            continue

    chunking_summary = ({
        "file_chunking": file_chunking,
        "fallback_files": sum(f["chunking_status"] == "fallback" for f in file_chunking),
        "error_files": sum(f["chunking_status"] == "error" for f in file_chunking),
    } if args.chunking in ("python", "source") else {})
    if not chunks_data:
        print(json.dumps({"status": "error", "error": "인덱싱 가능한 텍스트 내용이 없습니다.", **chunking_summary}))
        sys.exit(1)

    # Embed chunks
    model = get_embedding_model(model_name)
    texts = [item["text"] for item in chunks_data]
    embeddings = model.encode(texts, show_progress_bar=False)

    for item, emb in zip(chunks_data, embeddings):
        item["vector"] = emb.tolist()

    # LanceDB save
    db_path.mkdir(parents=True, exist_ok=True)
    db = lancedb.connect(str(db_path))

    existing_tables = lancedb_table_names(db.list_tables())
    mode = "overwrite" if collection in existing_tables else "create"
    table = db.create_table(collection, data=chunks_data, mode=mode)

    print(json.dumps({
        "status": "ok",
        "collection": collection,
        "indexed_docs": doc_count,
        "total_chunks": len(chunks_data),
        "db_path": str(db_path),
        **chunking_summary
    }))

def cmd_query(args, rewrite_http_client=None, rewrite_clock=None):
    db_path = Path(args.db_path).expanduser().resolve()
    collection = args.collection
    query_str = args.query
    top_k = args.top_k
    model_name = args.model

    if not db_path.exists():
        print(json.dumps({"status": "error", "error": f"LanceDB 경로가 존재하지 않습니다: {db_path}"}))
        sys.exit(1)

    try:
        import lancedb
    except ImportError:
        print(json.dumps({"status": "error", "error": "lancedb가 설치되지 않았습니다."}))
        sys.exit(1)

    db = lancedb.connect(str(db_path))
    existing_tables = lancedb_table_names(db.list_tables())
    if collection not in existing_tables:
        print(json.dumps({"status": "error", "error": f"컬렉션 '{collection}'을 찾을 수 없습니다."}))
        sys.exit(1)

    table = db.open_table(collection)

    if args.mode == "auto":
        resolved_mode, rule_name = classify_query(query_str)
        preclassified = (resolved_mode, rule_name)
    else:
        resolved_mode = args.mode
        rule_name = "explicit"
        preclassified = None

    rewrite_enabled = bool(getattr(args, "rewrite", False))
    rewrite_url = getattr(args, "rewrite_url", DEFAULT_REWRITE_URL)
    rewrite_model = getattr(args, "rewrite_model", DEFAULT_REWRITE_MODEL)
    rewrite_timeout = getattr(args, "rewrite_timeout", DEFAULT_REWRITE_TIMEOUT_S)
    rewrite_max_chars = getattr(args, "rewrite_max_chars", DEFAULT_REWRITE_MAX_CHARS)
    http_client = rewrite_http_client or getattr(args, "rewrite_http_client", None)
    clock = rewrite_clock or getattr(args, "rewrite_clock", time.perf_counter)

    rewrite_trace = rewrite_query(
        query_str,
        enabled=rewrite_enabled,
        url=rewrite_url,
        model=rewrite_model,
        timeout_s=rewrite_timeout,
        max_chars=rewrite_max_chars,
        mode=resolved_mode,
        preclassified=preclassified,
        http_client=http_client,
        clock=clock,
    )
    search_query = rewrite_trace["effective_query"]

    if resolved_mode == "exact":
        search_results = exact_search(table.to_arrow().to_pylist(), search_query, top_k)
    elif resolved_mode == "lexical":
        # LanceTable has no .to_list() on the installed lancedb (0.34.0) -
        # measured on this Mac (AttributeError). to_arrow().to_pylist() is
        # the stable route since pyarrow is a hard lancedb dependency.
        search_results = lexical_search(table.to_arrow().to_pylist(), search_query, top_k)
    elif resolved_mode == "dense":
        model = get_embedding_model(model_name)
        query_vector = model.encode(search_query, show_progress_bar=False).tolist()
        search_results = table.search(query_vector).limit(top_k).to_list()
    else:
        chunks = table.to_arrow().to_pylist()
        candidate_count = max(top_k, HYBRID_CANDIDATE_COUNT)
        lexical_results = lexical_search(chunks, search_query, candidate_count)
        # A hybrid query is invalid if dense retrieval cannot run; do not
        # catch this and silently turn the result into lexical-only output.
        model = get_embedding_model(model_name)
        query_vector = model.encode(search_query, show_progress_bar=False).tolist()
        dense_results = table.search(query_vector).limit(candidate_count).to_list()
        search_results = rrf_fuse(lexical_results, dense_results, top_k)

    # D22: all modes preserve absent text/filename/source as null, never invented
    # metadata. Consumers must label unknown values; requiring metadata later
    # would need explicit validation (and reject otherwise searchable legacy rows).
    formatted_results = []
    for r in search_results:
        formatted = {
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
            formatted["provenance"] = r["provenance"]
        formatted_results.append(formatted)

    print(json.dumps({
        "status": "ok",
        "query": query_str,
        "resolved_mode": resolved_mode,
        "rule": rule_name,
        "rewrite": rewrite_trace,
        "results": formatted_results
    }))

def cmd_dvc_commit(args):
    data_dir = Path(args.data_dir).expanduser().resolve()
    remote_url = args.remote_url
    bucket = args.bucket
    # D13/D21: S3 크리덴셜은 CLI 인자로 받지 않는다(ps 노출 방지) — Rust 스폰 시
    # KUBEMETAL_S3_ACCESS_KEY/KUBEMETAL_S3_SECRET_KEY 환경변수로 주입된다. 기본값은
    # seaweedfs-s3-credentials.yaml의 stringData(SeaweedFS 무인증 모드 더미값)와 동일하게
    # 맞춰 Rust 없이 단독 실행해도 동작한다.
    access_key = os.environ.get("KUBEMETAL_S3_ACCESS_KEY", "kubemetal")
    secret_key = os.environ.get("KUBEMETAL_S3_SECRET_KEY", "kubemetal-local")

    if not data_dir.exists():
        print(json.dumps({"status": "error", "error": f"데이터 경로가 존재하지 않습니다: {data_dir}"}))
        sys.exit(1)

    dvc_bin = get_dvc_bin()
    work_dir = data_dir if data_dir.is_dir() else data_dir.parent

    # Check / init DVC
    dvc_dir = work_dir / ".dvc"
    if not dvc_dir.exists():
        res = subprocess.run([dvc_bin, "init", "--no-scm"], cwd=work_dir, capture_output=True, text=True)
        if res.returncode != 0:
            res = subprocess.run([dvc_bin, "init"], cwd=work_dir, capture_output=True, text=True)

    # Add S3 remote
    remote_name = "seaweedfs"
    s3_uri = f"s3://{bucket}"
    
    subprocess.run([dvc_bin, "remote", "add", "-f", "-d", remote_name, s3_uri], cwd=work_dir, capture_output=True)
    subprocess.run([dvc_bin, "remote", "modify", remote_name, "endpointurl", remote_url], cwd=work_dir, capture_output=True)
    subprocess.run([dvc_bin, "remote", "modify", remote_name, "access_key_id", access_key], cwd=work_dir, capture_output=True)
    subprocess.run([dvc_bin, "remote", "modify", remote_name, "secret_access_key", secret_key], cwd=work_dir, capture_output=True)
    subprocess.run([dvc_bin, "remote", "modify", remote_name, "use_ssl", "false"], cwd=work_dir, capture_output=True)

    target_rel = data_dir.name if data_dir != work_dir else "."
    
    # dvc add
    subprocess.run([dvc_bin, "add", target_rel], cwd=work_dir, capture_output=True, text=True)

    # dvc push
    push_res = subprocess.run([dvc_bin, "push", "-r", remote_name], cwd=work_dir, capture_output=True, text=True)
    if push_res.returncode != 0:
        print(json.dumps({
            "status": "error",
            "error": f"DVC push 실패: {push_res.stderr.strip() or push_res.stdout.strip()}"
        }))
        sys.exit(1)

    print(json.dumps({
        "status": "ok",
        "message": f"DVC push 성공: {target_rel} -> {s3_uri} ({remote_url})",
        "remote": remote_name,
        "s3_uri": s3_uri
    }))

def main():
    parser = argparse.ArgumentParser(description="KubeMetal RAG & DVC CLI")
    subparsers = parser.add_subparsers(dest="command", required=True)

    # index subcommand
    p_index = subparsers.add_parser("index")
    p_index.add_argument("--docs-dir", required=True)
    p_index.add_argument("--db-path", default="~/.kubemetal/lancedb")
    p_index.add_argument("--collection", default="default")
    p_index.add_argument("--model", default="sentence-transformers/all-MiniLM-L6-v2")
    p_index.add_argument("--chunking", choices=("fixed", "heading", "python", "source"), default="fixed")

    # query subcommand
    p_query = subparsers.add_parser("query")
    p_query.add_argument("--query", required=True)
    p_query.add_argument("--db-path", default="~/.kubemetal/lancedb")
    p_query.add_argument("--collection", default="default")
    p_query.add_argument("--top-k", type=int, default=3)
    p_query.add_argument("--model", default="sentence-transformers/all-MiniLM-L6-v2")
    p_query.add_argument("--mode", choices=("auto", "dense", "lexical", "hybrid", "exact"), default="dense")
    p_query.add_argument("--rewrite", action="store_true", default=False)
    p_query.add_argument("--rewrite-url", default=DEFAULT_REWRITE_URL)
    p_query.add_argument("--rewrite-model", default=DEFAULT_REWRITE_MODEL)
    p_query.add_argument("--rewrite-timeout", type=_parse_rewrite_timeout_arg, default=DEFAULT_REWRITE_TIMEOUT_S)
    p_query.add_argument("--rewrite-max-chars", type=int, default=DEFAULT_REWRITE_MAX_CHARS)

    # dvc-commit subcommand
    p_dvc = subparsers.add_parser("dvc-commit")
    p_dvc.add_argument("--data-dir", required=True)
    p_dvc.add_argument("--remote-url", default="http://127.0.0.1:8333")
    p_dvc.add_argument("--bucket", default="dvc-repo")
    p_dvc.add_argument("--message", default="Commit dataset")

    args = parser.parse_args()

    if args.command == "index":
        cmd_index(args)
    elif args.command == "query":
        cmd_query(args)
    elif args.command == "dvc-commit":
        cmd_dvc_commit(args)

if __name__ == "__main__":
    main()
