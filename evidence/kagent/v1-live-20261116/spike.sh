#!/usr/bin/env bash
# Stage-1 live spike for #193: kagent 1.0.1 on the app's own k3s (colima profile `kubemetal`).
# Usage: spike.sh SCRATCH_DIR PHASE [PHASE...]   phases: pre artifacts remove-0x install-1x agent llm restore
# Each phase writes raw command output to $EV/raw/<phase>/*. D22: success is printed only after the
# cluster state it claims has been re-read. Refuses any context other than colima-kubemetal.
set -euo pipefail

EV="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$EV/../../.." && pwd)"
SCRATCH="${1:?SCRATCH_DIR required}"; shift
NEW=1.0.1
OLD="$(cat "$REPO/scripts/helm/kagent-version.txt")"
PROFILE="$(cat "$REPO/scripts/colima-profile.txt")"
CTX="colima-$PROFILE"
NS=kagent
OCI=oci://ghcr.io/kagent-dev/kagent/helm
RAW="$EV/raw"
mkdir -p "$SCRATCH" "$RAW"
[[ "$PROFILE" == kubemetal ]] || { echo "refusing: profile is $PROFILE" >&2; exit 1; }

log() { printf '[%s] %s\n' "$(date -u +%H:%M:%S)" "$*"; }
# run NAME CMD... : tee stdout+stderr to raw/<phase>/NAME.txt, keep going on failure (rc recorded)
run() {
  local name="$1"; shift
  { printf '$ %s\n' "$*"; "$@"; printf '\n[rc=%s]\n' "$?"; } >"$RAW/$PHASE/$name.txt" 2>&1 || true
}
runrc() { # like run but propagates rc
  local name="$1"; shift
  local rc=0
  { printf '$ %s\n' "$*"; "$@"; } >"$RAW/$PHASE/$name.txt" 2>&1 || rc=$?
  printf '\n[rc=%s]\n' "$rc" >>"$RAW/$PHASE/$name.txt"; return $rc
}

use_isolated_kubeconfig() {
  kubectl config view --minify --flatten --context "$CTX" >"$SCRATCH/kubeconfig"
  chmod 600 "$SCRATCH/kubeconfig"
  export KUBECONFIG="$SCRATCH/kubeconfig"
  local n; n="$(kubectl config get-contexts -o name | wc -l | tr -d ' ')"
  [[ "$n" == 1 && "$(kubectl config get-contexts -o name)" == "$CTX" ]] \
    || { echo "refusing: isolated kubeconfig must have exactly context $CTX" >&2; exit 1; }
}
need_ctx() {
  [[ -f "$SCRATCH/kubeconfig" ]] || { echo "run phase pre first" >&2; exit 1; }
  export KUBECONFIG="$SCRATCH/kubeconfig"
  [[ "$(kubectl config current-context)" == "$CTX" ]] || { echo "wrong context" >&2; exit 1; }
}
K() { kubectl --context "$CTX" "$@"; }
H() { helm --kube-context "$CTX" "$@"; }

wait_api() { # bounded
  for i in $(seq 1 60); do K get --raw=/readyz >/dev/null 2>&1 && return 0; sleep 5; done; return 1
}

ph_pre() {
  [[ -f "$RAW/$PHASE/colima-list-before.txt" ]] || run colima-list-before colima list
  # Host-global default kubeconfig current-context, recorded because colima start may change it.
  kubectl config current-context >"$RAW/$PHASE/default-current-context-before.txt" 2>&1 || true
  ( cd "$REPO" && runrc cluster-up make cluster-up )
  run colima-list-after-start colima list
  run colima-status colima --profile "$PROFILE" status
  kubectl config current-context >"$RAW/$PHASE/default-current-context-after-start.txt" 2>&1 || true
  use_isolated_kubeconfig
  wait_api || { echo "FAIL: API not ready" >&2; exit 1; }
  sleep 45; wait_api || { echo "FAIL: API forward lost after 45s (lima ssh forward)" >&2; exit 1; }
  run nodes K get nodes -o wide
  run version K version
  run helm-releases-all H list -A
  run namespaces K get ns
  run pods-all K get pods -A -o wide
  run top-node K top node
  run crds-kagent bash -c "kubectl --context $CTX get crd -o name | grep -i kagent || true"
  mkdir -p "$SCRATCH/backup"
  : >"$RAW/$PHASE/existing-cr-names.txt"
  for crd in $(K get crd -o name | grep -i kagent | sed 's#.*/##'); do
    if K get "$crd" -A -o yaml >"$SCRATCH/backup/$crd.yaml" 2>/dev/null; then
      K get "$crd" -A --no-headers -o custom-columns=NS:.metadata.namespace,NAME:.metadata.name 2>/dev/null \
        | sed "s#^#$crd #" >>"$RAW/$PHASE/existing-cr-names.txt" || true
    fi
  done
  run kagent-ns-all K -n "$NS" get all,cm,secret,sa,pvc -o wide   # names only (no -o yaml on secrets)
  run node-allocatable K get nodes -o jsonpath='{.items[*].status.allocatable}'
  run pods-kagent-resources K -n "$NS" get pods -o custom-columns='POD:.metadata.name,REQ:.spec.containers[*].resources.requests,LIM:.spec.containers[*].resources.limits'
  K get node -o name >/dev/null && log "pre OK: API ready, ctx=$CTX"
}

ph_artifacts() {
  need_ctx
  local d="$SCRATCH/charts"; rm -rf "$d"; mkdir -p "$d/new" "$d/old"
  for c in kagent kagent-crds; do
    runrc "pull-$c-$NEW" helm pull "$OCI/$c" --version "$NEW" --destination "$d/new"
    run "show-chart-$c-$NEW" helm show chart "$OCI/$c" --version "$NEW"
    run "show-values-$c-$NEW" helm show values "$OCI/$c" --version "$NEW"
    runrc "pull-$c-$OLD" helm pull "$OCI/$c" --version "$OLD" --destination "$d/old"
  done
  ( cd "$d" && shasum -a 256 new/*.tgz old/*.tgz ) >"$RAW/$PHASE/sha256.txt"
  run crds-render helm template kagent-crds "$d/new/kagent-crds-$NEW.tgz" -n "$NS"
  helm template kagent-crds "$d/new/kagent-crds-$NEW.tgz" -n "$NS" >"$SCRATCH/crds-1.0.1.yaml" 2>/dev/null
  helm template kagent "$d/new/kagent-$NEW.tgz" -n "$NS" >"$SCRATCH/render-default-1.0.1.yaml" 2>"$RAW/$PHASE/template-default.stderr.txt" || true
  python3 -I - "$SCRATCH/crds-1.0.1.yaml" >"$RAW/$PHASE/crd-gvk-1.0.1.txt" <<'PY'
import sys, yaml
for d in yaml.safe_load_all(open(sys.argv[1])):
    if d and d.get("kind") == "CustomResourceDefinition":
        s = d["spec"]
        print(s["group"], s["names"]["kind"], ",".join(f'{v["name"]}{"*" if v.get("storage") else ""}' for v in s["versions"]), s["scope"])
PY
  grep -hE '^\s*image:' "$SCRATCH/render-default-1.0.1.yaml" | sed 's/^\s*image:\s*//; s/"//g' | sort -u >"$RAW/$PHASE/images-default-1.0.1.txt" || true
  log "artifacts OK: $(wc -l <"$RAW/$PHASE/crd-gvk-1.0.1.txt") CRDs, $(wc -l <"$RAW/$PHASE/images-default-1.0.1.txt") images"
}

ph_down() {
  colima list | awk 'NR>1 && $1=="kubemetal" && $2=="Running"{f=1} END{exit !f}' && ( cd "$REPO" && runrc cluster-down make cluster-down )
  run colima-list-final colima list
  kubectl config current-context >"$RAW/$PHASE/default-current-context-final.txt" 2>&1 || true
  colima list | awk '$1=="kubemetal"{print}' | grep -q Stopped && log "down OK: kubemetal Stopped"
}

for PHASE in "$@"; do
  mkdir -p "$RAW/$PHASE"
  log "== phase $PHASE"
  "ph_${PHASE//-/_}"
done
