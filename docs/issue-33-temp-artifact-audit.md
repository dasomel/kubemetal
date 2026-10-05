# Issue #33 temporary artifact audit (2026-10-04)

## Plan and stopping condition

Goal: find an existing app-owned temporary artifact class before implementing TTL,
leases, disk admission, and confirmed deletion. Inspect producers and the existing
checkpoint safety helpers; implement only if ownership, temporary classification,
and workload protection can be established without guessing. Non-goals: verified
checkpoint eviction, shared caches, ownership/isolation, lineage, replay, RAG resume,
VM/cluster operations, and remote GitHub mutations. Verification here is source
inspection and `git diff --check`; runtime verification is not claimed.

Result: STOP under the caller's explicit prerequisite. Temporary artifacts exist,
but none of the classes below is currently eligible for safe TTL deletion using
the existing lifecycle/session evidence. This is not a claim that temporary
artifacts do not exist or that a future producer migration is impossible.

## Producer evidence

| Class | Source | Why not an eligible TTL candidate today |
|---|---|---|
| Partial fine-tune output | `scripts/mlx/finetune_wrapper.py:78-79,156-164` | Writes directly to the final named adapter directory with `exist_ok=True`; failure leaves output, but there is no durable temporary/failed classification. Reusing a name can mix existing and new output. |
| Failed model download output | `src-tauri/src/commands/modelhub.rs:320-353,376-401` | Creates final model directories and downloads directly to final file paths. Download state is in memory; no per-attempt staging directory or persisted completion/lease evidence distinguishes leftovers from usable models after restart. |
| Evaluation scratch | `scripts/prefect/host_runner.py:172,193-213` | `mkdtemp` in system temp, deleted in `finally`; crash leftovers are possible. No persistent app-owned parent inventory or lease linking these directories to active Prefect evaluations. Prefix matching in shared temp is insufficient authorization. |
| RAG lexical scratch | `scripts/rag/rag_host.py:142-144` | `TemporaryDirectory` in system temp; normal exit cleans up. No existing lifecycle marker links a leftover directory to an active lexical query. |
| Marker publication scratch | `src-tauri/src/services/mlx_lifecycle/marker.rs:124-154` | Real `.tmp-*` files inside the marker directory, but these are process identity state, not model artifacts. Creation is not serialized by adapter admission; treating them as disposable artifacts would alter the lifecycle safety mechanism. |

No app conversion/quantization output producer was found in the inspected command
and MLX script paths. The inspected MLX launch paths do not establish exclusive
ownership of Hugging Face's shared cache; it must remain outside this slice.

## Safety evidence and next owner decision

`services/mlx_artifacts.rs:20-29` explicitly says a missing manifest may be an old
artifact or failed training. Moreover, `commands/mlx.rs:901-913` permits successful
training without a written manifest when reporting or manifest writing fails.
Thus missing/corrupt manifest plus age cannot prove disposable output.

`commands/mlx.rs:353-453` already provides component lstat checks, normalized paths,
active/LKG protection and admission-serialized deletion; retain it unchanged.
`services/mlx_lifecycle/marker.rs:104-125` records kind/PID, not artifact paths or
Prefect/RAG leases. `commands/mlx.rs:132-147` holds adapter references in memory.

Owner decision needed: choose a producer to migrate to an exclusively app-owned,
per-attempt staging directory with durable completion and lease evidence. Specify
promotion/recovery compatibility for existing final directories; legacy unknown
directories must remain protected. Then implement the requested dry-run scan,
confirmed revalidation, and disk HOLD. `libc` already exists in Cargo.toml, so a
dependency-free statvfs implementation is available; it is not the blocker.

No runtime code, IPC, UI, D-registry, or mistakes-log row changed. D44 remains the
requested number for a future implemented policy (D43 reserved by caller for
another PR); no TTL value or decision was invented. No Rust/TS tests, trace gates,
or app/MLX/cluster verification ran because implementation stopped before changes
to those paths. The four requested acceptance criteria remain unimplemented.

## S2 implementation update (2026-10-06)

The producer defect above is historical evidence. `run_mlx_finetune` now uses the
S1 staging service: `~/.kubemetal/adapter-staging/<attempt_id>/out/`, required
wrapper `--output-dir`, marker PID/start-time identity, and exit/done-path/sha256
verification before admission-serialized exclusive promotion to `adapters/<name>`.
Existing final names (including legacy directories without manifests) are refused
and untouched. Failed/killed attempts remain staged; only a newly created empty
attempt is removed after spawn failure using rmdir. D45 records the contract;
D44 remains no signing. No TTL, startup reconcile, list IPC, warm-start resume or
checkpoint events are implemented in S2. The independent Prefect fine-tune flow
still omits the new required argument and consequently fails closed; its migration
needs separate Rust lifecycle integration. No real MLX training, real user artifact
directory or app UI observation is claimed by this update.
