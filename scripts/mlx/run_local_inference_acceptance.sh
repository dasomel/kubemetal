#!/usr/bin/env bash
set -euo pipefail

# KubeMetal #58 on-device acceptance runner.
# It does not start/own oMLX itself: start the intended runtime from KubeMetal first so PID
# ownership, loopback binding and UI state are validated through the production path.

RUNTIME="${RUNTIME:-omlx}"
ENDPOINT="${ENDPOINT:-http://127.0.0.1:8000}"
MODEL_A="${MODEL_A:-}"
MODEL_B="${MODEL_B:-}"
MODEL_DIR="${MODEL_DIR:-$HOME/.omlx/models}"
CACHE_DIR="${CACHE_DIR:-$HOME/.omlx/cache}"
CACHE_STATE="${CACHE_STATE:-warm}"
RUNTIME_PID="${RUNTIME_PID:-}"
API_KEY="${API_KEY:-}"
RUN_BRIDGE="${RUN_BRIDGE:-0}"
SKIP_STATIC="${SKIP_STATIC:-0}"
EVIDENCE_DIR="${EVIDENCE_DIR:-evidence/local-inference/$(date +%Y%m%d-%H%M%S)}"

if [[ -z "$MODEL_A" ]]; then
  echo "MODEL_A is required (oMLX model id used for benchmark)." >&2
  exit 2
fi
case "$ENDPOINT" in
  http://127.0.0.1:*|http://localhost:*) ;;
  *) echo "ENDPOINT must be loopback-only: $ENDPOINT" >&2; exit 2 ;;
esac

mkdir -p "$EVIDENCE_DIR"
exec > >(tee "$EVIDENCE_DIR/acceptance.log") 2>&1

printf 'KubeMetal #58 acceptance\n  runtime: %s\n  endpoint: %s\n  evidence: %s\n' "$RUNTIME" "$ENDPOINT" "$EVIDENCE_DIR"

if [[ "$SKIP_STATIC" != "1" ]]; then
  echo "== static verification =="
  make verify
fi

echo "== local-only readiness =="
RUNTIME="$RUNTIME" ENDPOINT="$ENDPOINT" MODEL_DIR="$MODEL_DIR" API_KEY="$API_KEY" \
  bash scripts/mlx/verify_local_inference_airgap.sh | tee "$EVIDENCE_DIR/airgap-readiness.txt"

echo "== benchmark matrix =="
for concurrency in 1 2 4 8; do
  args=(
    python3 scripts/mlx/benchmark_local_inference.py
    --runtime "$RUNTIME"
    --endpoint "$ENDPOINT"
    --model "$MODEL_A"
    --requests 8
    --concurrency "$concurrency"
    --cache-state "$CACHE_STATE"
    --cache-dir "$CACHE_DIR"
    --stream
    --output "$EVIDENCE_DIR/${RUNTIME}-${CACHE_STATE}-c${concurrency}.json"
  )
  [[ -n "$RUNTIME_PID" ]] && args+=(--runtime-pid "$RUNTIME_PID")
  [[ -n "$API_KEY" ]] && args+=(--api-key "$API_KEY")
  "${args[@]}"
done

if [[ -n "$MODEL_B" ]]; then
  echo "== multi-model load/switch/unload =="
  args=(
    python3 scripts/mlx/verify_omlx_multi_model.py
    --endpoint "$ENDPOINT"
    --model-a "$MODEL_A"
    --model-b "$MODEL_B"
    --cycles 2
    --chat-rounds 1
    --output "$EVIDENCE_DIR/multi-model.json"
  )
  [[ -n "$API_KEY" ]] && args+=(--api-key "$API_KEY")
  "${args[@]}"
else
  echo "MODEL_B not set: multi-model pressure harness skipped."
fi

if [[ "$RUN_BRIDGE" == "1" ]]; then
  echo "== private K3s/Colima bridge =="
  bash scripts/mlx/verify_local_inference_bridge.sh | tee "$EVIDENCE_DIR/bridge.txt"
else
  echo "RUN_BRIDGE=0: bridge test skipped. Set RUN_BRIDGE=1 after starting the KubeMetal private bridge."
fi

cat > "$EVIDENCE_DIR/manifest.json" <<EOF
{
  "schema_version": 1,
  "runtime": "${RUNTIME}",
  "endpoint": "${ENDPOINT}",
  "model_a": "${MODEL_A}",
  "model_b": "${MODEL_B}",
  "cache_state": "${CACHE_STATE}",
  "runtime_pid": "${RUNTIME_PID}",
  "bridge_requested": ${RUN_BRIDGE},
  "external_network_disconnected": false
}
EOF

cat <<EOF

Acceptance harness completed.
Evidence: $EVIDENCE_DIR

Still manual/physical by definition:
- repeat the local readiness/inference test with external networking actually disconnected;
- observe Activity Monitor/thermal behavior under the desired sustained workload;
- set external_network_disconnected=true only after that real test, never by assumption.
EOF
