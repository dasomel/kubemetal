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
  sgemm_handwritten cublas_sgemm:cublas cufft_c2c:cufft curand_host:curand nan_inf)
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
( cd "$HERE" && /usr/bin/shasum -a 256 common.h run_matrix.sh ./*.cu ) >"$WORK/manifest/sha256.txt" || true

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

cat >"$WORK/classify.py" <<'EOP'
import sys, os, re, json, collections
work, out_json, repeats = sys.argv[1], sys.argv[2], int(sys.argv[3])

def rd(p, default=""):
    try:
        with open(p, errors="replace") as f: return f.read()
    except OSError: return default

env = dict(l.split("=", 1) for l in rd(f"{work}/manifest/env.txt").splitlines() if "=" in l)
sha = {}
for l in rd(f"{work}/manifest/sha256.txt").splitlines():
    h, _, n = l.partition("  "); sha[n.lstrip("./")] = h
manifest = dict(env); manifest["probe_sha256"] = sha

UNSUP = re.compile(r"unsupported|not supported|not implemented|NOT_SUPPORTED|NOT_IMPLEMENTED|cannot (lower|compile)", re.I)
PROV = re.compile(r"CUMETAL_PROVENANCE\s+(.*)")
KV = re.compile(r'(\w+)=("[^"]*"|\S+)')

def provenance(text):
    ev = collections.Counter(); dev = set(); src = set(); sq = set(); ok = set(); reasons = set(); names = set()
    n_launch = 0
    for m in PROV.finditer(text):
        kv = {k: v.strip('"') for k, v in KV.findall(m.group(1))}
        ev[kv.get("event", "?")] += 1
        if kv.get("event") == "kernel_launch":
            n_launch += 1
            dev.add(kv.get("device", "?")); src.add(kv.get("source", "?")); sq.add(kv.get("semantic_quality", "?"))
            ok.add(kv.get("launch_success", "?"))
            if kv.get("unsupported_reason"): reasons.add(kv["unsupported_reason"])
            if kv.get("device_name"): names.add(kv["device_name"])
    return {"events": dict(ev), "kernel_launches": n_launch, "devices": sorted(dev), "sources": sorted(src),
            "semantic_quality": sorted(sq), "launch_success": sorted(ok), "unsupported_reasons": sorted(reasons),
            "device_names": sorted(names)}

def tail(s, n=400):
    s = s.strip(); return s[-n:]

def classify_path(d, path, kernel_probe):
    r = {"compile": None, "compile_rc": None, "runs": [], "parity": None, "provenance": None, "stable": None,
         "class": "unverified", "caveats": []}
    crc = rd(f"{d}/{path}.compile.rc").strip()
    if crc == "":
        r["compile"] = "not_attempted"; return r
    crc = int(crc); r["compile_rc"] = crc
    clog = rd(f"{d}/{path}.compile.log")
    if crc == 0:
        r["compile"] = "ok"
    else:
        r["compile"] = "timeout" if crc == 142 else "refused"
        r["compile_error"] = tail(clog)
        r["class"] = "failed" if crc == 142 else "unavailable"
        r["caveats"].append("compile " + ("timed out" if crc == 142 else "refused"))
        return r
    digests, alltext, rcs, parities = [], "", [], []
    for i in range(1, repeats + 1):
        rc = rd(f"{d}/{path}.run{i}.rc").strip(); out = rd(f"{d}/{path}.run{i}.out")
        rc = int(rc) if rc else -1; rcs.append(rc); alltext += out
        m = re.search(r"^MATRIX_RESULT .*$", out, re.M)
        line = m.group(0) if m else None
        dg = re.search(r"digest=(\S+)", line).group(1) if line else None
        st = re.search(r"status=(\S+)", line).group(1) if line else None
        parities.append(line); digests.append(dg)
        r["runs"].append({"exit": rc, "status": st, "parity_line": line, "error_tail": tail(out) if rc != 0 else None})
    r["parity"] = parities[0]
    r["provenance"] = provenance(rd(f"{d}/{path}.run1.out"))
    prov_all = provenance(alltext)
    if rcs and all(c == 0 for c in rcs) and all(x["status"] == "PASS" for x in r["runs"]):
        # every run passed its own tolerance check, so differing bits are "within tolerance", not a failure
        r["stable"] = True
        r["stability"] = "tolerance_only" if "na" in digests else ("bit_identical" if len(set(digests)) == 1 else "within_tolerance")
        if r["stability"] == "within_tolerance":
            r["stability_note"] = "output bits differed across runs (%d distinct digests / %d runs) but every run passed tolerance" % (len(set(digests)), len(digests))
    else:
        r["stable"] = False
    # --- classification (fail closed) ---
    if any(c == 142 for c in rcs):
        r["class"] = "failed"; r["caveats"].append("run timed out"); return r
    if any(c == 1 for c in rcs) or any(x["status"] == "MISMATCH" for x in r["runs"]):
        r["class"] = "failed"; r["caveats"].append("numeric mismatch"); return r
    if any(c >= 20 and c < 128 for c in rcs):
        txt = alltext
        r["class"] = "unavailable" if UNSUP.search(txt) else "failed"
        r["caveats"].append("API failure (exit %s)" % sorted(set(rcs)))
        return r
    if any(c != 0 for c in rcs):
        r["class"] = "failed"; r["caveats"].append("crash/abnormal exit %s" % sorted(set(rcs))); return r
    if not all(x["status"] == "PASS" for x in r["runs"]):
        r["class"] = "failed"; r["caveats"].append("no parity line"); return r
    cav = []
    if r.get("stability_note"): cav.append(r["stability_note"])
    if kernel_probe:
        if prov_all["kernel_launches"] == 0:
            cav.append("no kernel_launch provenance although kernels were launched")
        else:
            if set(prov_all["devices"]) != {"apple_gpu"}: cav.append("device not apple_gpu: %s" % prov_all["devices"])
            if set(prov_all["launch_success"]) != {"true"}: cav.append("launch_success not always true")
            if prov_all["unsupported_reasons"]: cav.append("unsupported_reason: %s" % prov_all["unsupported_reasons"])
            if set(prov_all["semantic_quality"]) - {"exact"}: cav.append("semantic_quality %s" % prov_all["semantic_quality"])
    else:
        if prov_all["kernel_launches"] == 0:
            cav.append("library call: numeric parity only; CuMetal emitted no kernel_launch provenance, GPU execution not independently proven")
    r["caveats"] += cav
    # bit-level nondeterminism within tolerance is reported but does not downgrade by itself
    blocking = [c for c in cav if c != r.get("stability_note")]
    r["class"] = "supported" if not blocking else "partial"
    return r

# Probes that launch __global__ kernels from user code (library probes launch none themselves).
LIB_PROBES = {"cublas_sgemm", "cufft_c2c", "curand_host"}
rows = []
for name in sorted(os.listdir(work)):
    d = f"{work}/{name}"
    if name == "manifest" or not os.path.isdir(d): continue
    row = {"probe": name, "lib": rd(f"{d}/lib").strip() or None}
    if os.path.exists(f"{d}/missing"):
        row.update({"overall": "unverified", "note": "probe source missing"}); rows.append(row); continue
    kp = name not in LIB_PROBES
    row["aot"] = classify_path(d, "aot", kp); row["jit"] = classify_path(d, "jit", kp)
    a, j = row["aot"]["class"], row["jit"]["class"]
    if a == j: ov = a
    elif "failed" in (a, j): ov = "failed"
    elif "unverified" in (a, j): ov = "partial" if "supported" in (a, j) else "unverified"
    else: ov = "partial"
    row["overall"] = ov
    notes = []
    for p in ("aot", "jit"):
        for c in row[p]["caveats"]: notes.append(f"{p}: {c}")
    row["note"] = "; ".join(notes)
    rows.append(row)

order = ["launch_1d","launch_2d","launch_3d","shared_sync","atomics","warp_shuffle","stream_event","managed_memory","fp64",
         "sgemm_handwritten","cublas_sgemm","cufft_c2c","curand_host","nan_inf"]
rows.sort(key=lambda r: order.index(r["probe"]) if r["probe"] in order else 99)
doc = {"schema": "cumetal-matrix/1", "issue": 84, "manifest": manifest, "repeats": repeats,
       "vocabulary": ["supported", "partial", "unavailable", "failed", "unverified"], "results": rows,
       "summary": dict(collections.Counter(r["overall"] for r in rows))}
with open(out_json, "w") as f: json.dump(doc, f, indent=2, sort_keys=True); f.write("\n")

md = out_json[:-5] + ".md" if out_json.endswith(".json") else out_json + ".md"
def cell(p):
    c = p["class"]; st = {"bit_identical": "bit-identical", "within_tolerance": "tolerance-stable", "tolerance_only": "tolerance-stable"}.get(p.get("stability"), "unstable" if p.get("stable") is False else "-")
    return f"{c} ({st})" if p.get("compile") == "ok" else f"{c} (compile {p.get('compile')})"
lines = ["# CuMetal compatibility matrix", "",
         f"- {manifest.get('cumetal_version')} | {manifest.get('llvm_clang')} | {manifest.get('metal_version')} | {manifest.get('hw_model')}",
         f"- date_utc {manifest.get('date_utc')} | repeats {repeats} | summary {doc['summary']}", "",
         "| probe | AOT | JIT | overall | note |", "|---|---|---|---|---|"]
for r in rows:
    if "aot" not in r: lines.append(f"| {r['probe']} | - | - | {r['overall']} | {r['note']} |"); continue
    lines.append(f"| {r['probe']} | {cell(r['aot'])} | {cell(r['jit'])} | **{r['overall']}** | {r['note'].replace('|','/')} |")
with open(md, "w") as f: f.write("\n".join(lines) + "\n")
EOP

"$PY" -I "$WORK/classify.py" "$WORK" "$OUT_JSON" "$REPEATS"
echo "wrote $OUT_JSON" >&2
