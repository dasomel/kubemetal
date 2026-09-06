#!/usr/bin/env bash
set -euo pipefail

# Evening/on-device verification for KubeMetal #58.
# The KubeMetal UI starts the private relay; this script verifies the same inference service
# from macOS loopback, the selected private bridge address, and the Colima VM.

TARGET_PORT="${TARGET_PORT:-8000}"
BRIDGE_HOST="${BRIDGE_HOST:-192.168.64.1}"
BRIDGE_PORT="${BRIDGE_PORT:-18000}"

require() {
  command -v "$1" >/dev/null 2>&1 || { echo "missing command: $1" >&2; exit 2; }
}

require curl
require colima

probe() {
  local name="$1" url="$2"
  printf '%-26s %s ... ' "$name" "$url"
  if curl --silent --show-error --fail --max-time 5 "$url" >/tmp/kubemetal-inference-probe.json; then
    echo "OK"
    cat /tmp/kubemetal-inference-probe.json
    echo
  else
    echo "FAIL"
    return 1
  fi
}

probe "macOS loopback" "http://127.0.0.1:${TARGET_PORT}/health"
probe "private bridge on host" "http://${BRIDGE_HOST}:${BRIDGE_PORT}/health"

if ! colima status >/dev/null 2>&1; then
  echo "Colima is not running; VM/K3s bridge verification skipped." >&2
  exit 3
fi

printf '%-26s %s ... ' "Colima VM" "http://host.lima.internal:${BRIDGE_PORT}/health"
if colima ssh -- sh -lc "command -v curl >/dev/null 2>&1 && curl -fsS --max-time 5 http://host.lima.internal:${BRIDGE_PORT}/health"; then
  echo
  echo "Colima VM → KubeMetal private bridge → oMLX: OK"
else
  echo
  echo "Colima VM probe failed. If curl is absent in the VM, validate from an existing K3s pod with:" >&2
  echo "  curl http://mac-gpu-service.default.svc.cluster.local:${BRIDGE_PORT}/health" >&2
  exit 4
fi

rm -f /tmp/kubemetal-inference-probe.json
