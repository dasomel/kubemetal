"""Opt-in Python source code structure-aware chunker (issue #38).

Stdlib only (`ast`), deterministic. The default fixed-size `chunk_text` in
`rag_host.py` and heading-aware Markdown chunker (`chunking.py`) are untouched;
this module is used when `--chunking python` (or `--chunking source`) is requested.

Usage
-----
1. Python API:
   >>> from source_chunking import chunk_python_source
   >>> res = chunk_python_source(code_text, chunk_size=500, overlap=50)
   >>> # res is a SourceChunkResult (subclasses list of str chunks)
   >>> for chunk in res:
   ...     print(chunk)
   >>> print(res.structured)       # True if AST parsing succeeded, False on fallback
   >>> print(res.fallback_reason)  # None on success, or error string on fallback (D22)

2. CLI invocation via `rag_host.py`:
   $ python3 scripts/rag/rag_host.py index \\
       --docs-dir ./path/to/docs \\
       --chunking python

   Chunk IDs carry a `p` marker (`<filename>_p0`, `<filename>_p1`, ...) when
   structured parsing succeeds, distinct from fixed IDs (`<filename>_0`) and
   Markdown heading IDs (`<filename>_h0`).

Design Decisions & Contracts
----------------------------
- D-S1: Stdlib `ast` only. No third-party parser dependencies (no tree-sitter, libcst).
- D-S2: Line ranges from the original source. Extracts segments using exact source
  line slices to preserve comments, indentation, docstrings, decorators, and syntax.
- D-S3: Preserves function decorators (`@dec`), `async def` definitions, and nested
  definitions (`def inner():` inside an enclosing function remains in the enclosing chunk).
- D-S4: Preserves module body. Module-level docstrings, imports, constants, globals,
  and trailing blocks (`if __name__ == '__main__':`) are grouped into module chunks.
- D-S5: Class decomposition. If a class definition fits within `chunk_size`, it is
  emitted whole. If it exceeds `chunk_size`, it decomposes at method boundaries into
  a class preamble chunk and individual method chunks.
- D-S6: Bounded chunk size. Large functions or segments exceeding `chunk_size` are
  split using fixed-size sliding windowing (`chunk_size`, `overlap`).
- D-S7 (D22 compliance): On syntax or parsing errors (`SyntaxError`), the chunker
  falls back to fixed-size chunking (`chunk_text`), sets `structured = False`, and
  explicitly records the failure reason in `fallback_reason`. It never fabricates
  a successful structural parse.
- D-S8: Determinism. Identical input text produces identical chunk strings and IDs.
"""

import ast
from typing import List, Optional, Tuple


def chunk_text(text: str, chunk_size: int = 500, overlap: int = 50) -> List[str]:
    """Frozen copy of the pre-#38 fixed chunker for fallback compatibility."""
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


def _split_plain(block: str, budget: int, overlap: int) -> List[str]:
    """Split an oversized plain code block with sliding windowing."""
    pieces = []
    start = 0
    step = max(budget - min(overlap, budget // 2), 1)
    while start < len(block):
        piece = block[start:start + budget].strip()
        if piece:
            pieces.append(piece)
        if start + budget >= len(block):
            break
        start += step
    return pieces


class SourceChunkResult(list):
    """Result of Python source code chunking.

    Subclasses list[str] so callers and LanceDB record builders can iterate over
    chunk texts directly, while also exposing structural metadata:
    - `structured`: True if AST parsing succeeded, False if fallen back.
    - `fallback_reason`: Description of the parse error if fallen back, else None.
    - `chunks`: Direct access to list[str].
    """

    def __init__(
        self,
        chunks: List[str],
        structured: bool = True,
        fallback_reason: Optional[str] = None,
    ):
        super().__init__(chunks)
        self.chunks = list(chunks)
        self.structured = structured
        self.fallback_reason = fallback_reason

    @property
    def is_structured(self) -> bool:
        return self.structured


def _get_start_line(node: ast.AST) -> int:
    """Return earliest line number of a node including decorators."""
    decorators = getattr(node, "decorator_list", [])
    lineno = getattr(node, "lineno", 1)
    if decorators:
        dec_lines = [getattr(d, "lineno", lineno) for d in decorators]
        return min([lineno] + dec_lines)
    return lineno


def _get_end_line(node: ast.AST) -> int:
    """Return last line number of a node."""
    return getattr(node, "end_lineno", getattr(node, "lineno", 1))


def _decompose_class(
    node: ast.ClassDef,
    lines: List[str],
    class_start: int,
    class_end: int,
    chunk_size: int,
    overlap: int,
) -> List[str]:
    """Decompose an oversized class into class preamble and method chunks."""
    class_text = "\n".join(lines[class_start - 1 : class_end]).strip()
    if len(class_text) <= chunk_size:
        return [class_text]

    # Find method and nested definition children in class body
    children = [
        n
        for n in node.body
        if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef))
    ]
    if not children:
        # No methods to split by; split plain block
        return _split_plain(class_text, chunk_size, overlap)

    chunks = []
    # 1. Class preamble (from class_start up to before first child)
    first_child_start = _get_start_line(children[0])
    if first_child_start > class_start:
        preamble = "\n".join(lines[class_start - 1 : first_child_start - 1]).strip()
        if preamble:
            if len(preamble) <= chunk_size:
                chunks.append(preamble)
            else:
                chunks.extend(_split_plain(preamble, chunk_size, overlap))

    # 2. Iterate children and inter-child statements
    prev_child_end = first_child_start - 1
    for child in children:
        child_start = _get_start_line(child)
        child_end = _get_end_line(child)

        # Inter-child code (attributes, expressions between methods)
        if child_start > prev_child_end + 1:
            inter_text = "\n".join(lines[prev_child_end : child_start - 1]).strip()
            if inter_text:
                if len(inter_text) <= chunk_size:
                    chunks.append(inter_text)
                else:
                    chunks.extend(_split_plain(inter_text, chunk_size, overlap))

        # Child definition (method or nested class)
        if isinstance(child, ast.ClassDef):
            child_chunks = _decompose_class(
                child, lines, child_start, child_end, chunk_size, overlap
            )
            chunks.extend(child_chunks)
        else:
            child_text = "\n".join(lines[child_start - 1 : child_end]).strip()
            if child_text:
                if len(child_text) <= chunk_size:
                    chunks.append(child_text)
                else:
                    chunks.extend(_split_plain(child_text, chunk_size, overlap))

        prev_child_end = child_end

    # 3. Post-child trailing statements before class_end
    if prev_child_end < class_end:
        post_text = "\n".join(lines[prev_child_end:class_end]).strip()
        if post_text:
            if len(post_text) <= chunk_size:
                chunks.append(post_text)
            else:
                chunks.extend(_split_plain(post_text, chunk_size, overlap))

    return chunks


def chunk_python_source(
    text: str,
    chunk_size: int = 500,
    overlap: int = 50,
) -> SourceChunkResult:
    """Structure-aware chunking for Python source code using stdlib `ast`.

    Divides Python source into module preamble, function, class, and module
    epilogue segments by original line ranges.

    Returns:
        SourceChunkResult containing the list of chunk strings, structured status,
        and fallback reason if AST parsing failed.
    """
    stripped = text.strip()
    if not stripped:
        return SourceChunkResult([], structured=True, fallback_reason=None)

    try:
        tree = ast.parse(text)
    except Exception as e:
        # D22: Never fabricate success on failure.
        # Fall back to fixed chunking and explicitly record failure reason.
        fallback = chunk_text(text, chunk_size=chunk_size, overlap=overlap)
        reason = f"{type(e).__name__}: {e}"
        return SourceChunkResult(fallback, structured=False, fallback_reason=reason)

    lines = text.splitlines()
    total_lines = len(lines)

    # If the file has no AST statements (e.g. only comments and whitespace)
    if not tree.body:
        if stripped:
            if len(stripped) <= chunk_size:
                return SourceChunkResult([stripped], structured=True, fallback_reason=None)
            chunks = _split_plain(stripped, chunk_size, overlap)
            return SourceChunkResult(chunks, structured=True, fallback_reason=None)
        return SourceChunkResult([], structured=True, fallback_reason=None)

    # Group top-level statements into contiguous module blocks and definitions
    groups: List[Tuple[str, List[ast.AST]]] = []
    current_module_group: List[ast.AST] = []

    for node in tree.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            if current_module_group:
                groups.append(("module", current_module_group))
                current_module_group = []
            groups.append(("def", [node]))
        else:
            current_module_group.append(node)
    if current_module_group:
        groups.append(("module", current_module_group))

    chunks: List[str] = []
    last_end = 0

    for idx, (kind, nodes) in enumerate(groups):
        if kind == "def":
            node = nodes[0]
            dec_start = _get_start_line(node)
            end = _get_end_line(node)

            # Include preceding comments immediately above decorators/def
            c_start = dec_start
            while c_start - 1 > last_end and lines[c_start - 2].strip().startswith("#"):
                c_start -= 1
            start = c_start

            if isinstance(node, ast.ClassDef):
                class_chunks = _decompose_class(
                    node, lines, start, end, chunk_size, overlap
                )
                chunks.extend(class_chunks)
            else:
                # FunctionDef or AsyncFunctionDef (including nested defs)
                func_text = "\n".join(lines[start - 1 : end]).strip()
                if func_text:
                    if len(func_text) <= chunk_size:
                        chunks.append(func_text)
                    else:
                        chunks.extend(_split_plain(func_text, chunk_size, overlap))
            last_end = max(last_end, end)
        else:
            # Module-level statements (imports, globals, docstrings, expressions)
            first_node = nodes[0]
            last_node = nodes[-1]
            first_lineno = getattr(first_node, "lineno", 1)
            last_lineno = _get_end_line(last_node)

            if idx == 0:
                # First module group starts at the very top of file (line 1)
                start = 1
            else:
                c_start = first_lineno
                while c_start - 1 > last_end and lines[c_start - 2].strip().startswith("#"):
                    c_start -= 1
                start = c_start

            end = last_lineno
            # If this is the last group in the file, extend to total_lines
            if idx == len(groups) - 1 and end < total_lines:
                end = total_lines

            mod_text = "\n".join(lines[start - 1 : end]).strip()
            if mod_text:
                if len(mod_text) <= chunk_size:
                    chunks.append(mod_text)
                else:
                    chunks.extend(_split_plain(mod_text, chunk_size, overlap))
            last_end = max(last_end, end)

    # Trailing comments or code past the last AST definition
    if last_end < total_lines:
        tail = "\n".join(lines[last_end:total_lines]).strip()
        if tail:
            if len(tail) <= chunk_size:
                chunks.append(tail)
            else:
                chunks.extend(_split_plain(tail, chunk_size, overlap))

    return SourceChunkResult(chunks, structured=True, fallback_reason=None)
