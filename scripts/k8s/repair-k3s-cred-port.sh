#!/usr/bin/env bash
# D1 (2026-11-08): repair k3s cred kubeconfigs that still point at a previous apiserver port.
# colima 0.10.3 (`kubernetes.port: 0`) picks a random k3s_listen_port on VM creation, persists it in
# /etc/colima/colima.json, and reuses that stored value over --k3s-listen-port and colima.yaml (it is
# NOT re-randomized every start). k3s writes /var/lib/rancher/k3s/server/cred/*.kubeconfig ONCE at
# bootstrap (listen+1) and never regenerates them (deleting them makes k3s fatal), so changing the
# stored listen port after bootstrap makes controller-manager dial the old port and crash-loop k3s.
# Pinning (scripts/colima-k3s-port.txt) covers new VMs; existing VMs need a one-time edit of
# /etc/colima/colima.json + restart, then this script rewrites creds. Idempotent: no change -> no restart.
# Scope: only the managed colima profile (D43). Nothing is printed as OK before the stability probe.
# Usage: repair-k3s-cred-port.sh      env: COLIMA_BIN, KUBE_CONTEXT (must equal the managed context)
set -euo pipefail

# Pure POSIX-sh functions shared between local fixture tests and remote VM execution (`sh -s`).
parse_secure_port() {
  _psp_line="$(awk '/Running kube-apiserver/ && !found { print; found=1 }')"
  [ -n "$_psp_line" ] || {
    echo "no 'Running kube-apiserver' line in the k3s journal" >&2
    return 2
  }
  case "$_psp_line" in
    *--secure-port=*)
      _psp_raw="${_psp_line#*--secure-port=}"
      _psp_tok="${_psp_raw%%[[:space:]]*}"
      _psp_val="$(printf '%s' "$_psp_tok" | cut -d'"' -f1)"
      ;;
    *)
      echo "no '--secure-port=' flag in 'Running kube-apiserver' journal line" >&2
      return 2
      ;;
  esac
  case "$_psp_val" in
    '' | 0* | *[!0-9]*)
      echo "malformed --secure-port value: '$_psp_val'" >&2
      return 2
      ;;
  esac
  if [ "$_psp_val" -gt 65535 ]; then
    echo "--secure-port out of range (1..65535): '$_psp_val'" >&2
    return 2
  fi
  printf '%s\n' "$_psp_val"
}

parse_stored_listen_port() {
  _psl_file="$1"
  [ -r "$_psl_file" ] || return 0
  _psl_first="$(grep -Eo '"k3s_listen_port"[[:space:]]*:[[:space:]]*("[0-9]+"|[0-9]+)' "$_psl_file" | sed -n '1p' || true)"
  _psl_rhs="${_psl_first#*:}"
  printf '%s' "$_psl_rhs" | tr -dc '0-9'
  printf '\n'
}

rewrite_creds() {
  _rc_dir="${1:-}"
  _rc_port="${2:-}"
  [ -d "$_rc_dir" ] || {
    echo "cred directory not found: '$_rc_dir'" >&2
    return 2
  }
  case "$_rc_port" in
    '' | 0* | *[!0-9]*)
      echo "invalid target apiserver port: '$_rc_port'" >&2
      return 2
      ;;
  esac
  if [ "$_rc_port" -gt 65535 ]; then
    echo "target apiserver port out of range (1..65535): '$_rc_port'" >&2
    return 2
  fi
  _rc_changed=0
  for _rc_f in "$_rc_dir"/*.kubeconfig; do
    [ -f "$_rc_f" ] || {
      echo "no *.kubeconfig files in '$_rc_dir'" >&2
      return 2
    }
    grep -q '^[[:space:]]*server: https://127\.0\.0\.1:[0-9][0-9]*$' "$_rc_f" || {
      echo "missing loopback server line in '$_rc_f'" >&2
      return 2
    }
    grep -q "^[[:space:]]*server: https://127\\.0\\.0\\.1:${_rc_port}\$" "$_rc_f" || _rc_changed=1
  done
  if [ "$_rc_changed" = 1 ]; then
    _rc_backup="${_rc_dir%/}-backup-$(date +%Y%m%d-%H%M%S)"
    if [ -e "$_rc_backup" ]; then
      _rc_seq=1
      while [ -e "${_rc_backup}-${_rc_seq}" ]; do
        _rc_seq=$((_rc_seq + 1))
      done
      _rc_backup="${_rc_backup}-${_rc_seq}"
    fi
    if ! mkdir -p "$_rc_backup" || ! cp -p "$_rc_dir"/*.kubeconfig "$_rc_backup"/; then
      echo "failed to back up *.kubeconfig to '$_rc_backup'" >&2
      rm -f "$_rc_backup"/*.kubeconfig 2>/dev/null || true
      rmdir "$_rc_backup" 2>/dev/null || true
      return 1
    fi
    # Stage every rewrite in $_rc_dir first (same filesystem; cp -p preserves 0600 + ownership)
    # and rename only after all staged files pass, preventing a mixed-port cred set on failure.
    for _rc_f in "$_rc_dir"/*.kubeconfig; do
      _rc_tmp="${_rc_f}.tmp.$$"
      rm -f "$_rc_tmp" 2>/dev/null || true
      if ! cp -p "$_rc_f" "$_rc_tmp" ||
        ! sed "s#^\\([[:space:]]*server: https://127\\.0\\.0\\.1:\\)[0-9][0-9]*\$#\\1${_rc_port}#" "$_rc_f" >"$_rc_tmp" ||
        ! grep -q "^[[:space:]]*server: https://127\\.0\\.0\\.1:${_rc_port}\$" "$_rc_tmp"; then
        echo "failed to stage rewritten kubeconfig for '$_rc_f'; originals left untouched" >&2
        rm -f "$_rc_dir"/*.kubeconfig.tmp."$$" 2>/dev/null || true
        rm -f "$_rc_backup"/*.kubeconfig 2>/dev/null || true
        rmdir "$_rc_backup" 2>/dev/null || true
        return 1
      fi
    done
    for _rc_f in "$_rc_dir"/*.kubeconfig; do
      _rc_tmp="${_rc_f}.tmp.$$"
      if ! mv -f "$_rc_tmp" "$_rc_f"; then
        rm -f "$_rc_dir"/*.kubeconfig.tmp."$$" 2>/dev/null || true
        _rc_restore_ok=1
        for _rc_bf in "$_rc_backup"/*.kubeconfig; do
          _rc_bname="${_rc_bf##*/}"
          cp -pf "$_rc_bf" "${_rc_dir%/}/$_rc_bname" || _rc_restore_ok=0
        done
        if [ "$_rc_restore_ok" = 1 ]; then
          echo "failed to move staged kubeconfig onto '$_rc_f'; restored all kubeconfigs from '$_rc_backup'" >&2
        else
          echo "failed to move staged kubeconfig onto '$_rc_f' and failed to restore all kubeconfigs from '$_rc_backup'" >&2
        fi
        return 1
      fi
    done
    echo "backup=$_rc_backup"
  fi
  echo "port=$_rc_port"
  echo "changed=$_rc_changed"
}

phase1_remote() {
  set -eu
  _p1_cred_dir="$1"
  _p1_colima_json="$2"
  echo "stored_listen_port=$(parse_stored_listen_port "$_p1_colima_json")"
  _p1_port="$(journalctl -u k3s --no-pager -r | parse_secure_port)"
  ss -ltn | grep -q "127.0.0.1:${_p1_port} " ||
    echo "warn: nothing listening on 127.0.0.1:${_p1_port} right now" >&2
  rewrite_creds "$_p1_cred_dir" "$_p1_port"
}

main() {
  local script_dir colima ctx pinned_port_file cred_dir colima_json stable_secs step_secs ready_tries
  local pinned_port phase1 p1_rc out stored_port port changed state_probe base_nr st nr i probe_out elapsed ready

  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  # shellcheck source=scripts/colima-profile.sh
  . "${script_dir}/../colima-profile.sh"

  colima="${COLIMA_BIN-$(command -v colima || echo colima)}"
  ctx="${KUBE_CONTEXT:-$COLIMA_CONTEXT}"
  pinned_port_file="${REPAIR_PINNED_PORT_FILE:-${script_dir}/../colima-k3s-port.txt}"
  cred_dir="${REPAIR_CRED_DIR:-/var/lib/rancher/k3s/server/cred}"
  colima_json="${REPAIR_COLIMA_JSON:-/etc/colima/colima.json}"
  stable_secs="${REPAIR_STABLE_SECS:-30}"
  step_secs="${REPAIR_STEP_SECS:-3}"
  ready_tries="${REPAIR_READY_TRIES:-40}"

  die() { echo "repair-k3s-cred-port: $*" >&2; exit 1; }
  [[ $# -eq 0 ]] || die "takes no arguments (profile comes from colima-profile.sh / KUBEMETAL_COLIMA_PROFILE)"
  [[ "$ctx" == "$COLIMA_CONTEXT" ]] ||
    die "refusing context '$ctx': only the managed context '$COLIMA_CONTEXT' is allowed (D43)"
  [[ -r "$pinned_port_file" ]] || die "missing pinned port source: $pinned_port_file"
  pinned_port="$(tr -d '[:space:]' <"$pinned_port_file")"
  if [[ ! "$pinned_port" =~ ^[1-9][0-9]*$ ]] || ((pinned_port < 1024 || pinned_port >= 65535)); then
    die "invalid pinned k3s listen port '$pinned_port' in $pinned_port_file"
  fi

  # Root-only cred dir and /etc/colima/colima.json: run inside the VM's sudo POSIX shell.
  vm() { "$colima" --profile "$KUBEMETAL_COLIMA_PROFILE" ssh -- sudo sh -s -- "$@"; }
  journal_tail() { vm journal <<<'journalctl -u k3s --no-pager -n 40' >&2 || true; }

  # Phase 1: inspect stored /etc/colima/colima.json port, measure running apiserver --secure-port,
  # cross-check the listener, and back up + rewrite cred kubeconfigs only when needed.
  # shellcheck disable=SC2016 # $1/$2 expand inside the remote VM shell.
  phase1="$(declare -f parse_secure_port parse_stored_listen_port rewrite_creds phase1_remote)"$'\n''phase1_remote "$1" "$2"'

  p1_rc=0
  out="$(vm "$cred_dir" "$colima_json" <<<"$phase1")" || p1_rc=$?
  [[ -z "$out" ]] || echo "$out"

  stored_port="$(sed -n 's/^stored_listen_port=//p' <<<"$out")"
  if [[ -n "$stored_port" && "$stored_port" != "$pinned_port" ]]; then
    cat >&2 <<EOF
WARN: VM $colima_json stores k3s_listen_port=$stored_port, which differs from scripts/colima-k3s-port.txt ($pinned_port).
WARN: colima 0.10.3 reuses $colima_json over --k3s-listen-port and colima.yaml; existing VMs are not migrated by the flag.
WARN: Manual one-time migration steps (this script performs no VM lifecycle ops):
  1. colima --profile $KUBEMETAL_COLIMA_PROFILE ssh -- sudo sed -i -E 's/("k3s_listen_port"[[:space:]]*:[[:space:]]*"?)[0-9]+("?)/\1'"$pinned_port"'\2/' /etc/colima/colima.json
  2. make cluster-down && make cluster-up
  3. make cluster-repair
EOF
  fi

  ((p1_rc == 0)) || die "measuring/rewriting creds failed"
  port="$(sed -n 's/^port=//p' <<<"$out")"
  changed="$(sed -n 's/^changed=//p' <<<"$out")"
  [[ "$port" =~ ^[0-9]+$ && "$changed" =~ ^[01]$ ]] || die "unparseable measurement: $out"

  if [[ "$changed" == 1 ]]; then
    echo "creds rewritten to 127.0.0.1:$port; restarting k3s"
    vm restart <<<'systemctl restart k3s' || die "systemctl restart k3s failed"
  else
    echo "creds already point at 127.0.0.1:$port; no restart"
  fi

  # Phase 2: wait for active, record baseline NRestarts at window start, and require no increase over stable_secs.
  # shellcheck disable=SC2016 # Command substitutions expand inside the remote VM shell.
  state_probe='printf "%s %s\n" "$(systemctl is-active k3s || true)" "$(systemctl show k3s -p NRestarts --value)"'
  base_nr=""
  st=""
  nr=""
  for ((i = 0; i < ready_tries; i++)); do
    probe_out="$(vm probe <<<"$state_probe" || true)"
    read -r st nr <<<"$probe_out"
    if [[ "$st" == active && "$nr" =~ ^[0-9]+$ ]]; then
      base_nr="$nr"
      break
    fi
    sleep "$step_secs"
  done
  if [[ -z "$base_nr" ]]; then
    journal_tail
    die "k3s did not become active (is-active=${st:-unknown} NRestarts=${nr:-unknown})"
  fi

  elapsed=0
  while ((elapsed < stable_secs)); do
    sleep "$step_secs"
    elapsed=$((elapsed + step_secs))
    if ! probe_out="$(vm probe <<<"$state_probe")"; then
      journal_tail
      die "k3s state probe failed after ${elapsed}s"
    fi
    read -r st nr <<<"$probe_out"
    if [[ "$st" != active || ! "$nr" =~ ^[0-9]+$ ]] || ((nr > base_nr)); then
      journal_tail
      die "k3s unstable (is-active=$st NRestarts=$nr baseline=$base_nr) after ${elapsed}s"
    fi
  done

  # Phase 3: node Ready through the host kubeconfig, as a user would see it.
  for ((i = 0; i < ready_tries; i++)); do
    ready="$(kubectl --context "$ctx" get node -o jsonpath='{range .items[*]}{.status.conditions[?(@.type=="Ready")].status}{"\n"}{end}' 2>/dev/null || true)"
    if [[ -n "$ready" ]] && ! grep -qv '^True$' <<<"$ready"; then
      echo "OK: k3s active, NRestarts=${base_nr} unchanged for ${stable_secs}s, node Ready (apiserver 127.0.0.1:$port)"
      exit 0
    fi
    sleep "$step_secs"
  done
  journal_tail
  die "node not Ready via context $ctx"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
