#!/usr/bin/env bash
# ==============================================================================
# check_licenses.sh — 번들 의존성의 라이선스 정책 게이트 (이슈 #9)
#
# NOTICE의 "GPL/AGPL/SSPL/BUSL 없음" 주장은 수기 검토로 세웠는데, 수기 검토는
# 다음 `cargo update` 한 번이면 거짓이 된다. lockfile이 바뀔 때 깨지는 게이트로
# 바꾼다 — CLAUDE.md의 "파생시킬 수 없으면 어긋날 때 실패하는 테스트를 둔다".
#
# 판정 대상은 **바이너리에 컴파일되는 것**뿐이다(cargo metadata + pnpm --prod).
# 클러스터가 pull하는 컨테이너 이미지는 범위 밖이고, 그 이유는 NOTICE에 적혀 있다.
#
# SPDX 식 처리: OR 중 하나라도 허용되면 통과(우리가 그 대안을 선택한다),
# AND는 전부 허용돼야 통과. cargo의 구식 `MIT/Apache-2.0` 슬래시 표기는 OR로,
# `WITH <exception>`은 기반 라이선스로 환원한다.
# 라이선스가 비어 있으면 UNKNOWN으로 보고 실패시킨다 — 모르는 것은 통과가 아니다.
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

# 테스트가 합성 입력을 주입할 수 있게 한다(음성 대조군 없는 게이트는 믿을 수 없다).
FIXTURE="${KUBEMETAL_LICENSE_FIXTURE:-}"

# --self-test: 정책 판정 자체가 맞는지 본다. 한 번도 실패해본 적 없는 게이트는
# 통과를 보고할 자격이 없다 — 특히 "GPL이면 막는다"는 주장은 GPL을 실제로 막는
# 것을 보여야 성립한다.
if [ "${1:-}" = "--self-test" ]; then
  tmp="$(mktemp)"; trap 'rm -f "$tmp"' EXIT
  fails=0
  run_case() {
    printf '%b' "$3" > "$tmp"
    if KUBEMETAL_LICENSE_FIXTURE="$tmp" "$0" >/dev/null 2>&1; then got=PASS; else got=FAIL; fi
    if [ "$got" = "$2" ]; then
      echo "  ok   $1"
    else
      echo "  BAD  $1 (기대=$2, 실제=$got)"; fails=$((fails + 1))
    fi
  }
  run_case "GPL-3.0 단독"           FAIL 'evil\t1.0\tGPL-3.0-only\n'
  run_case "AGPL-3.0"               FAIL 'evil\t1.0\tAGPL-3.0\n'
  run_case "SSPL-1.0"               FAIL 'mongo\t1.0\tSSPL-1.0\n'
  run_case "BUSL-1.1"               FAIL 'hashi\t1.0\tBUSL-1.1\n'
  run_case "라이선스 비어 있음"       FAIL 'mystery\t1.0\t\n'
  run_case "AND에 금지 포함"         FAIL 'x\t1.0\tMIT AND GPL-3.0-only\n'
  run_case "대롱거리는 OR"           FAIL 'x\t1.0\tMIT OR\n'
  run_case "괄호 불균형"             FAIL 'x\t1.0\t(MIT OR Apache-2.0\n'
  run_case "MIT"                    PASS 'a\t1.0\tMIT\n'
  run_case "OR에 LGPL 대안 존재"     PASS 'r-efi\t6.0\tMIT OR Apache-2.0 OR LGPL-2.1-or-later\n'
  run_case "슬래시 구식 표기"         PASS 'b\t1.0\tMIT/Apache-2.0\n'
  run_case "WITH 예외"               PASS 'c\t1.0\tApache-2.0 WITH LLVM-exception\n'
  run_case "괄호 + AND"              PASS 'd\t1.0\t(MIT OR Apache-2.0) AND Unicode-3.0\n'
  run_case "AND 양쪽 허용"           PASS 'e\t1.0\tBSD-3-Clause AND MIT\n'
  [ "$fails" -eq 0 ] || { echo "self-test 실패 ${fails}건"; exit 1; }
  echo "self-test 통과 (14건)"
  exit 0
fi

{
  if [ -n "$FIXTURE" ]; then
    cat "$FIXTURE"
  else
    cargo metadata --manifest-path "${PROJECT_ROOT}/src-tauri/Cargo.toml" --format-version 1 \
      | python3 -c '
import json, sys
for p in json.load(sys.stdin)["packages"]:
    if p["name"] != "kubemetal":
        print("{}\t{}\t{}".format(p["name"], p["version"], p.get("license") or ""))'
    (cd "${PROJECT_ROOT}" && pnpm licenses list --prod --json) \
      | python3 -c '
import json, sys
for lic, pkgs in json.load(sys.stdin).items():
    for p in pkgs:
        print("{}\t{}\t{}".format(p["name"], ",".join(p.get("versions", [])), lic))'
  fi
} | python3 -c '
import re, sys

# 허용: OSI 승인 퍼미시브 + 퍼블릭 도메인 계열. 여기 없는 것은 자동 통과하지 않는다.
ALLOWED = {
    "MIT", "MIT-0", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC",
    "Zlib", "0BSD", "Unlicense", "CC0-1.0", "Unicode-3.0", "Unicode-DFS-2016",
    "BSL-1.0", "MPL-2.0",  # MPL-2.0은 약한 카피레프트 — 허용하되 아래에서 별도 고지
}
# 약한 카피레프트: 통과시키되 소스 제공 의무가 붙으므로 NOTICE에 적혀 있어야 한다.
WEAK_COPYLEFT = {"MPL-2.0"}

def normalize(expr):
    expr = expr.replace("/", " OR ")
    expr = re.sub(r"\s+WITH\s+[A-Za-z0-9.\-]+", "", expr)  # 예외 조항은 기반 라이선스로 환원
    return expr.strip()

class ParseError(Exception):
    pass

def allowed(expr):
    """SPDX 식을 재귀 하향 파싱해 평가한다.

    expr := term (OR term)*   — 하나만 허용돼도 통과(우리가 그 대안을 고른다)
    term := factor (AND factor)*
    factor := "(" expr ")" | IDENT

    eval()을 쓰지 않는다 — 입력이 외부 lockfile에서 오므로, 안전함을 논증해야
    하는 코드보다 논증이 필요 없는 코드가 낫다. 파싱 실패는 통과가 아니라 실패다.
    """
    tokens = re.findall(r"\(|\)|[A-Za-z0-9.\-+]+", normalize(expr))
    if not tokens:
        return False
    pos = 0

    def peek():
        return tokens[pos] if pos < len(tokens) else None

    def parse_expr():
        nonlocal pos
        value = parse_term()
        while peek() == "OR":
            pos += 1
            value = parse_term() or value
        return value

    def parse_term():
        nonlocal pos
        value = parse_factor()
        while peek() == "AND":
            pos += 1
            value = parse_factor() and value
        return value

    def parse_factor():
        nonlocal pos
        tok = peek()
        if tok is None or tok in ("OR", "AND", ")"):
            raise ParseError(tok)
        pos += 1
        if tok == "(":
            value = parse_expr()
            if peek() != ")":
                raise ParseError("unbalanced")
            pos += 1
            return value
        return tok in ALLOWED

    try:
        result = parse_expr()
    except ParseError:
        return False
    return result if pos == len(tokens) else False

violations, unknown, weak = [], [], []
total = 0
for line in sys.stdin:
    line = line.rstrip("\n")
    if not line:
        continue
    name, version, lic = line.split("\t")
    total += 1
    if not lic.strip():
        unknown.append((name, version))
    elif not allowed(lic):
        violations.append((name, version, lic))
    elif any(w in normalize(lic) for w in WEAK_COPYLEFT):
        weak.append((name, version, lic))

print("검사한 구성요소: {}개".format(total))
if weak:
    print("\n약한 카피레프트 {}건 — NOTICE에 소스 입수 안내가 있어야 한다:".format(len(weak)))
    for n, v, l in weak:
        print("  - {} {} — {}".format(n, v, l))
if unknown:
    print("\n라이선스 미상 {}건:".format(len(unknown)))
    for n, v in unknown:
        print("  - {} {}".format(n, v))
if violations:
    print("\n정책 위반 {}건:".format(len(violations)))
    for n, v, l in violations:
        print("  - {} {} — {}".format(n, v, l))

if violations or unknown:
    print("\nFAIL: 허용 목록 밖이거나 미상인 라이선스가 있다. 정책을 바꾸려면"
          " check_licenses.sh의 ALLOWED와 NOTICE를 함께 고쳐야 한다.")
    sys.exit(1)
print("\nOK: 번들 의존성 라이선스가 모두 정책 안에 있다.")
'
