#!/usr/bin/env bash
# ==============================================================================
# gen_dependency_diff.sh — 업그레이드 전후 의존성/라이선스 diff 리포트 (이슈 #9)
#
# check_licenses.sh는 "지금" 시점의 정책 위반을 막는다. 이 스크립트는 "지난
# 릴리스와 비교해 뭐가 바뀌었는지"를 사람이 검토하기 위한 리포트다 — 게이트가
# 아니라 리뷰 보조 도구이므로 실패해도 release를 막지 않는다.
#
# 두 git ref(기본: 직전 태그 vs HEAD)에서 각각 cargo metadata + pnpm licenses로
# name/version/license 목록을 뽑아 비교한다. 비교 자체(추가/삭제/버전 변경/
# 라이선스 변경 판정)는 순수 함수라 --self-test로 고정 입력에 대해 검증한다;
# git worktree/cargo/pnpm을 실제로 구동하는 추출 부분은 self-test 대상이 아니다
# (환경 의존적이라 재현 가능한 음성 대조군을 만들 수 없다).
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

usage() {
  echo "사용법: $0 [<base-ref>] [<target-ref>]" >&2
  echo "       base-ref 기본값: target-ref 바로 이전 태그" >&2
  echo "       target-ref 기본값: HEAD" >&2
  echo "       $0 --self-test" >&2
  exit 2
}

# name\tversion\tlicense 두 목록을 비교해 리포트를 stdout에 쓴다.
# 순수 함수 — git/cargo/pnpm을 모른다.
diff_report() {
  local base_tsv="$1" target_tsv="$2"
  python3 -c '
import sys

def load(path):
    rows = {}
    with open(path) as f:
        for line in f:
            line = line.rstrip("\n")
            if not line:
                continue
            name, version, lic = line.split("\t")
            rows[name] = (version, lic)
    return rows

base = load(sys.argv[1])
target = load(sys.argv[2])

added = sorted(set(target) - set(base))
removed = sorted(set(base) - set(target))
common = sorted(set(base) & set(target))
version_changed = [n for n in common if base[n][0] != target[n][0]]
license_changed = [n for n in common if base[n][1] != target[n][1]]

print("의존성 diff: base={} target={}".format(len(base), len(target)))
print()
print("추가 {}건:".format(len(added)))
for n in added:
    print("  + {} {} — {}".format(n, *target[n]))
print()
print("삭제 {}건:".format(len(removed)))
for n in removed:
    print("  - {} {} — {}".format(n, *base[n]))
print()
print("버전 변경 {}건:".format(len(version_changed)))
for n in version_changed:
    print("  ~ {}: {} -> {}".format(n, base[n][0], target[n][0]))
print()
print("라이선스 변경 {}건:".format(len(license_changed)))
for n in license_changed:
    print("  ! {}: {!r} -> {!r}".format(n, base[n][1], target[n][1]))

if license_changed:
    print()
    print("주의: 라이선스가 바뀐 의존성은 check_licenses.sh 정책 재평가와 NOTICE 갱신이 필요할 수 있다.")
' "$base_tsv" "$target_tsv"
}

if [ "${1:-}" = "--self-test" ]; then
  tmp_base="$(mktemp)"; tmp_target="$(mktemp)"
  trap 'rm -f "$tmp_base" "$tmp_target"' EXIT

  printf 'a\t1.0\tMIT\nb\t2.0\tMIT\nc\t1.0\tMPL-2.0\n' > "$tmp_base"
  printf 'a\t1.1\tMIT\nc\t1.0\tGPL-3.0-only\nd\t1.0\tMIT\n' > "$tmp_target"

  out="$(diff_report "$tmp_base" "$tmp_target")"
  fails=0
  check() {
    if echo "$out" | grep -qF -- "$1"; then
      echo "  ok   $1"
    else
      echo "  BAD  기대한 줄을 못 찾음: $1"; fails=$((fails + 1))
    fi
  }
  check "추가 1건:"
  check "+ d 1.0 — MIT"
  check "삭제 1건:"
  check "- b 2.0 — MIT"
  check "버전 변경 1건:"
  check "~ a: 1.0 -> 1.1"
  check "라이선스 변경 1건:"
  check "! c: 'MPL-2.0' -> 'GPL-3.0-only'"
  [ "$fails" -eq 0 ] || { echo "self-test 실패 ${fails}건"; exit 1; }
  echo "self-test 통과 (6건)"
  exit 0
fi

# extract <ref> — 해당 ref를 임시 worktree에 체크아웃해 name/version/license
# tsv를 stdout에 낸다. check_licenses.sh의 추출 파이프라인과 동일한 형식이다.
extract() {
  local ref="$1"
  local wt
  wt="$(mktemp -d)"
  git -C "$PROJECT_ROOT" worktree add --detach --quiet "$wt" "$ref" >&2

  {
    cargo metadata --manifest-path "${wt}/src-tauri/Cargo.toml" --format-version 1 --locked \
      | python3 -c '
import json, sys
for p in json.load(sys.stdin)["packages"]:
    if p["name"] != "kubemetal":
        print("{}\t{}\t{}".format(p["name"], p["version"], p.get("license") or ""))'
    (
      cd "$wt"
      pnpm install --frozen-lockfile --silent >&2 2>&1
      pnpm licenses list --prod --json
    ) | python3 -c '
import json, sys
for lic, pkgs in json.load(sys.stdin).items():
    for p in pkgs:
        print("{}\t{}\t{}".format(p["name"], ",".join(p.get("versions", [])), lic))'
  } | sort -u

  git -C "$PROJECT_ROOT" worktree remove --force "$wt" >&2
}

if [ $# -ge 2 ]; then
  base_ref="$1"
  target_ref="$2"
elif [ $# -eq 1 ]; then
  base_ref="$1"
  target_ref="HEAD"
else
  base_ref="$(git -C "$PROJECT_ROOT" describe --tags --abbrev=0 HEAD^ 2>/dev/null)" || {
    echo "FAIL: 직전 태그를 찾을 수 없습니다. base-ref를 직접 지정하세요." >&2
    usage
  }
  target_ref="HEAD"
fi

echo "base=${base_ref} target=${target_ref}" >&2
base_tsv="$(mktemp)"; target_tsv="$(mktemp)"
trap 'rm -f "$base_tsv" "$target_tsv"' EXIT
extract "$base_ref" > "$base_tsv"
extract "$target_ref" > "$target_tsv"
diff_report "$base_tsv" "$target_tsv"
