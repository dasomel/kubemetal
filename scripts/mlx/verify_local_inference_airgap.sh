#!/usr/bin/env bash
set -euo pipefail

RUNTIME="${RUNTIME:-omlx}"
ENDPOINT="${ENDPOINT:-http://127.0.0.1:8000}"
MODEL_DIR="${MODEL_DIR:-$HOME/.omlx/models}"
API_KEY="${API_KEY:-}"
TMP_HEALTH="${TMPDIR:-/tmp}/kubemetal-airgap-health.$$"
TMP_MODELS="${TMPDIR:-/tmp}/kubemetal-airgap-models.$$"
trap 'rm -f "$TMP_HEALTH" "$TMP_MODELS"' EXIT

case "$ENDPOINT" in
  http://127.0.0.1:*|http://localhost:*) ;;
  *) echo "FAIL: endpoint must remain loopback-only for this verification: $ENDPOINT" >&2; exit 1 ;;
esac

if [[ "$RUNTIME" == "omlx" ]]; then
  command -v omlx >/dev/null 2>&1 || { echo "FAIL: omlx executable not found" >&2; exit 1; }
elif [[ "$RUNTIME" == "mlx-lm" ]]; then
  python3 -c 'import mlx_lm' >/dev/null 2>&1 || { echo "FAIL: mlx_lm import failed" >&2; exit 1; }
else
  echo "FAIL: unsupported runtime: $RUNTIME" >&2
  exit 1
fi

[[ -d "$MODEL_DIR" ]] || { echo "FAIL: local model directory not found: $MODEL_DIR" >&2; exit 1; }
# BSD find on macOS does not provide GNU -maxdepth. The model tree is intentionally traversed
# without following symlinks (find's default) and stops after the first regular file.
if ! find "$MODEL_DIR" -type f -print -quit 2>/dev/null | grep -q .; then
  echo "FAIL: no local model files found under $MODEL_DIR" >&2
  exit 1
fi

# Keep this probe independent from corporate/system proxy configuration. A loopback runtime must
# remain usable without DNS, proxy, package registry or model hub access.
unset HTTP_PROXY HTTPS_PROXY ALL_PROXY http_proxy https_proxy all_proxy
export NO_PROXY='127.0.0.1,localhost'
export no_proxy="$NO_PROXY"

headers=(-H 'Accept: application/json')
if [[ -n "$API_KEY" ]]; then
  headers+=(-H "Authorization: Bearer $API_KEY")
fi

health_code=$(curl -sS -o "$TMP_HEALTH" -w '%{http_code}' "${headers[@]}" "$ENDPOINT/health" || true)
models_code=$(curl -sS -o "$TMP_MODELS" -w '%{http_code}' "${headers[@]}" "$ENDPOINT/v1/models" || true)

if [[ ! "$health_code" =~ ^2 ]]; then
  echo "FAIL: local health probe returned HTTP ${health_code:-unreachable}" >&2
  cat "$TMP_HEALTH" 2>/dev/null || true
  exit 1
fi
if [[ ! "$models_code" =~ ^2 ]]; then
  echo "FAIL: local /v1/models probe returned HTTP ${models_code:-unreachable}" >&2
  cat "$TMP_MODELS" 2>/dev/null || true
  exit 1
fi

cat <<EOF
PASS: local inference air-gap readiness
  runtime:   $RUNTIME
  endpoint:  $ENDPOINT
  model dir: $MODEL_DIR
  health:    HTTP $health_code
  models:    HTTP $models_code

This proves the selected runtime/model path and API probes are local-only. Final acceptance still
requires repeating this command with external networking actually disconnected.
EOF
