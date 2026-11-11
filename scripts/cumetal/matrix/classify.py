"""Classify retained Phase 2 evidence without inferring unobserved GPU execution."""
import collections
import json
import os
import re
import sys

def rd(p, default=""):
    try:
        with open(p, errors="replace") as f: return f.read()
    except OSError: return default

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

def classify_path(d, path, kernel_probe, repeats=3):
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
        fields = dict(KV.findall(line)) if line else {}
        dg = fields.get("digest")
        st = fields.get("status")
        if st == "PASS" and not dg:
            st = None
        parities.append(line); digests.append(dg)
        r["runs"].append({"exit": rc, "status": st, "parity_line": line, "provenance": provenance(out), "error_tail": tail(out) if rc != 0 else None})
    r["parity"] = parities[0]
    r["provenance"] = provenance(alltext)
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
    # D22: evidence must hold for each execution; pooling hides missing traces.
    # Cost: absent library traces stay partial. Reclassify from retained runs if
    # upstream supplies stronger evidence; no GPU activity is inferred from parity.
    for i, run in enumerate(r["runs"], 1):
        prov = run["provenance"]
        issues = []
        if prov["kernel_launches"] == 0:
            issues.append("no kernel_launch provenance" if kernel_probe else
                          "library call: numeric parity only; GPU execution not independently proven")
        else:
            if prov["devices"] != ["apple_gpu"]:
                issues.append("device not apple_gpu: %s" % prov["devices"])
            if prov["launch_success"] != ["true"]:
                issues.append("launch_success not always true")
            if prov["unsupported_reasons"]:
                issues.append("unsupported_reason: %s" % prov["unsupported_reasons"])
            if prov["semantic_quality"] != ["exact"]:
                issues.append("semantic_quality %s" % prov["semantic_quality"])
            if any(source in ("", "?") for source in prov["sources"]):
                issues.append("missing launch source")
        cav.extend("run %d: %s" % (i, issue) for issue in issues)
    r["caveats"] += cav
    # bit-level nondeterminism within tolerance is reported but does not downgrade by itself
    blocking = [c for c in cav if c != r.get("stability_note")]
    r["class"] = "supported" if not blocking else "partial"
    return r

def main(work, out_json, repeats):
    env = dict(l.split("=", 1) for l in rd(f"{work}/manifest/env.txt").splitlines() if "=" in l)
    sha = {}
    for l in rd(f"{work}/manifest/sha256.txt").splitlines():
        h, _, n = l.partition("  "); sha[n.lstrip("./")] = h
    manifest = dict(env); manifest["probe_sha256"] = sha

    # Probes that launch __global__ kernels from user code (library probes launch none themselves).
    LIB_PROBES = {"cublas_sgemm", "cufft_c2c", "curand_host", "cusparse_spmv"}
    rows = []
    for name in sorted(os.listdir(work)):
        d = f"{work}/{name}"
        if name == "manifest" or not os.path.isdir(d): continue
        row = {"probe": name, "lib": rd(f"{d}/lib").strip() or None}
        if os.path.exists(f"{d}/missing"):
            row.update({"overall": "unverified", "note": "probe source missing"}); rows.append(row); continue
        kp = name not in LIB_PROBES
        row["aot"] = classify_path(d, "aot", kp, repeats); row["jit"] = classify_path(d, "jit", kp, repeats)
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
             "sgemm_handwritten","cublas_sgemm","cufft_c2c","curand_host","cusparse_spmv","nan_inf"]
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


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], int(sys.argv[3]))
