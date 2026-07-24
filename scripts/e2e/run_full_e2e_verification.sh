#!/usr/bin/env bash
# ==============================================================================
# run_full_e2e_verification.sh
# KubeMetal E2E 검증 스위트 (docs/17-comprehensive-test-environment.md)
#
# 각 단계는 실패 시 0이 아닌 코드로 종료한다. 이 러너는 실패를 그대로 전파하며,
# 모든 단계가 통과했을 때만 성공을 보고한다 — 무조건 성공 문구를 찍지 않는다.
#
# 전제: colima 기동 + `make provision` + `make forward` + MLX 서빙 기동(Step 1).
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

STEPS=(
  "01_synthetic_data_gen.py"
  "02_mlx_finetune_eval.py"
  "03_kagent_fault_injection.py"
  "04_coding_agent_patch.py"
)

echo "======================================================================"
echo "  KubeMetal E2E Verification Suite"
echo "  시작: $(date)"
echo "======================================================================"

failed=()
for step in "${STEPS[@]}"; do
  echo ""
  echo "---- ${step} ----"
  if python3 "${SCRIPT_DIR}/${step}"; then
    echo "---- ${step}: PASS ----"
  else
    echo "---- ${step}: FAIL ----" >&2
    failed+=("${step}")
    # 뒷 단계는 앞 단계 산출물에 의존하므로 첫 실패에서 멈춘다.
    break
  fi
done

echo ""
echo "======================================================================"
if [ ${#failed[@]} -eq 0 ]; then
  echo "  RESULT: PASS — ${#STEPS[@]}개 단계 전부 통과"
  echo "  종료: $(date)"
  echo "======================================================================"
  exit 0
fi

echo "  RESULT: FAIL — 실패 단계: ${failed[*]}"
echo "  종료: $(date)"
echo "======================================================================"
exit 1
