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

WORK="$(mktemp -d "${TMPDIR:-/tmp}/cumetal-matrix.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

# t <seconds> <cmd...>: run with a wall-clock limit (no coreutils timeout on macOS); rc 142 = timed out.
t() { local s="$1"; shift; "$PERL" -e 'alarm shift; exec @ARGV or exit 127' "$s" "$@"; }

mkdir -p "$WORK/manifest"
{
  echo "date_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "prefix=$PREFIX"
  echo "cumetal_version=$("$CUMETAL" --version 2>&1 | head -1 || true)"
  echo "llvm_clang=$("$CLANGXX" --version 2>&1 | head -1 || true)"
  echo "xcrun_clang=$(/usr/bin/xcrun clang++ --version 2>&1 | head -1 || true)"
  echo "sw_vers=$(/usr/bin/sw_vers 2>&1 | tr '\n\t' ';;' || true)"
  echo "metal_version=$(/usr/bin/xcrun metal --version 2>&1 | head -1 || true)"
  echo "hw_model=$(/usr/sbin/sysctl -n machdep.cpu.brand_string 2>&1 || true)"
  echo "doctor_rc=$(t 120 "$CUMETAL" doctor >"$WORK/manifest/doctor.log" 2>&1 && echo 0 || echo $?)"
} >"$WORK/manifest/env.txt"
( cd "$HERE" && /usr/bin/shasum -a 256 common.h run_matrix.sh classify.py ./*.cu ) >"$WORK/manifest/sha256.txt" || true

for spec in "${PROBES[@]}"; do
  p="${spec%%:*}"; lib=""; [ "$spec" != "$p" ] && lib="${spec#*:}"
  src="$HERE/$p.cu"; d="$WORK/$p"; mkdir -p "$d"
  echo "[probe] $p" >&2
  if [ ! -f "$src" ]; then echo "missing" >"$d/missing"; continue; fi
  echo "$lib" >"$d/lib"

  # --- AOT ---
  rc=0; t "$COMPILE_TIMEOUT" "$CUMETALC" -I "$HERE" "$src" -o "$d/aot.bin" >"$d/aot.compile.log" 2>&1 || rc=$?
  echo "$rc" >"$d/aot.compile.rc"
  # --- JIT ---
  rc=0
  { t "$COMPILE_TIMEOUT" "$CLANGXX" -x cuda -std=c++17 -O2 --cuda-gpu-arch=sm_80 -nocudainc -nocudalib \
      -I"$PREFIX/include" -I"$HERE" -include cuda_runtime.h -c "$src" -o "$d/jit.o" \
    && t "$COMPILE_TIMEOUT" /usr/bin/xcrun clang++ "$d/jit.o" -L"$PREFIX/lib" -lcumetal ${lib:+-l"$lib"} \
      -Wl,-rpath,"$PREFIX/lib" -o "$d/jit.bin"; } >"$d/jit.compile.log" 2>&1 || rc=$?
  echo "$rc" >"$d/jit.compile.rc"

  for path in aot jit; do
    [ "$(cat "$d/$path.compile.rc")" = 0 ] && [ -x "$d/$path.bin" ] || continue
    for r in $(seq 1 $REPEATS); do
      rc=0; CUMETAL_TRACE_GPU=1 t "$RUN_TIMEOUT" "$d/$path.bin" >"$d/$path.run$r.out" 2>&1 || rc=$?
      echo "$rc" >"$d/$path.run$r.rc"
    done
  done
done

"$PY" -I -B "$HERE/classify.py" "$WORK" "$OUT_JSON" "$REPEATS"
echo "wrote $OUT_JSON" >&2
