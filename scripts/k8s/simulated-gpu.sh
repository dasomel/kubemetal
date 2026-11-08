#!/usr/bin/env bash
# D52: scheduling-only simulated NVIDIA GPU on the app's own k3s node.
# Advertises `nvidia.com/gpu` as a node extended resource so NVIDIA-targeted manifests can be
# scheduled locally. NOTHING is passed through: pods get no device, K8s still never runs
# compute (hard invariant) -- real GPU work stays host CuMetal/MLX.
# Scope: only the managed colima context (D43). External clusters (D26/D30) are refused.
# Usage: simulated-gpu.sh {advertise [N]|retract|status|verify}   env: KUBE_CONTEXT
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/colima-profile.sh
. "${SCRIPT_DIR}/../colima-profile.sh"

RES="nvidia.com/gpu"
RES_PTR="nvidia.com~1gpu" # RFC 6901 escape of '/'
LABEL="kubemetal.io/simulated-gpu"
VERIFY_LABEL="kubemetal.io/simgpu-verify=true"
NOTE_KEY="kubemetal.io/simulated-gpu-note"
NOTE="scheduling-only simulation (D52): nvidia.com/gpu is advertised for manifest testing; no device is passed through"
NS="default" # colima-only namespace (D26)
CTX="${KUBE_CONTEXT:-$COLIMA_CONTEXT}"
POLL_TRIES="${SIMGPU_POLL_TRIES:-30}"
POLL_SLEEP="${SIMGPU_POLL_SLEEP:-2}"

die() { echo "simulated-gpu: $*" >&2; exit 1; }
# The Lima ssh forward to the apiserver can drop briefly (measured 2026-11-08); retry only
# connection-level failures so real API errors (NotFound, Forbidden, Invalid) still surface at once.
k() {
  local i err rc out
  err="$(mktemp)"
  for ((i = 0; i < ${SIMGPU_API_TRIES:-15}; i++)); do
    rc=0; out="$(kubectl --context "$CTX" "$@" 2>"$err")" || rc=$?
    if ((rc == 0)); then cat "$err" >&2; rm -f "$err"; printf '%s' "$out"; [[ -z "$out" ]] || echo; return 0; fi
    grep -qiE 'connection refused|was refused|connection reset|unable to handle the request|i/o timeout|EOF|TLS handshake' "$err" || break
    sleep "${SIMGPU_POLL_SLEEP:-2}"
  done
  cat "$err" >&2; rm -f "$err"; return "$rc"
}

[[ "$CTX" == "$COLIMA_CONTEXT" ]] ||
  die "refusing context '$CTX': only the managed context '$COLIMA_CONTEXT' is allowed (external clusters are out of scope, D26/D30)"

node_name() {
  local names
  names="$(k get nodes -o jsonpath='{.items[*].metadata.name}')" || die "cannot list nodes on $CTX"
  [[ -n "$names" && "$names" != *" "* ]] || die "expected exactly one node on $CTX, got: '${names}'"
  echo "$names"
}

# $1 node, $2 capacity|allocatable -> value or empty
field() { k get node "$1" -o jsonpath="{.status.$2.nvidia\.com/gpu}"; }

wait_for() { # $1 description, rest: command that succeeds when satisfied
  local what="$1" i; shift
  for ((i = 0; i < POLL_TRIES; i++)); do
    "$@" && return 0
    sleep "$POLL_SLEEP"
  done
  die "$what: not observed after $((POLL_TRIES * POLL_SLEEP))s"
}

cmd_advertise() {
  local n="${1-1}" node
  [[ "$n" =~ ^[0-9]+$ ]] || die "N must be a non-negative integer, got '$n'"
  n=$((10#$n))
  ((n < 1)) && n=1
  ((n > 8)) && n=8
  node="$(node_name)"
  k patch node "$node" --subresource=status --type=json \
    -p "[{\"op\":\"add\",\"path\":\"/status/capacity/${RES_PTR}\",\"value\":\"${n}\"}]" >/dev/null
  k label node "$node" "${LABEL}=true" --overwrite >/dev/null
  k annotate node "$node" "${NOTE_KEY}=${NOTE}" --overwrite >/dev/null
  # D22: success is printed only after the node itself reports it.
  advertised() { [[ "$(field "$node" capacity)" == "$n" && "$(field "$node" allocatable)" == "$n" ]]; }
  wait_for "$RES capacity+allocatable == $n on $node" advertised
  echo "OK: $node advertises $RES=$n (capacity=$(field "$node" capacity) allocatable=$(field "$node" allocatable)); scheduling-only, no device"
}

sweep_leftovers() {
  local out n=0
  out="$(k -n "$NS" delete pod -l "$VERIFY_LABEL" --ignore-not-found --wait=true --timeout=60s)" ||
    die "failed to sweep leftover verify pods ($VERIFY_LABEL)"
  # Count only actual deletions: with nothing to delete kubectl prints "No resources found",
  # which a non-empty-line count reported as "swept 1" (D22: never report work not done).
  n="$(grep -cE '^pod "[^"]+" deleted' <<<"$out" || true)"
  if ((n == 1)); then
    echo "swept 1 leftover verify pod"
  else
    echo "swept $n leftover verify pods"
  fi
}

cmd_retract() {
  sweep_leftovers
  local node ops="" sec
  node="$(node_name)"
  for sec in capacity allocatable; do
    [[ -n "$(field "$node" "$sec")" ]] && ops+="${ops:+,}{\"op\":\"remove\",\"path\":\"/status/${sec}/${RES_PTR}\"}"
  done
  [[ -z "$ops" ]] || k patch node "$node" --subresource=status --type=json -p "[${ops}]" >/dev/null
  k label node "$node" "${LABEL}-" >/dev/null
  k annotate node "$node" "${NOTE_KEY}-" >/dev/null
  gone() { # kubelet may re-sync allocatable; remove again if it lingers
    local a; a="$(field "$node" allocatable)"
    [[ -z "$(field "$node" capacity)" && -z "$a" ]] && return 0
    [[ -z "$a" ]] || k patch node "$node" --subresource=status --type=json \
      -p "[{\"op\":\"remove\",\"path\":\"/status/allocatable/${RES_PTR}\"}]" >/dev/null 2>&1 || true
    return 1
  }
  wait_for "$RES gone from $node" gone
  [[ -z "$(k get node "$node" -o jsonpath="{.metadata.labels.kubemetal\.io/simulated-gpu}")" ]] || die "label still present on $node"
  echo "OK: $node no longer advertises $RES"
}

cmd_status() {
  local node cap alloc lab
  node="$(node_name)"
  cap="$(field "$node" capacity)"; alloc="$(field "$node" allocatable)"
  lab="$(k get node "$node" -o jsonpath="{.metadata.labels.kubemetal\.io/simulated-gpu}")"
  if [[ -z "$cap" && -z "$alloc" ]]; then
    echo "not advertised ($node, context $CTX)${lab:+ [stale label ${LABEL}=${lab}]}"
  else
    echo "$node: capacity=${cap:-<none>} allocatable=${alloc:-<none>} label ${LABEL}=${lab:-<none>}"
  fi
}

pod_manifest() { # $1 name, $2 gpu count, $3 image
  cat <<YAML
apiVersion: v1
kind: Pod
metadata:
  name: $1
  labels: {app.kubernetes.io/part-of: kubemetal-simulated-gpu-verify, kubemetal.io/simgpu-verify: "true"}
spec:
  restartPolicy: Never
  automountServiceAccountToken: false
  securityContext: {runAsNonRoot: true, runAsUser: 65532, runAsGroup: 65532, fsGroup: 65532, seccompProfile: {type: RuntimeDefault}}
  containers:
  - name: c
    image: $3
    imagePullPolicy: Never
    securityContext: {allowPrivilegeEscalation: false, readOnlyRootFilesystem: true, capabilities: {drop: [ALL]}}
    resources: {limits: {$RES: "$2"}}
YAML
}

# The EXIT/INT/TERM trap fires after cmd_verify returns (or on signal/abort) — so the names
# the trap needs (mf/p1/p2/cleaned) must be globals, or cleanup dies on `set -u` and leaves pods behind.
VERIFY_MF="" VERIFY_P1="" VERIFY_P2="" VERIFY_CLEANED=0

cleanup() {
  local sig="${1:-0}"
  trap - EXIT INT TERM
  ((VERIFY_CLEANED)) && return 0
  VERIFY_CLEANED=1
  [[ -z "$VERIFY_MF" ]] || rm -f "$VERIFY_MF"
  if [[ -n "$VERIFY_P1" ]]; then
    k -n "$NS" delete pod "$VERIFY_P1" ${VERIFY_P2:+"$VERIFY_P2"} --ignore-not-found --wait=true --timeout=60s >/dev/null || { echo "FAIL: cleanup of $VERIFY_P1/$VERIFY_P2 failed" >&2; exit 1; }
  fi
  case "$sig" in
    INT) exit 130 ;;
    TERM) exit 143 ;;
    *) ;;
  esac
}

cmd_verify() {
  sweep_leftovers
  local node cap img suffix phase
  node="$(node_name)"
  cap="$(field "$node" capacity)"; [[ -n "$cap" ]] || die "$RES not advertised; run 'advertise' first"
  # Image must already be in the VM (never pulled): the k3s pause image.
  img="$(k get node "$node" -o jsonpath='{range .status.images[*]}{.names[*]}{"\n"}{end}' | tr ' ' '\n' | grep -E 'pause:[^@]+$' | head -1 || true)"
  [[ -n "$img" ]] || die "no pause image found in $node .status.images; refusing to pull"
  suffix="$(date +%s)-$$"; VERIFY_P1="simgpu-fit-${suffix}"; VERIFY_P2="simgpu-over-${suffix}"
  VERIFY_MF="$(mktemp)"
  VERIFY_CLEANED=0
  trap 'cleanup INT' INT
  trap 'cleanup TERM' TERM
  trap cleanup EXIT
  echo "image=$img (already on node, imagePullPolicy=Never)"

  pod_manifest "$VERIFY_P1" 1 "$img" >"$VERIFY_MF"; k -n "$NS" apply -f "$VERIFY_MF" >/dev/null
  running() { [[ "$(k -n "$NS" get pod "$VERIFY_P1" -o jsonpath='{.status.phase}')" == Running ]]; }
  wait_for "pod $VERIFY_P1 Running" running
  echo "PASS: $VERIFY_P1 (limits $RES=1) Running"

  # No exec tooling in the pause image -> assert the spec grants no device path instead.
  [[ -z "$(k -n "$NS" get pod "$VERIFY_P1" -o jsonpath='{.spec.volumes[*].name}{.spec.containers[*].volumeMounts[*].name}{.spec.containers[*].volumeDevices[*].name}{.spec.containers[*].securityContext.privileged}')" ]] ||
    die "$VERIFY_P1 spec carries volumes/devices/privileged"
  echo "PASS: $VERIFY_P1 spec has no volumes, hostPath, device mounts or privileged (kubectl exec unavailable in pause image; /dev/nvidia* not inspected directly)"

  pod_manifest "$VERIFY_P2" $((cap + 1)) "$img" >"$VERIFY_MF"; k -n "$NS" apply -f "$VERIFY_MF" >/dev/null
  unsched() { k -n "$NS" get events --field-selector "involvedObject.name=${VERIFY_P2},reason=FailedScheduling" -o jsonpath='{.items[*].message}' | grep -q "Insufficient ${RES}"; }
  wait_for "FailedScheduling 'Insufficient ${RES}' for $VERIFY_P2" unsched
  phase="$(k -n "$NS" get pod "$VERIFY_P2" -o jsonpath='{.status.phase}')"
  [[ "$phase" == Pending ]] || die "$VERIFY_P2 expected Pending, got '$phase'"
  echo "PASS: $VERIFY_P2 (limits $RES=$((cap + 1))) Pending, FailedScheduling Insufficient ${RES}"
}

case "${1:-}" in
  advertise) shift; cmd_advertise "$@" ;;
  retract) cmd_retract ;;
  status) cmd_status ;;
  verify) cmd_verify ;;
  *) die "usage: $0 {advertise [N]|retract|status|verify}" ;;
esac
