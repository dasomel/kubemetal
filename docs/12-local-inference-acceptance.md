# Local Inference Acceptance — Issue #58

This document is the execution gate for the code implemented in PR #59. Code/CI verification can
prove contracts and regressions; Apple Silicon/oMLX behavior, actual Metal pressure, network
isolation and benchmark values must be measured on the target Mac.

## 1. Static gate

```bash
make verify
```

The gate covers Rust tests, `clippy -D warnings`, TypeScript, DESIGN.md lint, IPC type checks,
license policy and web build.

## 2. Start the production runtime path

Use **MLX Studio → Local AI Runtime** to start oMLX. Do not start a separate shell-owned server for
the primary acceptance run: the test must exercise KubeMetal PID ownership, loopback binding,
restart behavior, logs and model controls.

Record the managed PID shown in the UI.

## 3. One-command acceptance harness

```bash
MODEL_A='<primary-model-id>' \
MODEL_B='<second-model-id>' \
RUNTIME_PID='<managed-pid>' \
RUN_BRIDGE=1 \
bash scripts/mlx/run_local_inference_acceptance.sh
```

Optional variables:

- `ENDPOINT=http://127.0.0.1:8000`
- `MODEL_DIR=$HOME/.omlx/models`
- `CACHE_DIR=$HOME/.omlx/cache`
- `CACHE_STATE=warm`
- `API_KEY=...` (environment only; never put it in evidence JSON or commit it)
- `EVIDENCE_DIR=evidence/local-inference/<run-id>`
- `SKIP_STATIC=1` only when `make verify` was already run against the exact same commit

The harness runs local-only readiness, streaming benchmark concurrency 1/2/4/8, optional two-model
load/switch/unload, and optional private bridge verification. It writes an evidence manifest and
individual JSON reports.

## 4. Cold / warm / SSD-restore evidence

A cache label is evidence metadata, not proof. Run the benchmark under conditions you physically
established:

```bash
python3 scripts/mlx/benchmark_local_inference.py \
  --runtime omlx \
  --endpoint http://127.0.0.1:8000 \
  --model '<model-id>' \
  --model-repo '<owner/repo>' \
  --model-revision '<immutable-revision>' \
  --model-digest '<digest-if-known>' \
  --quantization '<quantization>' \
  --runtime-pid '<pid>' \
  --cache-dir "$HOME/.omlx/cache" \
  --cache-state cold \
  --requests 8 --concurrency 1 --stream \
  --output evidence/local-inference/cold.json
```

Repeat for `warm` and, after an actual runtime restart with the SSD cache retained, `ssd-restore`.
Do not label a run `ssd-restore` merely because the directory existed. Confirm cache reuse from the
runtime log/diagnostics as well.

The benchmark schema records model identity, runtime, host, traffic, TTFT, API-boundary prefill
estimate, generation tok/s, process RSS and cache-directory size. Queue wait, engine-only model-load
time and peak unified-memory stay `null` unless a trustworthy source exists.

## 5. Regression gate

Compare two evidence files with explicit independent SLO thresholds:

```bash
python3 scripts/mlx/compare_local_inference_benchmarks.py \
  evidence/local-inference/baseline.json \
  evidence/local-inference/candidate.json \
  --max-latency-regression-pct 15 \
  --max-throughput-regression-pct 15 \
  --output evidence/local-inference/regression.json
```

Performance does **not** stand in for LLM output quality. RCA/RAG/citation/safety quality remains a
separate #21 evaluation signal.

## 6. API compatibility and client profiles

Use **MLX Studio → Runtime Evidence**:

1. Probe OpenAI chat, Anthropic messages, embeddings and rerank routes.
2. A 4xx model/auth/validation response may prove a route exists; 404/405 means absent.
3. Generate a client connection profile. The generated profile contains placeholders only and
   never the session API key.
4. The host endpoint must be loopback. K3s clients use only the private bridge endpoint.

## 7. Memory admission and diagnostics

Every oMLX model load through KubeMetal goes through host preflight. The backend checks macOS memory
pressure, thermal state, concurrent MLX fine-tuning and the Metal wired-memory limit when an
estimated model size is available. Critical memory pressure or serious/critical thermal state
denies the load; uncertain/elevated conditions are warnings.

Runtime diagnostics inspect retained KubeMetal oMLX logs for known Metal OOM, long-context prefill
guard, Metal-cap, tiered-cache and eviction evidence. Missing telemetry is reported as unavailable,
never healthy zero.

## 8. KV cache lifecycle

KubeMetal treats KV/prefix cache as disposable performance state:

- inspection and cleanup are HOME-confined;
- symbolic links are never followed;
- cleanup accepts only paths explicitly named as cache/KV-cache;
- dry-run is the default;
- the UI requires a preview before destructive cleanup;
- cleanup is disabled while the KubeMetal-managed runtime is running;
- runtime/model revision/digest/quantization/cache-format identity changes must be treated as cold
  unless compatibility is explicitly established.

The backend command `evaluate_local_inference_cache_compatibility` compares those identity fields;
`compatible=true` means only that reuse **may be benchmarked**, not that a cache hit occurred.

## 9. Air-gap acceptance

Readiness check:

```bash
RUNTIME=omlx \
ENDPOINT=http://127.0.0.1:8000 \
MODEL_DIR="$HOME/.omlx/models" \
bash scripts/mlx/verify_local_inference_airgap.sh
```

Final acceptance requires repeating local inference with external networking actually disabled.
The readiness script deliberately cannot claim that physical condition on its own.

## 10. Private K3s/Colima bridge

Start the private bridge in MLX Studio, then:

```bash
TARGET_PORT=8000 \
BRIDGE_HOST='<private-host-address>' \
BRIDGE_PORT=18000 \
bash scripts/mlx/verify_local_inference_bridge.sh
```

The host runtime remains bound to `127.0.0.1`. The KubeMetal relay is restricted to an explicit
private/loopback/link-local bind address and forwards to the loopback runtime. Public/wildcard bind
addresses are rejected.

## 11. Completion rule

Close #58 only after all code/CI gates are green **and** evidence exists for the selected target Mac:
managed start/stop/restart, two-model operations, API compatibility, private bridge, cold/warm/SSD
cache behavior, concurrency 1/2/4/8, Metal/memory/thermal behavior, wrong-key/occupied-port/public-
bind negative cases, and actual external-network-disconnected inference.

## 12. Run record — 2026-09-06

Target Mac: Apple M4 Pro, 64 GB, macOS 26, oMLX 0.6.4 (Homebrew), colima (vz) running. Packaged app
built from this branch via `make app` at commit `3f94200` plus the uncommitted bridge-default/API-key
UI fixes on top. Evidence: `evidence/local-inference/20260906-ondevice/` (`acceptance.log`,
`airgap-readiness.txt`, `bridge.txt`, `manifest.json`, `multi-model.json`,
`omlx-cold-c{1,2,4,8}.json`).

- **Managed runtime**: started from MLX Studio → Local AI Runtime, managed PID 53467, argv `omlx
  serve --host 127.0.0.1 --port 8000 --model-dir ~/.kubemetal/models --paged-ssd-cache-dir
  ~/.omlx/cache --paged-ssd-cache-max-size 20GB --hot-cache-max-size 4GB
  --max-concurrent-requests 4 --memory-guard balanced`; `/health` healthy. 6 models discovered from
  `~/.kubemetal/models` once the API key was entered in the card (admin cookie session works). Load
  + Pin of `mlx-community/Qwen2.5-0.5B-Instruct-4bit` reflected as Loaded/Pinned, actual size 457.8 MB.
- **Private bridge**: started at `127.0.0.1:18000 → 127.0.0.1:8000`. From inside the colima VM,
  `curl http://host.lima.internal:18000/health` and `http://host.lima.internal:8000/health` both
  returned 200 (`bridge.txt`: "Colima VM → KubeMetal private bridge → oMLX: OK").
- **Air-gap readiness**: `verify_local_inference_airgap.sh` PASS (local-only probes only; the
  external-network-disconnected repeat was **not** done).
- **Benchmark matrix**, model `Qwen2.5-0.5B-Instruct-4bit`, 8 streaming requests per concurrency,
  cache label `cold` (label only — the model had already been loaded once before the matrix, so
  treat as warm-ish; an `ssd-restore` run was not done):

  | Concurrency | TTFT p50 | Gen tok/s p50 | Latency p50 |
  |---|---|---|---|
  | 1 | 1.5 ms | 327 tok/s | 307 ms |
  | 2 | 2.1 ms | 195 tok/s | 506 ms |
  | 4 | 5.2 ms | 144 tok/s | 746 ms |
  | 8 | 10.2 ms | 100 tok/s | 1207 ms |

  0 errors across 32 requests; runtime RSS after the c8 run was 733 MB.
- **Multi-model harness** (0.5B ↔ 7B-Instruct-4bit, 2 cycles, 1 chat round each): `success=true`,
  14 events, 7B load 2675 ms cold then 800 ms on the second cycle, no errors.
- **Stop**: "Stop managed oMLX" sent SIGTERM to PID 53467 only; `omlx.log` shows a clean "Engine
  pool shutdown … Finished server process [53467]"; port 8000 released; no separate exit-code
  evidence was recorded (user-initiated stop, by design). Quitting the app closed the relay port
  18000.
- **Not verified in this run**: air-gapped (network actually disconnected) repeat, warm/ssd-restore
  cache runs, external-cluster L2 bridge, the mlx-lm fallback path via this UI.
