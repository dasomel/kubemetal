#!/usr/bin/env bash
# ==============================================================================
# gen_model_license_inventory.sh — 모델 가중치 라이선스 조회 (이슈 #9)
#
# 게이트가 아니라 사용자 도구다. 오너 스코프 결정(커밋 3d8f80a,
# docs/build-target-map.md)에 따라 모델 가중치는 이 저장소의 릴리스 증거 범위
# 밖이다: KubeMetal은 모델을 하나도 번들하지 않고, Model Hub는
# modelhub.rs가 HuggingFace API를 실시간 검색해 사용자가 고른 repo_id를
# 내려받는다 — 저장소 안에 인벤토리를 뜰 모델 카탈로그 자체가 없다.
# 런타임(Python 패키지) 쪽 증거는 gen_runtime_license_inventory.sh가 담당한다.
#
# 그래서 이 스크립트가 존재하는 이유는 하나다: NOTICE가 "내려받은 모델의
# 라이선스를 확인하고 준수하는 것은 사용자 책임"이라고 적어둔 일을 사용자가
# 실제로 할 수 있게 해주는 것. 검사할 모델은 CLI 인자나 --models-file로
# 받는다 — 목록을 하드코딩하면 이 저장소가 지원한다고 주장한 적 없는 모델을
# 지어내는 셈이 된다(D22).
#
# 각 모델의 HuggingFace Hub API 메타데이터에서 license 정보를 추출하여
# <out-dir>/model-license-inventory.json 에 기계 판독용 배열로 기록한다.
# 네트워크 실패나 라이선스 메타데이터 부재 시 UNKNOWN으로 기록하며,
# 모르는 것은 통과가 아니므로(check_licenses.sh 동일 원칙) 비영으로 실패한다.
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

usage() {
  echo "사용법: $0 [options] <model_id> [model_id ...]" >&2
  echo "       $0 [options] --models-file <path>" >&2
  echo "       $0 --self-test" >&2
  echo "" >&2
  echo "옵션:" >&2
  echo "  --models-file <path>   줄바꿈으로 구분된 모델 ID 목록 파일" >&2
  echo "  --out-dir <dir>        출력 디렉토리 (기본값: 프로젝트 루트)" >&2
  echo "  --self-test            모의 fixture 기반 자체 테스트 실행" >&2
  echo "  -h, --help             사용법 출력" >&2
  exit 2
}

# --self-test: 실제 네트워크 호출 없이 mock fixture로 정책 및 파싱을 검증한다.
if [ "${1:-}" = "--self-test" ]; then
  tmp_fixture="$(mktemp)"
  tmp_out_dir="$(mktemp -d)"
  tmp_models_file="$(mktemp)"
  trap 'rm -f "$tmp_fixture" "$tmp_models_file"; rm -rf "$tmp_out_dir"' EXIT
  fails=0

  # 합성 메타데이터 주입: 음성/양성 대조군 검증
  cat <<'EOF' > "$tmp_fixture"
{
  "test-org/model-carddata": {
    "cardData": {"license": "apache-2.0"}
  },
  "test-org/model-tag": {
    "tags": ["mlx", "license:mit"]
  },
  "test-org/model-toplevel": {
    "license": "bsd-3-clause"
  },
  "test-org/model-missing-license": {
    "cardData": {},
    "tags": ["mlx"]
  },
  "test-org/model-empty-license": {
    "cardData": {"license": "  "}
  },
  "test-org/model-not-found": null
}
EOF

  run_case() {
    local desc="$1" expected="$2"
    shift 2
    local got
    if KUBEMETAL_MODEL_FIXTURE="$tmp_fixture" "$0" --out-dir "$tmp_out_dir" "$@" >/dev/null 2>&1; then
      got="PASS"
    else
      got="FAIL"
    fi

    if [ "$got" = "$expected" ]; then
      echo "  ok   $desc"
    else
      echo "  BAD  $desc (기대=$expected, 실제=$got)"
      fails=$((fails + 1))
    fi
  }

  echo "gen_model_license_inventory self-test 시작..."
  # 알려진 라이선스 추출 -> PASS 케이스 (최소 2개)
  run_case "PASS: cardData.license 추출 (apache-2.0)" PASS "test-org/model-carddata"
  run_case "PASS: tags 라이선스 추출 (mit)" PASS "test-org/model-tag"
  run_case "PASS: top-level license 추출 (bsd-3-clause)" PASS "test-org/model-toplevel"
  run_case "PASS: 복수 모델 모두 라이선스 확인" PASS "test-org/model-carddata" "test-org/model-tag"
  printf "test-org/model-carddata\n# 주석 라인\n\ntest-org/model-toplevel\n" > "$tmp_models_file"
  run_case "PASS: --models-file 파일 입력 지원" PASS --models-file "$tmp_models_file"

  # UNKNOWN -> FAIL 케이스 (최소 2개)
  run_case "FAIL: API 실패/모델 없음 -> UNKNOWN 처리" FAIL "test-org/model-not-found"
  run_case "FAIL: 메타데이터에 license 없음 -> UNKNOWN 처리" FAIL "test-org/model-missing-license"
  run_case "FAIL: license 필드가 공백 -> UNKNOWN 처리" FAIL "test-org/model-empty-license"
  run_case "FAIL: 정상 모델과 UNKNOWN 혼합 -> FAIL" FAIL "test-org/model-carddata" "test-org/model-missing-license"

  # 산출물 JSON 스키마 및 저장 확인
  rm -f "${tmp_out_dir}/model-license-inventory.json"
  if KUBEMETAL_MODEL_FIXTURE="$tmp_fixture" "$0" --out-dir "$tmp_out_dir" "test-org/model-carddata" >/dev/null 2>&1; then
    if python3 -c '
import json, sys
data = json.load(open(sys.argv[1]))
assert len(data) == 1
assert data[0]["model_id"] == "test-org/model-carddata"
assert data[0]["license"] == "apache-2.0"
assert data[0]["source_url"] == "https://huggingface.co/test-org/model-carddata"
' "${tmp_out_dir}/model-license-inventory.json" 2>/dev/null; then
      echo "  ok   산출물 JSON 스키마 및 내용 일치"
    else
      echo "  BAD  산출물 JSON 스키마 불일치"
      fails=$((fails + 1))
    fi
  else
    echo "  BAD  산출물 파일 생성 실패"
    fails=$((fails + 1))
  fi

  if [ "$fails" -gt 0 ]; then
    echo "self-test 실패 ${fails}건"
    exit 1
  fi
  echo "self-test 통과 (10건)"
  exit 0
fi

OUT_DIR="${PROJECT_ROOT}"
MODELS=()
MODELS_FILE=""

while [ $# -gt 0 ]; do
  case "$1" in
    --models-file)
      [ $# -ge 2 ] || { echo "오류: --models-file 뒤에 파일 경로가 필요합니다." >&2; usage; }
      MODELS_FILE="$2"
      shift 2
      ;;
    --out-dir)
      [ $# -ge 2 ] || { echo "오류: --out-dir 뒤에 디렉토리 경로가 필요합니다." >&2; usage; }
      OUT_DIR="$2"
      shift 2
      ;;
    -h|--help)
      usage
      ;;
    -*)
      echo "알 수 없는 옵션: $1" >&2
      usage
      ;;
    *)
      MODELS+=("$1")
      shift
      ;;
  esac
done

if [ -n "$MODELS_FILE" ]; then
  [ -f "$MODELS_FILE" ] || { echo "오류: 파일을 찾을 수 없습니다: $MODELS_FILE" >&2; exit 1; }
  while IFS= read -r line || [ -n "$line" ]; do
    trimmed="$(echo "$line" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
    if [ -n "$trimmed" ] && [[ ! "$trimmed" =~ ^# ]]; then
      MODELS+=("$trimmed")
    fi
  done < "$MODELS_FILE"
fi

if [ ${#MODELS[@]} -eq 0 ]; then
  echo "오류: 검사할 모델 ID가 지정되지 않았습니다. 모델 ID 인자를 전달하거나 --models-file을 지정하세요." >&2
  usage
fi

OUT_FILE="${OUT_DIR}/model-license-inventory.json"

python3 -c '
import json
import os
import subprocess
import sys

out_file = sys.argv[1]
raw_models = sys.argv[2:]
fixture_path = os.environ.get("KUBEMETAL_MODEL_FIXTURE", "")

# 중복 제거 (순서 보존)
seen = set()
models = []
for m in raw_models:
    if m not in seen:
        seen.add(m)
        models.append(m)

fixture_data = None
if fixture_path:
    if not os.path.exists(fixture_path):
        print(f"오류: KUBEMETAL_MODEL_FIXTURE 파일을 찾을 수 없습니다: {fixture_path}", file=sys.stderr)
        sys.exit(2)
    with open(fixture_path, "r", encoding="utf-8") as f:
        fixture_data = json.load(f)

def fetch_model_data(model_id):
    if fixture_data is not None:
        return fixture_data.get(model_id)

    # curl을 호출하는 이유: macOS Python은 시스템 루트 CA를 직접 보지 못해
    # CERTIFICATE_VERIFY_FAILED가 발생할 수 있으나, 시스템 curl은 키체인을 통해
    # 신뢰성 있게 SSL 핸드셰이크를 처리한다.
    url = f"https://huggingface.co/api/models/{model_id}"
    try:
        res = subprocess.run(
            ["curl", "-sSf", "--connect-timeout", "10", "--max-time", "30",
             "-H", "User-Agent: kubemetal-model-license-inventory", url],
            capture_output=True,
            text=True
        )
        if res.returncode != 0:
            return None
        return json.loads(res.stdout)
    except Exception:
        return None

def extract_license(data):
    if not data or not isinstance(data, dict):
        return "UNKNOWN"

    lic = None
    # 1. cardData.license
    card_data = data.get("cardData")
    if isinstance(card_data, dict):
        l = card_data.get("license")
        if isinstance(l, str) and l.strip():
            lic = l.strip()
        elif isinstance(l, list) and l:
            lic = ", ".join(str(x).strip() for x in l if str(x).strip())

    # 2. top-level license
    if not lic:
        l = data.get("license")
        if isinstance(l, str) and l.strip():
            lic = l.strip()
        elif isinstance(l, list) and l:
            lic = ", ".join(str(x).strip() for x in l if str(x).strip())

    # 3. tags (license:xxx)
    if not lic:
        tags = data.get("tags")
        if isinstance(tags, list):
            for t in tags:
                if isinstance(t, str) and t.startswith("license:"):
                    extracted = t.split("license:", 1)[1].strip()
                    if extracted:
                        lic = extracted
                        break

    if not lic or lic.strip().upper() == "UNKNOWN":
        return "UNKNOWN"
    return lic.strip()

inventory = []
unknowns = []

for model_id in models:
    data = fetch_model_data(model_id)
    lic = extract_license(data)
    source_url = f"https://huggingface.co/{model_id}"

    inventory.append({
        "model_id": model_id,
        "license": lic,
        "source_url": source_url
    })

    if lic == "UNKNOWN":
        unknowns.append(model_id)

out_dir = os.path.dirname(os.path.abspath(out_file))
os.makedirs(out_dir, exist_ok=True)
with open(out_file, "w", encoding="utf-8") as f:
    json.dump(inventory, f, indent=2, ensure_ascii=False)
    f.write("\n")

print(f"생성: {out_file} (모델 {len(inventory)}개)")
for item in inventory:
    status = "OK  " if item["license"] != "UNKNOWN" else "FAIL"
    print("  [{}] {} — {}".format(status, item["model_id"], item["license"]))

if unknowns:
    print(f"\nFAIL: 라이선스가 UNKNOWN인 모델 {len(unknowns)}건 — 모르는 것은 통과가 아닙니다:")
    for m in unknowns:
        print(f"  - {m}")
    sys.exit(1)

print("\nOK: 모든 모델 라이선스가 확인되었습니다.")
sys.exit(0)
' "$OUT_FILE" "${MODELS[@]}"
