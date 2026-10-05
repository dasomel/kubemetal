"""Opt-in heading-aware Markdown chunker (issue #38). Stdlib only, deterministic.

The default fixed-size `chunk_text` in rag_host.py is untouched; this module is only
used when `--chunking heading` is requested.

Decisions
- D-H1: only ATX headings (`#`..`######`, up to 3 leading spaces) start a section.
  Setext headings (`Title` + `===`/`---`) are NOT recognised: the `---` underline is
  indistinguishable from a horizontal rule or a table delimiter without lookahead
  heuristics, and a wrong guess silently mis-splits. Setext titles stay body text.
- D-H2: fenced code blocks and pipe tables are atomic. A single one larger than the
  size budget is emitted whole (over budget) rather than split; embedding models
  truncate, a half code block is worse than an oversized chunk. Every other oversized
  section is split to the same max size as the fixed chunker.
- D-H3: the heading path is stored in the chunk text as a `[A > B]` first line (not a
  new field), so the LanceDB / FTS5 schema is unchanged and the path is searchable.
  The prefix counts against `chunk_size`.
- D-H4: a section shorter than `chunk_size // 4` is merged with the following
  section(s) while the result still fits; the later heading line is kept inline.
"""

import re

_HEADING = re.compile(r"^ {0,3}(#{1,6})(?:[ \t]+(.*?))?[ \t]*$")
_CLOSING_HASHES = re.compile(r"[ \t]+#+$")
_FENCE = re.compile(r"^ {0,3}(`{3,}|~{3,})")


def _parse_sections(text):
    """Return [(path_tuple, heading_line, blocks)]; blocks are atomic text units."""
    sections = []
    stack = []  # [(level, title)]
    path, heading_line, blocks = (), "", []
    para, table = [], []
    fence = None  # (char, length, lines) while inside a fenced block

    def flush_para():
        if para:
            blocks.append("\n".join(para))
            para.clear()

    def flush_table():
        if table:
            blocks.append("\n".join(table))
            table.clear()

    def close_section():
        flush_para()
        flush_table()
        sections.append((path, heading_line, list(blocks)))
        blocks.clear()

    for line in text.splitlines():
        if fence is not None:
            fence[2].append(line)
            m = _FENCE.match(line)
            if m and m.group(1)[0] == fence[0] and len(m.group(1)) >= fence[1] \
                    and not line.strip()[len(m.group(1)):].strip():
                blocks.append("\n".join(fence[2]))
                fence = None
            continue
        m = _FENCE.match(line)
        if m:
            flush_para()
            flush_table()
            fence = (m.group(1)[0], len(m.group(1)), [line])
            continue
        m = _HEADING.match(line)
        if m:
            close_section()
            level = len(m.group(1))
            title = _CLOSING_HASHES.sub("", (m.group(2) or "").strip()).strip()
            while stack and stack[-1][0] >= level:
                stack.pop()
            if title:
                stack.append((level, title))
            path = tuple(t for _, t in stack)
            heading_line = line.strip()
            continue
        if line.lstrip().startswith("|"):
            flush_para()
            table.append(line)
        elif not line.strip():
            flush_para()
            flush_table()
        else:
            flush_table()
            para.append(line)
    if fence is not None:  # unterminated fence: keep it as one block to the end
        blocks.append("\n".join(fence[2]))
    close_section()
    return sections


def _prefix(path, chunk_size):
    if not path:
        return ""
    label = " > ".join(path)
    limit = max(chunk_size // 4, 8)
    if len(label) > limit:
        label = "…" + label[-(limit - 1):]
    return f"[{label}]\n"


def _split_plain(block, budget, overlap):
    """Split an oversized plain paragraph with the fixed chunker's windowing."""
    pieces, start = [], 0
    step = max(budget - min(overlap, budget // 2), 1)
    while start < len(block):
        piece = block[start:start + budget].strip()
        if piece:
            pieces.append(piece)
        if start + budget >= len(block):
            break
        start += step
    return pieces


def _is_atomic(block):
    return _FENCE.match(block.split("\n", 1)[0]) is not None or block.lstrip().startswith("|")


def _pack(blocks, budget, overlap):
    """Greedy-pack blocks into texts of at most `budget` chars (atomic blocks may exceed)."""
    out, current = [], ""
    for block in blocks:
        pieces = [block] if len(block) <= budget or _is_atomic(block) \
            else _split_plain(block, budget, overlap)
        for piece in pieces:
            if current and len(current) + 2 + len(piece) > budget:
                out.append(current)
                current = ""
            current = f"{current}\n\n{piece}" if current else piece
    if current:
        out.append(current)
    return out


def chunk_markdown_headings(text: str, chunk_size: int = 500, overlap: int = 50):
    """Heading-aware chunking; same signature and size cap as `chunk_text`."""
    min_size = chunk_size // 4
    result = []
    pending = None  # (path, body) tiny single-chunk section awaiting a merge

    def emit(path, body):
        body = body.strip()
        if body:
            result.append(_prefix(path, chunk_size) + body)

    for path, heading_line, blocks in _parse_sections(text):
        budget = max(chunk_size - len(_prefix(path, chunk_size)), 1)
        texts = _pack(blocks, budget, overlap)
        if not texts:
            continue  # empty section: its title still lives in descendants' paths
        if pending is not None:
            p_path, p_body = pending
            merged = f"{p_body}\n\n{heading_line}\n{texts[0]}" if heading_line else f"{p_body}\n\n{texts[0]}"
            if len(merged) <= max(chunk_size - len(_prefix(p_path, chunk_size)), 1):
                texts[0] = merged
                path = p_path
                pending = None
            else:
                emit(p_path, p_body)
                pending = None
        if len(texts) == 1 and len(texts[0]) < min_size:
            pending = (path, texts[0])
            continue
        for body in texts:
            emit(path, body)
    if pending is not None:
        emit(*pending)
    return result
