#!/usr/bin/env bash
# D52 regression: simulated-gpu.sh against a PATH-shimmed kubectl; no cluster, no network.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SUT="${SIMGPU_SCRIPT:-${SCRIPT_DIR}/simulated-gpu.sh}"
TEST_DIR="$(mktemp -d -t kubemetal-simgpu)"
trap 'rm -rf "$TEST_DIR"' EXIT
mkdir -p "$TEST_DIR/bin"
export STATE="$TEST_DIR/state" CALLS="$TEST_DIR/calls"
export SIMGPU_POLL_TRIES=2 SIMGPU_POLL_SLEEP=0
export PATH="$TEST_DIR/bin:$PATH"

# Stub: one node "n1"; patch add/remove edits $STATE/{capacity,allocatable};
# STUB_MISMATCH=1 makes the apiserver "report" a different value than was patched.
cat > "$TEST_DIR/bin/kubectl" <<'STUB'
#!/usr/bin/env bash
echo "$*" >> "$CALLS"
args="$*"
case "$args" in
  *"get nodes"*) echo n1 ;;
  *"patch node"*)
    if [[ "$args" == *'"op":"add"'* ]]; then
      v="$(sed -E 's/.*"value":"([0-9]+)".*/\1/' <<<"$args")"
      [[ -n "${STUB_MISMATCH:-}" ]] && v=$((v + 1))
      echo "$v" > "$STATE.capacity"; echo "$v" > "$STATE.allocatable"
    else
      [[ "$args" == *capacity* ]] && rm -f "$STATE.capacity"
      [[ "$args" == *allocatable* ]] && rm -f "$STATE.allocatable"
    fi ;;
  *"get node n1"*".status.images"*) echo "rancher/mirrored-pause:3.6" ;;
  *"get node n1"*"status.capacity"*) cat "$STATE.capacity" 2>/dev/null || true ;;
  *"get node n1"*"status.allocatable"*) cat "$STATE.allocatable" 2>/dev/null || true ;;
  *"get node n1"*labels*) cat "$STATE.label" 2>/dev/null || true ;;
  *"label node"*) [[ "$args" == *"simulated-gpu-"* ]] && rm -f "$STATE.label" || echo true > "$STATE.label" ;;
  *"get pod simgpu-fit-"*"status.phase"*)
    pod_name="$(grep -oE 'simgpu-fit-[0-9]+-[0-9]+' <<<"$args" || true)"
    target_pid="${pod_name##*-}"
    if [[ -n "${STUB_SEND_INT:-}" && -n "$target_pid" && ! -f "$STATE.int_sent" ]]; then
      echo 1 > "$STATE.int_sent"
      kill -INT "$target_pid"
      sleep 0.2
    elif [[ -n "${STUB_SEND_TERM:-}" && -n "$target_pid" && ! -f "$STATE.term_sent" ]]; then
      echo 1 > "$STATE.term_sent"
      kill -TERM "$target_pid"
      sleep 0.2
    fi
    echo Running ;;
  *"get pod simgpu-over-"*"status.phase"*) echo Pending ;;
  *"get events"*"FailedScheduling"*) echo "0/1 nodes are available: 1 Insufficient nvidia.com/gpu." ;;
  *"apply -f"*)
    mf_file="$(sed -E 's/.*-f ([^ ]+).*/\1/' <<<"$args")"
    if [[ -f "$mf_file" ]] && grep -q "kubemetal.io/simgpu-verify" "$mf_file"; then
      echo "labeled" >> "$STATE.manifest_labeled"
    fi ;;
  *"delete pod"*)
    if [[ -n "${STUB_DELETE_FAIL:-}" && "$args" == *"simgpu-fit-"* ]]; then
      exit 1
    fi
    if [[ -n "${STUB_SWEEP_FAIL:-}" && "$args" == *"-l kubemetal.io/simgpu-verify=true"* ]]; then
      exit 1
    fi
    if [[ -f "$STATE.leftover" && "$args" == *"-l kubemetal.io/simgpu-verify=true"* ]]; then
      cat "$STATE.leftover"
      rm -f "$STATE.leftover"
    fi ;;
esac
exit 0
STUB
chmod +x "$TEST_DIR/bin/kubectl"

fail() { echo "FAIL: $*" >&2; exit 1; }
run() { : > "$CALLS"; OUT="$("$SUT" "$@" 2>&1)" && RC=0 || RC=$?; }
reset() { rm -f "$STATE".*; : > "$CALLS"; unset STUB_MISMATCH STUB_DELETE_FAIL STUB_SWEEP_FAIL STUB_SEND_INT STUB_SEND_TERM || true; }
expect_fail() { [[ $RC -ne 0 ]] || fail "$1: expected non-zero, got 0 ($OUT)"; }
expect_out() { grep -q -- "$2" <<<"$OUT" || fail "$1: output lacks '$2': $OUT"; }

# 1. wrong context refused, kubectl never called
reset
KUBE_CONTEXT=colima-beluga run advertise 1
expect_fail "wrong context"; expect_out "wrong context" "refusing context"
[[ ! -s "$CALLS" ]] || fail "kubectl called for refused context"
KUBE_CONTEXT=narwhal run status; expect_fail "external ctx"
echo "PASS: foreign contexts refused without touching kubectl"

# 2. non-integer rejected; clamp
for bad in abc 1.5 -2 ""; do reset; run advertise "$bad"; expect_fail "N='$bad'"; done
reset; run advertise 99; [[ $RC -eq 0 ]] || fail "clamp high: $OUT"; [[ "$(cat "$STATE.capacity")" == 8 ]] || fail "99 not clamped to 8"
reset; run advertise 0; [[ $RC -eq 0 ]] || fail "clamp low: $OUT"; [[ "$(cat "$STATE.capacity")" == 1 ]] || fail "0 not clamped to 1"
reset; run advertise; [[ "$(cat "$STATE.capacity")" == 1 ]] || fail "default not 1"
echo "PASS: N validated and clamped to 1..8 (default 1)"

# 3. read-back mismatch -> failure, no success text
reset; STUB_MISMATCH=1 run advertise 2
expect_fail "mismatch"
! grep -q '^OK' <<<"$OUT" || fail "success printed despite mismatch: $OUT"
echo "PASS: advertise fails without printing success when read-back mismatches"

# 4. happy path then retract twice (idempotent), status
reset; run advertise 2; [[ $RC -eq 0 ]] && expect_out "advertise" "^OK"
run status; expect_out "status" "capacity=2 allocatable=2"
run retract; [[ $RC -eq 0 ]] || fail "retract 1: $OUT"
run retract; [[ $RC -eq 0 ]] || fail "retract 2 (idempotent): $OUT"
run status; expect_out "status after retract" "not advertised"
echo "PASS: advertise/status/retract, retract idempotent"

# 5. verify exercises EXIT trap cleanup (D52 globals vs local-var regression) and delete failure
reset; run advertise 1; [[ $RC -eq 0 ]] || fail "advertise before verify: $OUT"
run verify
[[ $RC -eq 0 ]] || fail "verify exited $RC: $OUT"
! grep -qi "unbound variable" <<<"$OUT" || fail "unbound variable in verify output: $OUT"
grep -qE 'delete pod simgpu-fit-[0-9]+-[0-9]+ simgpu-over-[0-9]+-[0-9]+' "$CALLS" ||
  fail "expected delete pod <fit-pod> <over-pod> in stub calls: $(cat "$CALLS")"
STUB_DELETE_FAIL=1 run verify
expect_fail "verify delete failure"
expect_out "verify delete failure" "FAIL: cleanup"
echo "PASS: verify cleans up both pods via EXIT trap and surfaces delete failures"

# 6. verify sweeps leftover verify pods before creating new ones; first delete uses label selector
reset; run advertise 1; [[ $RC -eq 0 ]] || fail "advertise before verify sweep test: $OUT"
echo 'pod "simgpu-fit-leftover-1" deleted' > "$STATE.leftover"
run verify
[[ $RC -eq 0 ]] || fail "verify with leftover pod exited $RC: $OUT"
expect_out "verify swept leftover pod" "swept 1 leftover verify pod"
first_del="$(grep -m1 "delete pod" "$CALLS")"
grep -q -- "-l kubemetal.io/simgpu-verify=true" <<<"$first_del" ||
  fail "verify first delete did not use label selector: $first_del"
grep -q -- "--ignore-not-found" <<<"$first_del" || fail "verify first delete missing --ignore-not-found: $first_del"
grep -q -- "--wait=true" <<<"$first_del" || fail "verify first delete missing --wait=true: $first_del"
grep -q -- "--timeout=60s" <<<"$first_del" || fail "verify first delete missing --timeout=60s: $first_del"
[[ -s "$STATE.manifest_labeled" ]] || fail "pod manifest missing kubemetal.io/simgpu-verify label"
echo "PASS: verify sweeps leftover pods first using label selector and succeeds"

# 7. retract sweeps leftover verify pods first
reset; run advertise 1; [[ $RC -eq 0 ]] || fail "advertise before retract sweep test: $OUT"
echo 'pod "simgpu-over-leftover-2" deleted' > "$STATE.leftover"
run retract
[[ $RC -eq 0 ]] || fail "retract with leftover pod exited $RC: $OUT"
expect_out "retract swept leftover pod" "swept 1 leftover verify pod"
expect_out "retract success" "OK: n1 no longer advertises nvidia.com/gpu"
first_del="$(grep -m1 "delete pod" "$CALLS")"
grep -q -- "-l kubemetal.io/simgpu-verify=true" <<<"$first_del" ||
  fail "retract first delete did not use label selector: $first_del"
echo "PASS: retract sweeps leftover verify pods first"

# 8. failure to sweep leftover verify pods terminates verify
reset; run advertise 1; [[ $RC -eq 0 ]] || fail "advertise before sweep failure test: $OUT"
STUB_SWEEP_FAIL=1 run verify
expect_fail "sweep failure must die"
expect_out "sweep failure message" "failed to sweep leftover verify pods"
echo "PASS: failure to sweep leftover verify pods dies immediately"

# 9. SIGINT triggers cleanup exactly once and exits 130
reset; run advertise 1; [[ $RC -eq 0 ]] || fail "advertise before INT test: $OUT"
STUB_SEND_INT=1 run verify
[[ $RC -eq 130 ]] || fail "expected 130 on SIGINT, got $RC ($OUT)"
clean_calls="$(grep -cE 'delete pod simgpu-fit-[0-9]+-[0-9]+' "$CALLS" || true)"
[[ "$clean_calls" -eq 1 ]] || fail "expected cleanup to run once, ran $clean_calls times"
echo "PASS: SIGINT cleans up pods exactly once and exits 130"

# 10. SIGTERM triggers cleanup exactly once and exits 143
reset; run advertise 1; [[ $RC -eq 0 ]] || fail "advertise before TERM test: $OUT"
STUB_SEND_TERM=1 run verify
[[ $RC -eq 143 ]] || fail "expected 143 on SIGTERM, got $RC ($OUT)"
clean_calls="$(grep -cE 'delete pod simgpu-fit-[0-9]+-[0-9]+' "$CALLS" || true)"
[[ "$clean_calls" -eq 1 ]] || fail "expected cleanup to run once, ran $clean_calls times"
echo "PASS: SIGTERM cleans up pods exactly once and exits 143"

# 11. delete-by-label printing "No resources found" reports swept 0 (never swept 1)
reset; run advertise 1; [[ $RC -eq 0 ]] || fail "advertise before no-resources sweep test: $OUT"
echo "No resources found in default namespace." > "$STATE.leftover"
run verify
[[ $RC -eq 0 ]] || fail "verify with No resources found exited $RC: $OUT"
expect_out "verify no-resources swept 0" "swept 0"
! grep -q "swept 1" <<<"$OUT" || fail "No resources found miscounted as swept 1: $OUT"
echo "No resources found" > "$STATE.leftover"
run retract
[[ $RC -eq 0 ]] || fail "retract with No resources found exited $RC: $OUT"
expect_out "retract no-resources swept 0" "swept 0"
! grep -q "swept 1" <<<"$OUT" || fail "No resources found miscounted as swept 1 on retract: $OUT"
echo "PASS: 'No resources found' from delete-by-label reports swept 0"
