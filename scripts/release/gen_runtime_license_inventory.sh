#!/usr/bin/env bash
# ==============================================================================
# gen_runtime_license_inventory.sh — MLX venv(런타임) 라이선스 인벤토리 생성 (이슈 #9)
#
# check_licenses.sh는 바이너리에 컴파일되는 것만 본다. mlx.rs가 pip install로
# 사용자 macOS에 올리는 ~/.kubemetal/venv(mlx-lm/mlx-vlm 등, 버전 미고정,
# requirements/constraints 파일 없음)는 그 범위 밖이지만, 릴리스 증거로서
# "무엇이 설치되는가"를 기계 판독 가능하게 남겨야 한다(이슈 #9).
#
# check_licenses.sh와의 차이(중요): 여기서 잡히는 GPL/LGPL/AGPL/MPL 등은
# PyPI에서 사용자 자신의 요청으로 사용자 자신의 기기에 설치되는 것이지 KubeMetal
# 릴리스 산출물에 재배포되는 것이 아니다. 따라서 금지 목록으로 막지 않는다 —
# 이 스크립트의 임무는 "무엇이 깔리는가"의 완전성과 정확한 고지이지, 정책
# 차단이 아니다. 다만 UNKNOWN(라이선스 식별 불가)은 다른 이야기다: 모르는 것은
# 통과가 아니라는 이 저장소의 원칙(D22, check_licenses.sh와 동일)이 여기도
# 적용돼 fail-closed로 막는다.
#
# venv가 없으면(앱이 온디맨드로 만들기 전) 인벤토리를 지어내지 않는다(D22) —
# 존재하지 않는 venv에 대한 가짜 결과를 만들지 말고 실패를 그대로 드러낸다.
#
# 라이선스 추출 우선순위 (PEP 639 → 구식 필드 순, 히트한 출처를 license_source에
# 기록한다):
#   1. License-Expression (PEP 639 SPDX 식) — 비어있지 않으면 그대로 사용
#   2. Classifier의 "License :: ..." — "OSI Approved" 세그먼트는 버리고 마지막
#      세그먼트를 취함. 여러 classifier가 있으면 정렬된 유일값을 " | "로 결합.
#   3. License 필드 — 비어있지 않고 60자 미만일 때만 사용한다. 그 이상이면
#      식별자가 아니라 라이선스 전문이 박혀 있다는 뜻이라 식별자로 못 쓴다.
#   4. 위 셋 다 없으면 UNKNOWN (license_source: NONE)
#
# 감사된 오버라이드 (이슈 #9 후속): tiktoken/word2number처럼 License 필드에
# 식별자 대신 라이선스 전문이 박혀 있어 위 세 규칙이 전부 UNKNOWN을 내는
# 실제 사례가 있다. 이런 경우 전문을 파싱해 SPDX를 추측하면 그게 바로 D22가
# 금지하는 "상태 지어내기"이므로, 대신 사람이 verified_source의 실제 라이선스
# 텍스트를 직접 읽고 scripts/release/runtime-license-overrides.json에 기록한
# 값만 쓴다. 오버라이드는 규칙 1-3이 전부 UNKNOWN을 낸 패키지에만 적용되며
# (이미 식별된 라이선스를 절대 덮어쓰지 않음), 적용되면 license_source를
# "override"로 표시하고 verified_source를 산출물에 남겨 사람이 확인한
# 값임을 드러낸다. --no-overrides로 끌 수 있고, 오버라이드 파일이 없으면
# 조용히 무시한다(오버라이드 없이도 동작해야 함) — 단, UNKNOWN은 여전히
# fail-closed다.
#
# 애매한 분류자(예: "BSD License")를 정밀 SPDX(BSD-3-Clause 등)로 추측해
# 승격하지 않는다 — 어떤 BSD 변종인지 classifier가 말하지 않는데 추측하면
# 그게 바로 D22가 금지하는 "상태 지어내기"다. 원문 그대로 보존한다.
#
# 어긋남 가드: venv 기본 경로($HOME/.kubemetal/venv)가 이 스크립트와
# src-tauri/src/commands/mlx.rs::venv_dir()에 이중으로 있다. 한쪽만 바뀌면
# 조용히 어긋나므로(AGENTS.md: "같은 사실이 두 곳에 있으면 이미 틀린 것") mlx.rs가
# 여전히 ".kubemetal"과 "venv"를 함께 포함하는지 매 실행마다 검사하고, self-test
# 에도 포함한다.
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
MLX_RS="${PROJECT_ROOT}/src-tauri/src/commands/mlx.rs"
DEFAULT_OVERRIDES="${SCRIPT_DIR}/runtime-license-overrides.json"

usage() {
  echo "사용법: $0 [--venv <path>] [--out-dir <dir>] [--overrides <path>|--no-overrides]" >&2
  echo "       $0 --self-test" >&2
  echo "       $0 -h|--help" >&2
  echo "" >&2
  echo "옵션:" >&2
  echo "  --venv <path>      검사할 venv 경로 (기본값: \$HOME/.kubemetal/venv)" >&2
  echo "  --out-dir <dir>    출력 디렉토리 (기본값: 프로젝트 루트)" >&2
  echo "  --overrides <path> 감사된 오버라이드 파일 경로 (기본값: ${DEFAULT_OVERRIDES})" >&2
  echo "  --no-overrides     오버라이드 파일을 사용하지 않음" >&2
  echo "  --self-test        모의 fixture 기반 자체 테스트 실행" >&2
  echo "  -h, --help         사용법 출력" >&2
  exit 2
}

# mlx.rs venv_dir()의 정의 텍스트가 여전히 ".kubemetal"과 "venv"를 함께 쓰는지
# 검사하는 순수 함수 — self-test가 합성 입력으로도 검증할 수 있게 분리한다.
check_mlx_divergence_content() {
  local content="$1"
  echo "$content" | grep -q "\.kubemetal" && echo "$content" | grep -q "venv"
}

check_mlx_divergence_file() {
  if [ ! -f "$MLX_RS" ]; then
    return 0
  fi
  if ! check_mlx_divergence_content "$(cat "$MLX_RS")"; then
    echo "오류: ${MLX_RS}의 venv_dir()이 더 이상 \".kubemetal\"과 \"venv\"를 함께 쓰지 않는 것으로 보입니다 — 이 스크립트의 기본 venv 경로와 어긋났을 수 있습니다. 두 곳을 맞추세요." >&2
    return 1
  fi
  return 0
}

# 실제 인벤토리 생성 로직 (python). fixture 환경변수가 있으면 실제 venv 대신
# 합성 메타데이터를 쓴다 — self-test가 네트워크/venv 없이 정책과 파싱을
# 검증할 수 있게 한다.
write_gen_py() {
  cat > "$1" <<'PYEOF'
import json
import os
import re
import sys


def extract_license(license_expression, license_field, classifiers):
    """(license, license_source)를 반환한다. 우선순위:
    1) License-Expression  2) Classifier  3) License 필드(60자 미만)  4) UNKNOWN
    """
    if license_expression and license_expression.strip():
        return license_expression.strip(), "License-Expression"

    values = []
    for c in classifiers or []:
        if not isinstance(c, str) or not c.startswith("License"):
            continue
        parts = [p.strip() for p in c.split("::")]
        parts = [p for p in parts if p and p != "OSI Approved"]
        if len(parts) <= 1:
            # "License" 접두만 남으면(예: "License :: OSI Approved"만 있는 경우)
            # 실제 식별자가 없는 것이므로 이 classifier는 쓰지 않는다.
            continue
        values.append(parts[-1])
    if values:
        # 정렬된 유일값을 결합한다 — 애매한 값을 정밀 SPDX로 승격하지 않고
        # classifier가 말한 원문을 그대로 보존한다(예: "BSD License").
        return " | ".join(sorted(set(values))), "Classifier"

    if license_field and license_field.strip() and len(license_field.strip()) < 60:
        return license_field.strip(), "License"

    return "UNKNOWN", "NONE"


COPYLEFT_RE = re.compile(r"GPL|LGPL|AGPL|MPL|Artistic|SSPL|BUSL", re.IGNORECASE)


def load_overrides(path):
    """감사된 오버라이드 파일을 읽어 {정규화된 name: {license, verified_source, note}}
    를 반환한다. 경로가 비어있거나 파일이 없으면 빈 dict(오버라이드 없음) —
    이는 오류가 아니다. JSON이 깨졌거나 항목이 불완전하면 하드 에러(exit 1):
    출처 없는 오버라이드는 증거가 아니다.
    """
    if not path:
        return {}
    if not os.path.isfile(path):
        return {}

    with open(path, "r", encoding="utf-8") as f:
        raw = f.read()
    try:
        data = json.loads(raw)
    except json.JSONDecodeError as e:
        print(
            "오류: 오버라이드 파일의 JSON이 올바르지 않습니다: {} ({})".format(path, e),
            file=sys.stderr,
        )
        sys.exit(1)

    result = {}
    for entry in data.get("overrides", []):
        if not isinstance(entry, dict):
            print(
                "오류: 오버라이드 항목이 객체가 아닙니다 (파일: {}): {}".format(path, entry),
                file=sys.stderr,
            )
            sys.exit(1)
        for field in ("name", "license", "verified_source"):
            value = entry.get(field)
            if not isinstance(value, str) or not value.strip():
                print(
                    "오류: 오버라이드 항목의 '{}' 필드가 없거나 비어 있습니다 (파일: {}): {}".format(
                        field, path, entry
                    ),
                    file=sys.stderr,
                )
                sys.exit(1)
        key = entry["name"].strip().lower()
        result[key] = {
            "license": entry["license"].strip(),
            "verified_source": entry["verified_source"].strip(),
        }
    return result


def gather_packages(fixture_path):
    if fixture_path:
        with open(fixture_path, "r", encoding="utf-8") as f:
            data = json.load(f)
        return data.get("packages", [])

    import importlib.metadata as im

    pkgs = []
    for d in im.distributions():
        meta = d.metadata
        name = meta.get("Name") or "UNKNOWN"
        version = meta.get("Version") or "0"
        classifiers = meta.get_all("Classifier") or []
        pkgs.append(
            {
                "name": name,
                "version": version,
                "license_expression": meta.get("License-Expression"),
                "license_field": meta.get("License"),
                "classifiers": classifiers,
            }
        )
    return pkgs


def main():
    out_file = sys.argv[1]
    venv_path = sys.argv[2] if len(sys.argv) > 2 else ""
    overrides_path = sys.argv[3] if len(sys.argv) > 3 else ""
    fixture_path = os.environ.get("KUBEMETAL_RUNTIME_LICENSE_FIXTURE", "")

    packages_raw = gather_packages(fixture_path)
    overrides = load_overrides(overrides_path)

    inventory = []
    unknowns = []
    copyleft = []
    overridden = []
    for p in packages_raw:
        name = p.get("name") or "UNKNOWN"
        version = p.get("version") or "0"
        lic, source = extract_license(
            p.get("license_expression"), p.get("license_field"), p.get("classifiers")
        )

        record = {"name": name, "version": version, "license": lic, "license_source": source}

        # 가드: 오버라이드는 규칙 1-3이 전부 UNKNOWN(license_source == "NONE")을
        # 낸 패키지에만 적용한다 — 메타데이터가 이미 선언한 라이선스는 절대
        # 덮어쓰지 않는다.
        if lic == "UNKNOWN" and source == "NONE":
            ov = overrides.get(name.strip().lower())
            if ov:
                record["license"] = ov["license"]
                record["license_source"] = "override"
                record["verified_source"] = ov["verified_source"]
                lic = ov["license"]
                overridden.append(
                    "{}=={} : {} ({})".format(name, version, lic, ov["verified_source"])
                )

        inventory.append(record)
        if lic == "UNKNOWN":
            unknowns.append("{}=={}".format(name, version))
        elif COPYLEFT_RE.search(lic):
            copyleft.append("{}=={} ({})".format(name, version, lic))

    inventory.sort(key=lambda x: x["name"].lower())
    copyleft.sort()
    overridden.sort()

    result = {
        "generated_from": {
            "venv": venv_path,
            "python_version": "{}.{}.{}".format(*sys.version_info[:3]),
        },
        "packages": inventory,
        "summary": {
            "total": len(inventory),
            "unknown": len(unknowns),
            "copyleft": copyleft,
            "overridden": len(overridden),
        },
    }

    out_dir = os.path.dirname(os.path.abspath(out_file))
    os.makedirs(out_dir, exist_ok=True)
    with open(out_file, "w", encoding="utf-8") as f:
        json.dump(result, f, indent=2, ensure_ascii=False, sort_keys=False)
        f.write("\n")

    print("생성: {} (패키지 {}개)".format(out_file, len(inventory)))

    if overridden:
        print("")
        print("오버라이드: 사람이 감사한 라이선스 {}건 적용됨 (자동 추출이 아님):".format(len(overridden)))
        for o in overridden:
            print("  - {}".format(o))

    if copyleft:
        print("")
        print(
            "주의: 카피레프트 계열 라이선스 {}건 — 재배포 금지 목록의 대상이 아니다".format(
                len(copyleft)
            )
        )
        print("(PyPI에서 사용자 자신의 기기에 설치되는 것). 고지 확인만 필요:")
        for c in copyleft:
            print("  - {}".format(c))

    if unknowns:
        print("")
        print(
            "FAIL: 라이선스를 식별할 수 없는 패키지 {}건 — 모르는 것은 통과가 아닙니다:".format(
                len(unknowns)
            )
        )
        for u in unknowns:
            print("  - {}".format(u))
        sys.exit(1)

    print("")
    print("OK: 모든 런타임 패키지의 라이선스가 확인되었습니다.")
    sys.exit(0)


main()
PYEOF
}

# ------------------------------------------------------------------------------
# --self-test: 합성 fixture로 추출 규칙과 fail-closed 정책을 검증한다. 네트워크나
# 실제 venv가 필요 없다.
# ------------------------------------------------------------------------------
if [ "${1:-}" = "--self-test" ]; then
  tmp_fixture="$(mktemp)"
  tmp_out_a="$(mktemp -d)"
  tmp_out_b="$(mktemp -d)"
  tmp_out_c="$(mktemp -d)"
  tmp_overrides_ok="$(mktemp)"
  tmp_overrides_missing_source="$(mktemp)"
  tmp_overrides_blank_name="$(mktemp)"
  tmp_overrides_blank_license="$(mktemp)"
  tmp_overrides_malformed="$(mktemp)"
  trap 'rm -f "$tmp_fixture" "$tmp_overrides_ok" "$tmp_overrides_missing_source" "$tmp_overrides_blank_name" "$tmp_overrides_blank_license" "$tmp_overrides_malformed"; rm -rf "$tmp_out_a" "$tmp_out_b" "$tmp_out_c"' EXIT
  fails=0
  total=0

  echo "gen_runtime_license_inventory self-test 시작..."

  write_fixture() { printf '%s' "$1" > "$tmp_fixture"; }

  run_case() {
    local desc="$1" expected="$2" fixture_json="$3" extra_args="${4:-}"
    local case_out
    case_out="$(mktemp -d)"
    write_fixture "$fixture_json"
    local got
    if KUBEMETAL_RUNTIME_LICENSE_FIXTURE="$tmp_fixture" "$0" --out-dir "$case_out" $extra_args >/dev/null 2>&1; then
      got=PASS
    else
      got=FAIL
    fi
    rm -rf "$case_out"
    total=$((total + 1))
    if [ "$got" = "$expected" ]; then
      echo "  ok   $desc"
    else
      echo "  BAD  $desc (기대=$expected, 실제=$got)"
      fails=$((fails + 1))
    fi
  }

  ALL_KNOWN_FIXTURE='{"packages":[
    {"name":"pkg-expr","version":"1.0","license_expression":"MIT",
     "classifiers":["License :: OSI Approved :: Apache Software License"],
     "license_field":"Apache-2.0"},
    {"name":"pkg-cls","version":"1.0","license_expression":null,
     "classifiers":["License :: OSI Approved :: MIT License"],"license_field":null},
    {"name":"pkg-multi","version":"1.0","license_expression":null,
     "classifiers":["License :: OSI Approved :: MIT License",
                    "License :: OSI Approved :: Apache Software License",
                    "License :: OSI Approved :: MIT License"],
     "license_field":null},
    {"name":"pkg-field","version":"1.0","license_expression":null,
     "classifiers":[],"license_field":"BSD-3-Clause"},
    {"name":"pkg-gpl","version":"1.0","license_expression":null,
     "classifiers":[],"license_field":"GPL-3.0-only"},
    {"name":"pkg-bsd","version":"1.0","license_expression":null,
     "classifiers":["License :: OSI Approved :: BSD License"],"license_field":null}
  ]}'

  # 1: License-Expression 우선 / 4: License 필드 fallback / 10: 카피레프트는
  # 실패시키지 않음 — 이 조합은 모두 알려진 라이선스라 exit 0이어야 한다.
  run_case "PASS: 전부 식별된 조합(all-known) -> exit 0 (케이스 1,2,3,4,9,10,11 소재)" \
    PASS "$ALL_KNOWN_FIXTURE"

  # 5: License 필드가 60자 이상(전문 임베드) -> UNKNOWN
  run_case "FAIL: License 필드 60자 이상 -> UNKNOWN" FAIL '{"packages":[
    {"name":"pkg-longtext","version":"1.0","license_expression":null,
     "classifiers":[],
     "license_field":"This is a very long embedded license text that goes on and on well past the sixty character threshold for sure"}
  ]}'

  # 6: 라이선스 메타데이터 전무 -> UNKNOWN
  run_case "FAIL: 라이선스 메타데이터 전무 -> UNKNOWN" FAIL '{"packages":[
    {"name":"pkg-none","version":"1.0","license_expression":null,
     "classifiers":[],"license_field":null}
  ]}'

  # 7: 공백만 있는 라이선스 -> UNKNOWN
  run_case "FAIL: 공백뿐인 License 필드 -> UNKNOWN" FAIL '{"packages":[
    {"name":"pkg-blank","version":"1.0","license_expression":"   ",
     "classifiers":[],"license_field":"   "}
  ]}'

  # 8: 알려진 패키지 + UNKNOWN 혼재 -> 전체 실패 (음성 대조군: 게이트가 실제로 막는지)
  run_case "FAIL: 알려진 패키지와 UNKNOWN 혼재 -> 전체 실패" FAIL '{"packages":[
    {"name":"pkg-known","version":"1.0","license_expression":"MIT",
     "classifiers":[],"license_field":null},
    {"name":"pkg-unknown2","version":"1.0","license_expression":null,
     "classifiers":[],"license_field":null}
  ]}'

  echo "  --- 산출물 내용/스키마 검증 ---"

  write_fixture "$ALL_KNOWN_FIXTURE"
  KUBEMETAL_RUNTIME_LICENSE_FIXTURE="$tmp_fixture" "$0" --out-dir "$tmp_out_a" >/dev/null 2>&1

  content_py="$(mktemp -t kubemetal-runtime-license-check-XXXXXX.py)"
  cat > "$content_py" <<'PYEOF'
import json
import sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8") as f:
    data = json.load(f)

by_name = {p["name"]: p for p in data["packages"]}


def check(desc, cond):
    print("  {}   {}".format("ok" if cond else "BAD", desc))


check(
    "License-Expression 우선순위 (classifier/License 필드보다 우선)",
    by_name["pkg-expr"]["license"] == "MIT"
    and by_name["pkg-expr"]["license_source"] == "License-Expression",
)
check(
    "classifier fallback, OSI Approved 세그먼트 제거",
    by_name["pkg-cls"]["license"] == "MIT License"
    and by_name["pkg-cls"]["license_source"] == "Classifier",
)
check(
    "복수 classifier 정렬+유일값 결합",
    by_name["pkg-multi"]["license"] == "Apache Software License | MIT License",
)
check(
    "License 필드 fallback (60자 미만)",
    by_name["pkg-field"]["license"] == "BSD-3-Clause"
    and by_name["pkg-field"]["license_source"] == "License",
)
check(
    "카피레프트가 summary.copyleft에 감지되지만 전체 실패는 아님",
    "pkg-gpl==1.0 (GPL-3.0-only)" in data["summary"]["copyleft"],
)
check(
    "애매한 'BSD License' 원문 보존 (정밀 SPDX로 승격 금지)",
    by_name["pkg-bsd"]["license"] == "BSD License",
)
names = [p["name"].lower() for p in data["packages"]]
check("packages가 소문자 이름 기준으로 정렬됨", names == sorted(names))
check(
    "스키마: generated_from/summary 필드 존재",
    "venv" in data["generated_from"]
    and "python_version" in data["generated_from"]
    and {"total", "unknown", "copyleft"}.issubset(data["summary"].keys()),
)
PYEOF
  content_out="$(python3 "$content_py" "${tmp_out_a}/runtime-license-inventory.json")"
  rm -f "$content_py"
  echo "$content_out"
  content_bad=$(printf '%s\n' "$content_out" | grep -c "BAD" || true)
  content_ok=$(printf '%s\n' "$content_out" | grep -c "  ok" || true)
  fails=$((fails + content_bad))
  total=$((total + content_bad + content_ok))

  # 13: 결정적 산출물 — 같은 fixture는 바이트 동일한 JSON을 낸다.
  KUBEMETAL_RUNTIME_LICENSE_FIXTURE="$tmp_fixture" "$0" --out-dir "$tmp_out_b" >/dev/null 2>&1
  total=$((total + 1))
  if diff -q "${tmp_out_a}/runtime-license-inventory.json" "${tmp_out_b}/runtime-license-inventory.json" >/dev/null 2>&1; then
    echo "  ok   결정적 산출물 (동일 fixture -> 바이트 동일 JSON)"
  else
    echo "  BAD  결정적 산출물 (동일 fixture인데 산출물이 다름)"
    fails=$((fails + 1))
  fi

  echo "  --- 오버라이드 메커니즘 검증 ---"

  # 오버라이드 적용(UNKNOWN 패키지) + 오버라이드 무시(이미 선언된 패키지, 음성
  # 대조군)를 한 fixture/파일 조합으로 함께 검증한다.
  cat > "$tmp_overrides_ok" <<'JSONEOF'
{
  "_README": "테스트용 오버라이드 파일",
  "overrides": [
    {"name": "pkg-custom-unknown", "license": "MIT",
     "verified_source": "https://example.com/license",
     "note": "UNKNOWN 패키지에 오버라이드가 적용되는지 검증"},
    {"name": "pkg-declared", "license": "GPL-3.0-only",
     "verified_source": "https://example.com/other",
     "note": "이미 선언된 패키지에는 오버라이드가 절대 적용되면 안 됨(음성 대조군)"}
  ]
}
JSONEOF

  OVERRIDE_MIX_FIXTURE='{"packages":[
    {"name":"pkg-custom-unknown","version":"1.0","license_expression":null,
     "classifiers":[],"license_field":null},
    {"name":"pkg-declared","version":"1.0","license_expression":"Apache-2.0",
     "classifiers":[],"license_field":null}
  ]}'

  run_case "PASS: UNKNOWN 패키지에 오버라이드 적용 + 이미 선언된 패키지는 무시 -> exit 0" \
    PASS "$OVERRIDE_MIX_FIXTURE" "--overrides $tmp_overrides_ok"

  write_fixture "$OVERRIDE_MIX_FIXTURE"
  override_stdout="$(KUBEMETAL_RUNTIME_LICENSE_FIXTURE="$tmp_fixture" "$0" --out-dir "$tmp_out_c" --overrides "$tmp_overrides_ok" 2>&1)"

  total=$((total + 1))
  if printf '%s\n' "$override_stdout" | grep -q "오버라이드"; then
    echo "  ok   오버라이드 섹션이 stdout에 표시됨"
  else
    echo "  BAD  오버라이드 섹션이 stdout에 표시되지 않음"
    fails=$((fails + 1))
  fi

  override_content_py="$(mktemp -t kubemetal-runtime-license-override-check-XXXXXX.py)"
  cat > "$override_content_py" <<'PYEOF'
import json
import sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8") as f:
    data = json.load(f)

by_name = {p["name"]: p for p in data["packages"]}


def check(desc, cond):
    print("  {}   {}".format("ok" if cond else "BAD", desc))


check(
    "UNKNOWN 패키지에 오버라이드 적용 (license/license_source/verified_source)",
    by_name["pkg-custom-unknown"]["license"] == "MIT"
    and by_name["pkg-custom-unknown"]["license_source"] == "override"
    and by_name["pkg-custom-unknown"].get("verified_source")
    == "https://example.com/license",
)
check(
    "이미 선언된 패키지는 오버라이드가 적용되지 않음 (원래 값 유지, verified_source 없음)",
    by_name["pkg-declared"]["license"] == "Apache-2.0"
    and by_name["pkg-declared"]["license_source"] == "License-Expression"
    and "verified_source" not in by_name["pkg-declared"],
)
check(
    "summary.overridden 카운트가 정확함 (적용 1건만 집계)",
    data["summary"]["overridden"] == 1,
)
PYEOF
  override_content_out="$(python3 "$override_content_py" "${tmp_out_c}/runtime-license-inventory.json")"
  rm -f "$override_content_py"
  echo "$override_content_out"
  override_content_bad=$(printf '%s\n' "$override_content_out" | grep -c "BAD" || true)
  override_content_ok=$(printf '%s\n' "$override_content_out" | grep -c "  ok" || true)
  fails=$((fails + override_content_bad))
  total=$((total + override_content_bad + override_content_ok))

  # 오버라이드 항목 검증: 필수 필드 누락/공백은 하드 에러(exit 1) — 출처 없는
  # 오버라이드는 증거가 아니다. all-known fixture를 써서 실패 원인이
  # UNKNOWN 정책이 아니라 오버라이드 파일 자체임을 격리한다.
  printf '%s' '{"_README":"t","overrides":[{"name":"pkg-x","license":"MIT"}]}' > "$tmp_overrides_missing_source"
  run_case "FAIL: 오버라이드 항목에 verified_source 누락 -> exit 1" \
    FAIL "$ALL_KNOWN_FIXTURE" "--overrides $tmp_overrides_missing_source"

  printf '%s' '{"_README":"t","overrides":[{"name":"","license":"MIT","verified_source":"https://example.com"}]}' > "$tmp_overrides_blank_name"
  run_case "FAIL: 오버라이드 항목의 name이 공백 -> exit 1" \
    FAIL "$ALL_KNOWN_FIXTURE" "--overrides $tmp_overrides_blank_name"

  printf '%s' '{"_README":"t","overrides":[{"name":"pkg-x","license":"   ","verified_source":"https://example.com"}]}' > "$tmp_overrides_blank_license"
  run_case "FAIL: 오버라이드 항목의 license가 공백 -> exit 1" \
    FAIL "$ALL_KNOWN_FIXTURE" "--overrides $tmp_overrides_blank_license"

  printf '%s' '{not valid json,,,' > "$tmp_overrides_malformed"
  run_case "FAIL: 오버라이드 파일 JSON이 깨짐 -> exit 1" \
    FAIL "$ALL_KNOWN_FIXTURE" "--overrides $tmp_overrides_malformed"

  total=$((total + 1))
  malformed_exit=0
  malformed_stderr="$(KUBEMETAL_RUNTIME_LICENSE_FIXTURE="$tmp_fixture" "$0" --out-dir "$tmp_out_c" --overrides "$tmp_overrides_malformed" 2>&1 1>/dev/null)" || malformed_exit=$?
  if [ "$malformed_exit" -eq 1 ] && ! printf '%s' "$malformed_stderr" | grep -q "Traceback"; then
    echo "  ok   깨진 오버라이드 JSON -> 스택 트레이스 없이 exit 1, 명확한 한국어 메시지"
  else
    echo "  BAD  깨진 오버라이드 JSON 처리 (exit=$malformed_exit, stderr에 Traceback 포함 여부 확인 필요)"
    fails=$((fails + 1))
  fi

  # --no-overrides / 기본 오버라이드 파일(scripts/release/runtime-license-overrides.json)
  # 통합 검증: tiktoken은 실제 오버라이드 파일에 등재된 실제 시드 항목이다.
  TIKTOKEN_FIXTURE='{"packages":[
    {"name":"tiktoken","version":"0.13.0","license_expression":null,
     "classifiers":[],"license_field":null}
  ]}'
  run_case "PASS: 기본 오버라이드 파일이 tiktoken(UNKNOWN) 해소 -> exit 0" \
    PASS "$TIKTOKEN_FIXTURE"
  run_case "FAIL: --no-overrides 지정 시 tiktoken이 다시 UNKNOWN -> exit 1" \
    FAIL "$TIKTOKEN_FIXTURE" "--no-overrides"

  # 오버라이드 파일이 없어도(오타 경로 등) 오류가 아니라 오버라이드 없음으로
  # 처리되고, 매칭되는 UNKNOWN 패키지는 그대로 UNKNOWN으로 남아 게이트가 문다.
  run_case "FAIL: 존재하지 않는 오버라이드 파일 -> 오류 아님, UNKNOWN 유지 -> exit 1" \
    FAIL '{"packages":[
      {"name":"pkg-none-ov","version":"1.0","license_expression":null,
       "classifiers":[],"license_field":null}
    ]}' "--overrides ${tmp_out_a}/does-not-exist-overrides.json"

  echo "  --- mlx.rs 어긋남 가드 ---"

  total=$((total + 1))
  if check_mlx_divergence_file; then
    echo "  ok   mlx.rs venv_dir() 어긋남 가드 (실제 파일, 일치)"
  else
    echo "  BAD  mlx.rs venv_dir() 어긋남 가드 (실제 파일에서 어긋남 감지됨)"
    fails=$((fails + 1))
  fi

  total=$((total + 1))
  if check_mlx_divergence_content 'pub(crate) fn venv_dir() -> Result<PathBuf, String> { Ok(home_dir()?.join("other").join("place")) }'; then
    echo "  BAD  mlx.rs 어긋남 가드 음성 대조군 (drift 있는 합성 내용을 통과시킴)"
    fails=$((fails + 1))
  else
    echo "  ok   mlx.rs 어긋남 가드 음성 대조군 (drift 있는 합성 내용을 실패로 감지)"
  fi

  [ "$fails" -eq 0 ] || { echo "self-test 실패 ${fails}건"; exit 1; }
  echo "self-test 통과 (${total}건)"
  exit 0
fi

# ------------------------------------------------------------------------------
# 일반 실행
# ------------------------------------------------------------------------------
VENV="${HOME}/.kubemetal/venv"
OUT_DIR="${PROJECT_ROOT}"
OVERRIDES="${DEFAULT_OVERRIDES}"

while [ $# -gt 0 ]; do
  case "$1" in
    --venv)
      [ $# -ge 2 ] || { echo "오류: --venv 뒤에 경로가 필요합니다." >&2; usage; }
      VENV="$2"
      shift 2
      ;;
    --out-dir)
      [ $# -ge 2 ] || { echo "오류: --out-dir 뒤에 디렉토리 경로가 필요합니다." >&2; usage; }
      OUT_DIR="$2"
      shift 2
      ;;
    --overrides)
      [ $# -ge 2 ] || { echo "오류: --overrides 뒤에 경로가 필요합니다." >&2; usage; }
      OVERRIDES="$2"
      shift 2
      ;;
    --no-overrides)
      OVERRIDES=""
      shift
      ;;
    -h|--help)
      usage
      ;;
    *)
      echo "알 수 없는 옵션: $1" >&2
      usage
      ;;
  esac
done

# 어긋남 가드: mlx.rs의 venv_dir() 정의가 여전히 이 스크립트의 기본 venv 경로와
# 맞는지 매 실행마다 검사한다(AGENTS.md: "어긋날 때 실패하는 테스트를 둔다").
check_mlx_divergence_file || exit 1

FIXTURE="${KUBEMETAL_RUNTIME_LICENSE_FIXTURE:-}"

if [ -n "$FIXTURE" ]; then
  PY_BIN="python3"
else
  PY_BIN="${VENV}/bin/python"
  if [ ! -x "$PY_BIN" ]; then
    echo "오류: MLX venv가 아직 생성되지 않았습니다 (${VENV}). 이 venv는 앱이 모델 학습/서빙을 처음 실행할 때 온디맨드로 만듭니다 — 먼저 앱에서 venv를 생성한 뒤 다시 실행하세요." >&2
    exit 2
  fi
fi

OUT_FILE="${OUT_DIR}/runtime-license-inventory.json"

gen_py="$(mktemp -t kubemetal-runtime-license-XXXXXX.py)"
trap 'rm -f "$gen_py"' EXIT
write_gen_py "$gen_py"

"$PY_BIN" "$gen_py" "$OUT_FILE" "$VENV" "$OVERRIDES"
