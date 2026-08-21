#!/usr/bin/env python3
"""IPC 반환 타입 대조 — Rust `#[tauri::command]`와 프런트 `invoke<T>` 주석이 어긋나면 실패한다.

왜 필요한가: `invoke<T>`의 T는 **주장일 뿐 검증되지 않는다**. tsc는 그 주장을 그대로 믿으므로
Rust가 bool을 돌려주는데 `invoke<string>`이라 적어두면 타입 체크는 통과하고 런타임에서만
깨진다. 실측 2026-08-21: `kill_mlx_process`가 그 상태였고, 프로세스를 정상 종료하고도
"종료 실패"를 표시했다.

구조체/Vec 반환은 이름 대응이라 자동 판정하지 않는다 — 원시 타입만 본다.
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


def ts_invocations():
    out = []
    files = glob.glob("src/**/*.ts", recursive=True) + glob.glob("src/**/*.tsx", recursive=True)
    for path in files:
        with open(path, encoding="utf-8") as fh:
            src = fh.read()
        for match in re.finditer(r"invoke<([^>]+)>\(\s*'(\w+)'", src):
            out.append((match.group(2), match.group(1).strip(), path))
    return out


def rust_core(ret):
    match = re.match(r"Result<\s*(.+?)\s*,\s*String\s*>", ret)
    return (match.group(1) if match else ret).strip()


def main():
    rust = rust_commands()
    mismatches = []
    checked = 0
    for name, ts_type, path in ts_invocations():
        if name not in rust:
            mismatches.append(f"{name}: 프런트가 호출하지만 Rust 커맨드가 없다 ({path})")
            continue
        core = rust_core(rust[name])
        if core not in PRIM:
            continue  # 구조체/Vec은 범위 밖
        checked += 1
        if PRIM[core] != ts_type:
            mismatches.append(
                f"{name}: Rust {rust[name]} 인데 TS는 invoke<{ts_type}> ({path})"
            )

    print(f"원시 타입 커맨드 {checked}건 대조")
    if mismatches:
        print("\nFAIL: IPC 반환 타입이 어긋났다 —")
        for line in mismatches:
            print(f"  - {line}")
        return 1
    print("OK: Rust 반환 타입과 invoke<T> 주석이 일치한다.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
