#!/usr/bin/env python3
"""IPC 반환 타입 대조 — Rust `#[tauri::command]`와 프런트 `invoke<T>` 주석이 어긋나면 실패한다.

왜 필요한가: `invoke<T>`의 T는 **주장일 뿐 검증되지 않는다**. tsc는 그 주장을 그대로 믿으므로
Rust가 bool을 돌려주는데 `invoke<string>`이라 적어두면 타입 체크는 통과하고 런타임에서만
깨진다. 실측 2026-08-21: `kill_mlx_process`가 그 상태였고, 프로세스를 정상 종료하고도
"종료 실패"를 표시했다.

구조체/Vec 반환은 이름 대응이라 자동 판정하지 않는다 — 원시 타입만 본다. 다만 **건너뛴
수는 반드시 출력한다**: 조용히 넘어가면서 OK를 보고하는 게이트는 검사하지 않는 게이트와
구분되지 않는다. 실제로 첫 판에서 `invoke<Record<string, number>>` 같은 중첩 제네릭을
정규식이 못 읽고 2건을 말없이 빠뜨린 채 통과를 보고했다(실측 2026-08-22).
"""
import glob
import re
import sys

PRIM = {
    "bool": "boolean",
    "String": "string",
    "u16": "number",
    "u32": "number",
    "u64": "number",
    "f64": "number",
    "i32": "number",
    "usize": "number",
}


def rust_commands():
    out = {}
    for path in glob.glob("src-tauri/src/**/*.rs", recursive=True):
        with open(path, encoding="utf-8") as fh:
            src = fh.read()
        pattern = r"#\[tauri::command\][\s\S]{0,200}?fn\s+(\w+)\s*\([\s\S]*?\)\s*(?:->\s*([^{]+))?\{"
        for match in re.finditer(pattern, src):
            out[match.group(1)] = (match.group(2) or "()").strip()
    return out


def _read_type_arg(src, start):
    """`invoke<` 바로 뒤에서 꺾쇠 균형을 세어 타입 인자를 읽는다.

    `[^>]+`로는 `Record<string, number>`처럼 중첩된 제네릭을 읽지 못하고, 그런 호출을
    **말없이 건너뛴다**. 건너뛴 것은 검사되지 않은 것이므로 여기서 제대로 읽는다.
    """
    depth = 1
    i = start
    while i < len(src) and depth:
        if src[i] == "<":
            depth += 1
        elif src[i] == ">":
            depth -= 1
            if depth == 0:
                return src[start:i], i + 1
        i += 1
    return None, start


def ts_invocations():
    """(커맨드명, TS 타입 인자, 파일) 목록과, 타입 인자 없는 호출 수를 함께 돌려준다."""
    out = []
    untyped = 0
    files = glob.glob("src/**/*.ts", recursive=True) + glob.glob("src/**/*.tsx", recursive=True)
    for path in files:
        with open(path, encoding="utf-8") as fh:
            src = fh.read()
        for match in re.finditer(r"invoke\s*(<)?", src):
            pos = match.end()
            if match.group(1):
                type_arg, pos = _read_type_arg(src, pos)
                if type_arg is None:
                    continue
            else:
                type_arg = None
            call = re.match(r"\(\s*'(\w+)'", src[pos:])
            if not call:
                continue
            if type_arg is None:
                untyped += 1
                continue
            out.append((call.group(1), type_arg.strip(), path))
    return out, untyped


def rust_core(ret):
    match = re.match(r"Result<\s*(.+?)\s*,\s*String\s*>", ret)
    return (match.group(1) if match else ret).strip()


def main():
    rust = rust_commands()
    invocations, untyped = ts_invocations()
    mismatches = []
    checked = 0
    skipped_nonprimitive = 0
    for name, ts_type, path in invocations:
        if name not in rust:
            mismatches.append(f"{name}: 프런트가 호출하지만 Rust 커맨드가 없다 ({path})")
            continue
        core = rust_core(rust[name])
        if core not in PRIM:
            skipped_nonprimitive += 1
            continue  # 구조체/Vec은 이름 대응이라 자동 판정 불가
        checked += 1
        if PRIM[core] != ts_type:
            mismatches.append(
                f"{name}: Rust {rust[name]} 인데 TS는 invoke<{ts_type}> ({path})"
            )

    print(
        f"invoke 호출 {len(invocations) + untyped}건 — 대조 {checked} · "
        f"구조체 반환 {skipped_nonprimitive} · 타입 인자 없음 {untyped}"
    )
    if mismatches:
        print("\nFAIL: IPC 반환 타입이 어긋났다 —")
        for line in mismatches:
            print(f"  - {line}")
        return 1
    print("OK: Rust 반환 타입과 invoke<T> 주석이 일치한다.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
