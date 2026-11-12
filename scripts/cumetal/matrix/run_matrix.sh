#!/bin/bash
# CuMetal compatibility matrix runner (issue #84 Phase 2).
# Usage: run_matrix.sh <out.json> [probe ...]   (also writes <out>.md next to the JSON)
# Builds every probe through AOT (cumetalc) and JIT (clang CUDA + link), runs it 3x with
# CUMETAL_TRACE_GPU=1, and classifies fail-closed: compile success alone is never "supported".
set -euo pipefail

OUT_JSON="${1:?usage: run_matrix.sh <out.json> [probe ...]}"
shift || true
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${CUMETAL_PREFIX:-$HOME/.local/opt/cumetal}"
CLANGXX=/opt/homebrew/opt/llvm/bin/clang++
CUMETALC=/opt/homebrew/bin/cumetalc
CUMETAL=/opt/homebrew/bin/cumetal
PY=/usr/bin/python3
PERL=/usr/bin/perl
export PATH="$PREFIX/libexec/cumetal/cuda_toolchain:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"

COMPILE_TIMEOUT="${MATRIX_COMPILE_TIMEOUT:-300}"
RUN_TIMEOUT="${MATRIX_RUN_TIMEOUT:-120}"
REPEATS=3

# probe:extra-lib (lib only added to the JIT link line; cumetalc resolved the libs on its own in measurement)
ALL_PROBES=(launch_1d launch_2d launch_3d shared_sync atomics warp_shuffle stream_event managed_memory fp64
  sgemm_handwritten cublas_sgemm:cublas cufft_c2c:cufft curand_host:curand cusparse_spmv:cusparse nan_inf)
if [ "$#" -gt 0 ]; then PROBES=("$@"); else PROBES=("${ALL_PROBES[@]}"); fi

# D22: keep each attempt beside the explicit output; only build products are temporary.
# Cost: evidence consumes disk until the recipient removes it; replay needs no tools.
MD="${OUT_JSON%.json}.md"
if [[ "$OUT_JSON" != *.json ]]; then MD="$OUT_JSON.md"; fi
[ ! -e "$OUT_JSON" ] && [ ! -e "$MD" ] || { echo "destination exists" >&2; exit 1; }
for spec in "${PROBES[@]}"; do
  [[ "$spec" =~ ^[a-zA-Z0-9_]+(:[a-zA-Z0-9_]+)?$ ]] || { echo "invalid probe: $spec" >&2; exit 1; }
done
WORK="$(mktemp -d "$OUT_JSON.evidence.XXXXXX")"
BIN="$(mktemp -d "${TMPDIR:-/tmp}/cumetal-matrix-bin.XXXXXX")"
cleanup() {
  local rc=$?
  trap - EXIT
  rm -rf "$BIN"
  if [ ! -f "$WORK/manifest/evidence.json" ]; then
    "$PY" -I -B "$HERE/evidence.py" "$WORK" "$REPEATS" partial || rc=1
  fi
  [ "$rc" = 0 ] || echo "incomplete/failed attempt retained: $WORK" >&2
  exit "$rc"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# t <seconds> <cmd...>: run with a wall-clock limit (no coreutils timeout on macOS); rc 142 = timed out.
t() { local s="$1"; shift; "$PERL" -e 'alarm shift; exec @ARGV or exit 127' "$s" "$@"; }

mkdir -p "$WORK/manifest"
{
  echo "date_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "prefix=$PREFIX"
  # doctor captures architecture/OS, component paths and tool availability, not env dumps.
  # Isolate it from inherited secrets; home paths in diagnostics are acceptable.
  echo "doctor_rc=$(t 120 /usr/bin/env -i HOME="$HOME" PATH="$PATH" LC_ALL=C "$CUMETAL" doctor >"$WORK/manifest/doctor.log" 2>&1 && echo 0 || echo $?)"
} >"$WORK/manifest/env.txt"
"$PY" -I -B "$HERE/host_manifest.py" "$WORK/manifest/host.json" --cumetal "$CUMETAL" --clang "$CLANGXX"
mkdir "$WORK/manifest/sources"
cp "$HERE/"*.h "$HERE/run_matrix.sh" "$HERE/classify.py" "$HERE/evidence.py" "$HERE/host_manifest.py" "$HERE/"*.cu "$WORK/manifest/sources/"
( cd "$WORK/manifest/sources" && /usr/bin/shasum -a 256 ./* ) >"$WORK/manifest/sha256.txt"

for spec in "${PROBES[@]}"; do
  p="${spec%%:*}"; lib=""; [ "$spec" != "$p" ] && lib="${spec#*:}"
  src="$WORK/manifest/sources/$p.cu"; d="$WORK/$p"; mkdir "$d"; mkdir "$BIN/$p"
  echo "[probe] $p" >&2
  if [ ! -f "$src" ]; then echo "missing" >"$d/missing"; continue; fi
  echo "$lib" >"$d/lib"

  # --- AOT ---
  rc=0; t "$COMPILE_TIMEOUT" "$CUMETALC" -I "$WORK/manifest/sources" "$src" -o "$BIN/$p/aot.bin" >"$d/aot.compile.log" 2>&1 || rc=$?
  echo "$rc" >"$d/aot.compile.rc"
  # --- JIT ---
  rc=0
  { t "$COMPILE_TIMEOUT" "$CLANGXX" -x cuda -std=c++17 -O2 --cuda-gpu-arch=sm_80 -nocudainc -nocudalib \
      -I"$PREFIX/include" -I"$WORK/manifest/sources" -include cuda_runtime.h -c "$src" -o "$BIN/$p/jit.o" \
    && t "$COMPILE_TIMEOUT" /usr/bin/xcrun clang++ "$BIN/$p/jit.o" -L"$PREFIX/lib" -lcumetal ${lib:+-l"$lib"} \
      -Wl,-rpath,"$PREFIX/lib" -o "$BIN/$p/jit.bin"; } >"$d/jit.compile.log" 2>&1 || rc=$?
  echo "$rc" >"$d/jit.compile.rc"

  for path in aot jit; do
    [ "$(cat "$d/$path.compile.rc")" = 0 ] && [ -x "$BIN/$p/$path.bin" ] || continue
    for r in $(seq 1 $REPEATS); do
      rc=0; CUMETAL_TRACE_GPU=1 t "$RUN_TIMEOUT" "$BIN/$p/$path.bin" >"$d/$path.run$r.out" 2>&1 || rc=$?
      echo "$rc" >"$d/$path.run$r.rc"
    done
  done
done

"$PY" -I -B "$HERE/evidence.py" "$WORK" "$REPEATS" complete
"$PY" -I -B "$HERE/classify.py" "$WORK" "$OUT_JSON" "$REPEATS" --initial
echo "wrote $OUT_JSON" >&2
