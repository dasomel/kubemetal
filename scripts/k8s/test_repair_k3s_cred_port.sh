#!/usr/bin/env bash
# D1 regression: repair-k3s-cred-port.sh exercises real port extraction, cred backup/rewrite,
# stored colima.json port check, and NRestarts stability window against fixture files + stub CLIs; no VM.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SUT="${SCRIPT_DIR}/repair-k3s-cred-port.sh"
# shellcheck source=scripts/k8s/repair-k3s-cred-port.sh
. "$SUT"

TEST_DIR="$(mktemp -d -t kubemetal-repair)"
trap 'chmod -R u+w "$TEST_DIR" 2>/dev/null || true; rm -rf "$TEST_DIR"' EXIT
mkdir -p "$TEST_DIR/bin" "$TEST_DIR/cred" "$TEST_DIR/orig"

REAL_MV="$(command -v mv)"
REAL_SED="$(command -v sed)"
export REAL_MV REAL_SED
export CALLS="$TEST_DIR/calls"
export STATE_DIR="$TEST_DIR"
export REPAIR_CRED_DIR="$TEST_DIR/cred"
export REPAIR_COLIMA_JSON="$TEST_DIR/colima.json"
export REPAIR_STABLE_SECS=2 REPAIR_STEP_SECS=1 REPAIR_READY_TRIES=2
export PATH="$TEST_DIR/bin:$PATH" COLIMA_BIN="$TEST_DIR/bin/colima"

DEFAULT_JOURNAL=$'Nov 08 12:05:00 vm k3s[200]: time="2026-11-08T12:05:00Z" level=info msg="Running kube-apiserver --advertise-port=36443 --secure-port=36444 --tls-cert-file=/var/lib/rancher/k3s/server/tls/serving-kube-apiserver.crt"\nNov 08 11:00:00 vm k3s[100]: time="2026-11-08T11:00:00Z" level=info msg="Running kube-apiserver --secure-port=53017"'
export STUB_JOURNAL="$DEFAULT_JOURNAL"

# colima stub: logs argv and executes the real `sh -s -- ...` payload sent by repair-k3s-cred-port.sh
# (using /bin/dash when present to mirror Ubuntu's /bin/sh inside the Colima VM).
cat >"$TEST_DIR/bin/colima" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
echo "colima $*" >>"$CALLS"
if [[ "${1:-}" == "--profile" && "${3:-}" == "ssh" && "${4:-}" == "--" && "${5:-}" == "sudo" ]]; then
  shift 5
  if [[ "${1:-}" == "sh" && -x /bin/dash ]]; then
    shift
    exec /bin/dash "$@"
  fi
  exec "$@"
fi
echo "unexpected colima invocation: $*" >&2
exit 99
STUB

cat >"$TEST_DIR/bin/journalctl" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "${STUB_JOURNAL-}"
STUB

cat >"$TEST_DIR/bin/ss" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "${STUB_SS:-LISTEN 0 4096 127.0.0.1:36444 0.0.0.0:*}"
STUB

cat >"$TEST_DIR/bin/systemctl" <<'STUB'
#!/usr/bin/env bash
echo "systemctl $*" >>"$CALLS"
pop_seq() {
  local f="$1" fallback="$2" first rest
  if [[ -s "$f" ]]; then
    IFS= read -r first <"$f"
    rest="$(tail -n +2 "$f")"
    if [[ -n "$rest" ]]; then printf '%s\n' "$rest" >"$f"; else printf '%s\n' "$first" >"$f"; fi
    printf '%s\n' "$first"
  else
    printf '%s\n' "$fallback"
  fi
}
case "$*" in
  "is-active k3s") pop_seq "$STATE_DIR/active_seq" "${STUB_ACTIVE:-active}" ;;
  "show k3s -p NRestarts --value") pop_seq "$STATE_DIR/nrestarts_seq" "${STUB_NRESTARTS:-0}" ;;
  "restart k3s") exit "${STUB_RESTART_RC:-0}" ;;
  *) echo "unexpected systemctl: $*" >&2; exit 98 ;;
esac
STUB

cat >"$TEST_DIR/bin/kubectl" <<'STUB'
#!/usr/bin/env bash
echo "kubectl $*" >>"$CALLS"
printf '%s\n' "${STUB_READY:-True}"
STUB

# Fault-injection stubs (test-only): pass straight through to real binaries unless
# STUB_FAIL_STAGE_N / STUB_FAIL_MV_N targets the Nth kubeconfig stage write or mv.
cat >"$TEST_DIR/bin/sed" <<'STUB'
#!/usr/bin/env bash
if [[ -n "${STUB_FAIL_STAGE_N:-}" && "${2:-}" == *.kubeconfig ]]; then
  n=0
  [[ -f "$STATE_DIR/stage_count" ]] && n="$(cat "$STATE_DIR/stage_count")"
  n=$((n + 1))
  printf '%s\n' "$n" >"$STATE_DIR/stage_count"
  if [[ "$n" -eq "$STUB_FAIL_STAGE_N" ]]; then
    compgen -G "$REPAIR_CRED_DIR/*.tmp.*" >"$STATE_DIR/mid_stage_tmps" || true
    echo "sed: simulated staging failure on call $n" >&2
    exit 1
  fi
fi
exec "$REAL_SED" "$@"
STUB

cat >"$TEST_DIR/bin/mv" <<'STUB'
#!/usr/bin/env bash
if [[ -n "${STUB_FAIL_MV_N:-}" ]]; then
  n=0
  [[ -f "$STATE_DIR/mv_count" ]] && n="$(cat "$STATE_DIR/mv_count")"
  n=$((n + 1))
  printf '%s\n' "$n" >"$STATE_DIR/mv_count"
  if [[ "$n" -eq "$STUB_FAIL_MV_N" ]]; then
    "$REAL_SED" -n 's/^[[:space:]]*server: //p' "$REPAIR_CRED_DIR/admin.kubeconfig" >"$STATE_DIR/mid_mv_admin"
    echo "mv: simulated rename failure on call $n" >&2
    exit 1
  fi
fi
exec "$REAL_MV" "$@"
STUB

chmod +x "$TEST_DIR/bin/colima" "$TEST_DIR/bin/journalctl" "$TEST_DIR/bin/ss" \
  "$TEST_DIR/bin/systemctl" "$TEST_DIR/bin/kubectl" "$TEST_DIR/bin/sed" "$TEST_DIR/bin/mv"

seed_fixtures() {
  [[ ! -d "$TEST_DIR/cred" ]] || chmod 700 "$TEST_DIR/cred"
  rm -rf "$TEST_DIR"/cred-backup-* "$TEST_DIR/active_seq" "$TEST_DIR/nrestarts_seq" \
    "$TEST_DIR/stage_count" "$TEST_DIR/mv_count" "$TEST_DIR/mid_stage_tmps" "$TEST_DIR/mid_mv_admin"
  rm -f "$TEST_DIR/cred"/*.tmp.* 2>/dev/null || true
  mkdir -p "$TEST_DIR/cred"
  cat >"$TEST_DIR/cred/controller.kubeconfig" <<'EOF'
apiVersion: v1
# comment server: https://127.0.0.1:99999
clusters:
- cluster:
    certificate-authority-data: FAKECA==
    server: https://127.0.0.1:53017
  name: default
contexts:
- context:
    cluster: default
    user: controller
  name: default
current-context: default
note-url: https://127.0.0.1:12345/healthz
EOF
  cat >"$TEST_DIR/cred/admin.kubeconfig" <<'EOF'
apiVersion: v1
clusters:
- cluster:
    certificate-authority-data: ADMINCA==
    server: https://127.0.0.1:53017
    tls-server-name: kubernetes.default
  name: default
EOF
  chmod 600 "$TEST_DIR/cred/controller.kubeconfig" "$TEST_DIR/cred/admin.kubeconfig"
  echo "untouched-non-kubeconfig" >"$TEST_DIR/cred/README.keep"
  cp -p "$TEST_DIR/cred/controller.kubeconfig" "$TEST_DIR/orig/controller.kubeconfig"
  cp -p "$TEST_DIR/cred/admin.kubeconfig" "$TEST_DIR/orig/admin.kubeconfig"
  printf '{"k3s_listen_port": 36443}\n' >"$TEST_DIR/colima.json"
  export STUB_JOURNAL="$DEFAULT_JOURNAL"
  unset STUB_ACTIVE STUB_NRESTARTS STUB_READY STUB_RESTART_RC STUB_FAIL_STAGE_N STUB_FAIL_MV_N || true
}

fail() { echo "FAIL: $*" >&2; exit 1; }
run() { : >"$CALLS"; "$@" 2>&1; }

assert_mode_600() {
  local f mode_str
  for f in "$TEST_DIR/cred/controller.kubeconfig" "$TEST_DIR/cred/admin.kubeconfig"; do
    mode_str="$(ls -ld "$f")"
    case "$mode_str" in
      -rw-------*) ;;
      *) fail "$1: expected mode 0600 on $f, got: $mode_str" ;;
    esac
  done
}

assert_creds_untouched() {
  cmp -s "$TEST_DIR/cred/controller.kubeconfig" "$TEST_DIR/orig/controller.kubeconfig" ||
    fail "$1: controller.kubeconfig was modified"
  cmp -s "$TEST_DIR/cred/admin.kubeconfig" "$TEST_DIR/orig/admin.kubeconfig" ||
    fail "$1: admin.kubeconfig was modified"
  compgen -G "$TEST_DIR/cred-backup-*" >/dev/null && fail "$1: unexpected backup created"
  return 0
}

# 1. Context, arg, and profile guards reject before invoking colima.
seed_fixtures
out="$(KUBE_CONTEXT=colima-narwhal run bash "$SUT")" && fail "foreign context accepted"
grep -q "refusing context" <<<"$out" || fail "no refusal message: $out"
[[ ! -s "$CALLS" ]] || fail "foreign context reached colima"

out="$(run bash "$SUT" other)" && fail "argument accepted"
grep -q "takes no arguments" <<<"$out" || fail "no arg refusal: $out"

out="$(KUBEMETAL_COLIMA_PROFILE=default run bash "$SUT")" && fail "reserved profile accepted"

# 2. Malformed or absent --secure-port fails without touching files or creating backups.
for bad_journal in \
  "" \
  "Nov 08 12:00:00 vm systemd[1]: Started k3s" \
  'Nov 08 12:00:00 vm k3s[1]: msg="Running kube-apiserver --advertise-port=36443"' \
  'Nov 08 12:00:00 vm k3s[1]: msg="Running kube-apiserver --secure-port="' \
  'Nov 08 12:00:00 vm k3s[1]: msg="Running kube-apiserver --secure-port=abc"' \
  'Nov 08 12:00:00 vm k3s[1]: msg="Running kube-apiserver --secure-port=36444xyz"' \
  'Nov 08 12:00:00 vm k3s[1]: msg="Running kube-apiserver --secure-port=0"' \
  'Nov 08 12:00:00 vm k3s[1]: msg="Running kube-apiserver --secure-port=70000"'; do
  seed_fixtures
  if parse_secure_port <<<"$bad_journal" >/dev/null 2>&1; then
    fail "parse_secure_port accepted bad journal: $bad_journal"
  fi
  out="$(STUB_JOURNAL="$bad_journal" run bash "$SUT")" && fail "SUT succeeded on bad journal: $bad_journal"
  ! grep -q '^OK' <<<"$out" || fail "OK printed on bad journal: $out"
  ! grep -q 'systemctl restart k3s' "$CALLS" || fail "restarted k3s on bad journal"
  assert_creds_untouched "bad journal ($bad_journal)"
done

seed_fixtures
if rewrite_creds "$TEST_DIR/cred" "notaport" >/dev/null 2>&1; then
  fail "rewrite_creds accepted non-numeric port"
fi
assert_creds_untouched "rewrite_creds bad port"

# 3. Drifted fixture kubeconfigs: server lines rewritten to 36444, other lines untouched, backup created.
seed_fixtures
out="$(run bash "$SUT")" || fail "happy path failed: $out"
grep -q '^OK: ' <<<"$out" || fail "no OK: $out"
! grep -q '^WARN:' <<<"$out" || fail "unexpected WARN when colima.json matches pinned port: $out"
grep -q '^changed=1$' <<<"$out" || fail "expected changed=1: $out"
grep -q "restarting k3s" <<<"$out" || fail "changed creds must restart: $out"
grep -q -- 'colima --profile kubemetal ssh -- sudo sh -s' "$CALLS" || fail "profile ssh not passed: $(cat "$CALLS")"
grep -q 'systemctl restart k3s' "$CALLS" || fail "systemctl restart k3s not called"

for f in controller.kubeconfig admin.kubeconfig; do
  grep -q '^    server: https://127\.0\.0\.1:36444$' "$TEST_DIR/cred/$f" ||
    fail "$f server line not rewritten to 36444: $(cat "$TEST_DIR/cred/$f")"
  ! grep -q '53017' "$TEST_DIR/cred/$f" || fail "$f still contains old port 53017"
  diff -u <(grep -v '^[[:space:]]*server: ' "$TEST_DIR/orig/$f") \
          <(grep -v '^[[:space:]]*server: ' "$TEST_DIR/cred/$f") ||
    fail "$f non-server lines were modified"
done
[[ "$(cat "$TEST_DIR/cred/README.keep")" == "untouched-non-kubeconfig" ]] || fail "non-kubeconfig file touched"
assert_mode_600 "happy path rewrite"
! compgen -G "$TEST_DIR/cred/*.tmp.*" >/dev/null || fail "temp files left after happy path"

backup_dir="$(sed -n 's/^backup=//p' <<<"$out")"
[[ -n "$backup_dir" && -d "$backup_dir" ]] || fail "backup directory missing: $backup_dir"
cmp -s "$backup_dir/controller.kubeconfig" "$TEST_DIR/orig/controller.kubeconfig" ||
  fail "backup controller.kubeconfig does not match pre-rewrite file"
cmp -s "$backup_dir/admin.kubeconfig" "$TEST_DIR/orig/admin.kubeconfig" ||
  fail "backup admin.kubeconfig does not match pre-rewrite file"

# 4. Idempotent second run against the already-rewritten dir reports changed=0 and does not restart.
out="$(run bash "$SUT")" || fail "idempotent second run failed: $out"
grep -q '^changed=0$' <<<"$out" || fail "second run did not report changed=0: $out"
grep -q "no restart" <<<"$out" || fail "unchanged creds must not restart: $out"
! grep -q '^backup=' <<<"$out" || fail "second run created redundant backup: $out"
! grep -q 'systemctl restart k3s' "$CALLS" || fail "second run called systemctl restart"

# 5. Stability window records baseline NRestarts: non-zero constant passes; any increase fails.
out="$(STUB_NRESTARTS=5 run bash "$SUT")" || fail "stable non-zero NRestarts=5 failed: $out"
grep -q '^OK: .*NRestarts=5 unchanged' <<<"$out" || fail "expected OK with NRestarts=5 unchanged: $out"

printf '5\n5\n6\n' >"$TEST_DIR/nrestarts_seq"
out="$(run bash "$SUT")" && fail "increasing NRestarts (5->6) reported OK"
! grep -q '^OK' <<<"$out" || fail "OK printed when NRestarts increased"
grep -q 'k3s unstable.*NRestarts=6 baseline=5' <<<"$out" || fail "missing baseline instability detail: $out"
rm -f "$TEST_DIR/nrestarts_seq"

printf 'active\nactivating\n' >"$TEST_DIR/active_seq"
out="$(run bash "$SUT")" && fail "inactive transition reported OK"
! grep -q '^OK' <<<"$out" || fail "OK printed when k3s left active"
rm -f "$TEST_DIR/active_seq"

# 6. Stored /etc/colima/colima.json k3s_listen_port mismatch prints WARN with manual steps and runs no lifecycle ops.
printf '{"k3s_listen_port": "53016", "other": true}\n' >"$TEST_DIR/colima.json"
[[ "$(parse_stored_listen_port "$TEST_DIR/colima.json")" == "53016" ]] || fail "quoted stored port parse failed"
out="$(run bash "$SUT")" || fail "stored port mismatch should warn, not fail healthy cluster: $out"
grep -q 'WARN:.*53016.*36443' <<<"$out" || fail "missing stored-port mismatch WARN: $out"
grep -q 'make cluster-down && make cluster-up' <<<"$out" || fail "missing manual restart step in WARN: $out"
grep -q 'make cluster-repair' <<<"$out" || fail "missing cluster-repair step in WARN: $out"
! grep -E '^colima' "$CALLS" | grep -qwE 'start|stop|delete|restart' || fail "lifecycle op invoked: $(cat "$CALLS")"

# 7. NotReady node still fails without printing OK.
printf '{"k3s_listen_port": 36443}\n' >"$TEST_DIR/colima.json"
out="$(STUB_READY=False run bash "$SUT")" && fail "NotReady node reported OK"
! grep -q '^OK' <<<"$out" || fail "OK printed for NotReady node"

# 8. Atomic all-or-nothing rewrite: staging failure and 2nd-mv failure leave no mixed state or temp files.
if [[ "$(id -u)" -ne 0 ]]; then
  seed_fixtures
  chmod 500 "$TEST_DIR/cred"
  out="$(run bash "$SUT")" && { chmod 700 "$TEST_DIR/cred"; fail "SUT succeeded with read-only cred dir"; }
  chmod 700 "$TEST_DIR/cred"
  ! grep -q '^OK' <<<"$out" || fail "OK printed on read-only cred dir: $out"
  ! grep -q 'systemctl restart k3s' "$CALLS" || fail "restarted k3s on read-only cred dir"
  ! compgen -G "$TEST_DIR/cred/*.tmp.*" >/dev/null || fail "temp files left after read-only staging failure"
  assert_creds_untouched "staging failure (read-only dir)"
  assert_mode_600 "staging failure (read-only dir)"
fi

seed_fixtures
out="$(STUB_FAIL_STAGE_N=2 run bash "$SUT")" && fail "SUT succeeded when 2nd staged write failed"
[[ -s "$STATE_DIR/mid_stage_tmps" ]] || fail "expected 1st temp file to exist before 2nd stage failure"
grep -q "failed to stage rewritten kubeconfig" <<<"$out" || fail "missing staging failure message: $out"
! grep -q '^OK' <<<"$out" || fail "OK printed on 2nd staging failure: $out"
! grep -q 'systemctl restart k3s' "$CALLS" || fail "restarted k3s on 2nd staging failure"
! compgen -G "$TEST_DIR/cred/*.tmp.*" >/dev/null || fail "temp files left after 2nd staging failure"
assert_creds_untouched "staging failure (2nd file)"
assert_mode_600 "staging failure (2nd file)"

seed_fixtures
out="$(STUB_FAIL_MV_N=2 run bash "$SUT")" && fail "SUT succeeded when 2nd mv failed"
[[ "$(cat "$STATE_DIR/mid_mv_admin")" == "https://127.0.0.1:36444" ]] ||
  fail "1st mv did not rewrite admin.kubeconfig before 2nd mv failed: $(cat "$STATE_DIR/mid_mv_admin" 2>/dev/null || true)"
grep -q "restored all kubeconfigs from" <<<"$out" || fail "missing backup restore message: $out"
! grep -q '^OK' <<<"$out" || fail "OK printed on 2nd mv failure: $out"
! grep -q 'systemctl restart k3s' "$CALLS" || fail "restarted k3s on 2nd mv failure"
! compgen -G "$TEST_DIR/cred/*.tmp.*" >/dev/null || fail "temp files left after 2nd mv failure"
cmp -s "$TEST_DIR/cred/controller.kubeconfig" "$TEST_DIR/orig/controller.kubeconfig" ||
  fail "2nd mv failure: controller.kubeconfig not restored to original"
cmp -s "$TEST_DIR/cred/admin.kubeconfig" "$TEST_DIR/orig/admin.kubeconfig" ||
  fail "2nd mv failure: admin.kubeconfig not restored to original"
assert_mode_600 "2nd mv failure restore"

seed_fixtures
if (export STUB_FAIL_MV_N=2; rewrite_creds "$TEST_DIR/cred" "36444" >/dev/null 2>&1); then
  fail "direct rewrite_creds succeeded when 2nd mv failed"
fi
! compgen -G "$TEST_DIR/cred/*.tmp.*" >/dev/null || fail "temp files left after direct rewrite_creds 2nd mv failure"
cmp -s "$TEST_DIR/cred/controller.kubeconfig" "$TEST_DIR/orig/controller.kubeconfig" ||
  fail "direct 2nd mv failure: controller.kubeconfig not restored"
cmp -s "$TEST_DIR/cred/admin.kubeconfig" "$TEST_DIR/orig/admin.kubeconfig" ||
  fail "direct 2nd mv failure: admin.kubeconfig not restored"
assert_mode_600 "direct 2nd mv failure restore"

echo "repair-k3s-cred-port: all checks passed"
