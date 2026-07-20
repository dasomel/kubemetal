# KubeMetal — Claude Code Guide

> Apple Silicon-only hybrid MLOps desktop app: K8s (Colima/vz) control plane + macOS host
> MLX compute, unified in a Tauri v2 (Rust) + React/TypeScript desktop app.

## Quick Overview

KubeMetal splits **control** and **compute**: MLflow/MinIO(/Prefect) run as pods inside a
Colima (`vz` + `virtiofs`) K3s VM, while all ML computation (MLX LoRA fine-tuning, serving)
runs natively on the macOS host — Metal GPU cannot be passed through to Linux VMs, so this
split is a hard architectural invariant, not a preference.

> **Working procedure:** follow the global `<procedural_completion>` doctrine
> (`~/.claude/CLAUDE.md`) on substantive tasks — goal → decompose → execute → verify → risk
> (five principles + completion gate + escalation). Trivial one-shots answer directly.

## Document Map (source of truth)

| Doc | Content |
|-----|---------|
| `docs/01-proposal.md` | Project proposal v0.2 — problem, architecture, tech stack, roadmap |
| `docs/02-requirements.md` | OSS research + FR/NFR spec v0.2 — IPC command table, manifest spec |
| `docs/03-mvp-design.md` | Phase 1 MVP design — directory layout, Rust/TS reference code, §4 decision registry |
| `docs/archive/` | Superseded first drafts (init/spec/arch.md) — do not implement from these |

**Decision registry (D1–D10)** lives in `docs/03-mvp-design.md` §4 and is canonical.
Code must follow it. Changing a decision requires updating ALL three docs in the same task,
plus a Mistakes Log entry if the change was driven by a defect.

## Architecture Invariants

- **K8s never runs compute.** Anything touching MLX/Metal executes as a host process
  spawned by the Rust backend — never inside a pod.
- **Canonical ports (D1):** MLflow host-forward **5001** (5000 is taken by macOS AirPlay
  Receiver), MinIO S3 API 9000, MinIO Console 9001, model serving 8080 (`/v1`, configurable).
- **Phase 1 metrics = sysinfo RAM/CPU only (D2).** No `powermetrics`, no sudo, no GPU
  metrics until the Phase 3 privileged-helper work. Never add root-requiring code paths
  to the default app flow.
- **VM sizing is derived, not hardcoded (D4):** detected host RAM 16GB → VM 4GB,
  32–48GB → 8GB, 64GB+ → 12GB.
- **Pod → host bridge (D10):** `ExternalName` Service `mac-gpu-service` (namespace
  `default`) aliasing `host.lima.internal` — never `host.docker.internal` (Docker
  Desktop-only); no `ports` field (CNAME-only). CoreDNS resolution of
  `host.lima.internal` is an UNVERIFIED assumption — verify on real hardware before
  building on it.

## Plan Mode Guide

Use Plan mode for: new IPC commands or FR-level features, changes to the D1–D10 registry,
Colima lifecycle logic changes, anything touching the K8s↔host bridge or guardrails.
Skip it for: doc typos, UI styling, single-file refactors with tests.

## Mistakes Log (Compounding Engineering)

> Add a row whenever a mistake is made (including ones caught in review). Same mistake,
> never twice. Format: `| YYYY-MM-DD | Mistake | Fix |`

### Rust / Tauri
| Date | Mistake | Fix |
|------|---------|-----|
| 2026-07-20 | Parsed `colima status` by matching stdout strings — colima logs via logrus to **stderr**, so the match always fails; `vm_type` was also hardcoded | Use `colima status --json` and deserialize with serde |
| 2026-07-20 | `Command::new("colima")` assumed shell PATH — packaged .app launched from Finder does NOT inherit it, so Homebrew CLIs are not found | Resolve absolute paths (`/opt/homebrew/bin`, `/usr/local/bin`, …) via `resolve_cli_path` helper before spawning |
| 2026-07-20 | Blocking `std::process::Command::output()` inside an async `tauri::command` (`colima start` runs minutes) | Use `tokio::process::Command`; never block the async runtime |
| 2026-07-20 | Error UX via JS `alert()` — wry WebView does not implement alert/confirm/prompt | Use `@tauri-apps/plugin-dialog` (`message`/`ask`) + `dialog:default` capability |
| 2026-07-20 | Created `sysinfo::System::new_all()` on every metrics call | Hold one instance in `tauri::State<Mutex<System>>` and `refresh_*()` per call |

### macOS platform
| Date | Mistake | Fix |
|------|---------|-----|
| 2026-07-20 | Bound MLflow to host port 5000 — macOS AirPlay Receiver (ControlCenter) occupies 5000/7000, forwarding silently breaks | Use 5001 (D1); never claim 5000 on macOS |
| 2026-07-20 | Spec required "powermetrics C-Binding" for GPU metrics — powermetrics is a CLI (no public C API) and requires **root** | Phase 1 ships sysinfo RAM/CPU only; GPU metrics = Phase 3 privileged helper + CLI parsing (D2) |
| 2026-07-20 | OOM guard triggered on "available RAM < 10%" — macOS file cache keeps RAM near-full at all times, guaranteeing false triggers | Trigger on memory pressure levels (warn/critical) instead (D11) |

### Kubernetes / MLOps stack
| Date | Mistake | Fix |
|------|---------|-----|
| 2026-07-20 | `ExternalName` Service manifest declared `ports` — ExternalName is a DNS CNAME alias only, no port proxying | Omit `ports`; clients dial `host.lima.internal:8080` directly (D10) |
| 2026-07-20 | Referenced a nonexistent tool "mlx-serve" | The real tool is `mlx_lm.server` (mlx-lm package) or `llama-server` (D12) |
| 2026-07-20 | Guide cited decision IDs (D6/D8/D9) that didn't match the canonical registry in `docs/03-mvp-design.md` §4 — two decisions weren't even registered | Registry is canonical; cite it verbatim, register missing decisions (D11 memory pressure, D12 serving tool) instead of inventing numbers |
| 2026-07-20 | UI hardcoded "6 CPU / 12GB" VM default, contradicting the 16GB device profile | Auto-size from detected RAM per D4 profile table |

## Core Flows

- Frontend `src/` (React + TS + Tailwind) ↔ Tauri IPC ↔ `src-tauri/src/commands/`
  (colima / metrics / provision) → `src-tauri/src/services/` (process spawn, sysinfo).
- IPC command names are fixed by `docs/02-requirements.md` §4.1 (`get_system_metrics`,
  `get_cluster_status`, `start_cluster`, `stop_cluster`, `provision_mlops_stack`,
  `start_port_forward`, `stop_port_forward`, `run_mlx_finetune`, `kill_mlx_process`).
  Rust, TS types, and docs must stay in sync.
- K8s manifests live in `scripts/k8s/` (mlflow, minio, mac-gpu-bridge).

## Development Commands

```bash
pnpm tauri dev                      # run app (requires colima installed for cluster features)
pnpm tauri build                    # bundle .app/.dmg
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo fmt   --manifest-path src-tauri/Cargo.toml
npx tsc --noEmit                    # TS type check
colima status --json                # ground truth for cluster state while debugging
```

Type-check passing ≠ feature working: UI/IPC changes must be verified in the running app
(`pnpm tauri dev`), cluster changes against a real `colima` (`kubectl get pods`).

## Permissions

### Allowed
- Modify `src/`, `src-tauri/src/`, `scripts/`, `docs/`, `package.json`, `Cargo.toml`,
  `tauri.conf.json`, `capabilities/`.
- Add dependencies (pnpm / crates) when justified in the task.

### Forbidden
- No blocking process calls inside async commands; no `alert()`; no shell-PATH assumptions.
- No sudo / `powermetrics` / privileged-helper code before Phase 3 (D2).
- No hardcoded ports, VM sizes, or CLI paths — use the D1/D4 constants and `resolve_cli_path`.
- No secrets or tokens in source, docs, or `tauri.conf.json`.
- Never start Colima with memory above the D4 profile for the detected host RAM.
- Do not implement from `docs/archive/` drafts.

## Code Style

- **Rust**: rustfmt defaults (4-space), edition 2021, clippy-clean (`-D warnings`).
- **TS/TSX/YAML/JSON/TOML**: 2-space indentation; imports external → internal → relative.
- Files > 300 lines: split. UI text: no hardcoded user-facing strings once i18n lands.
- Conventional Commits, scoped to the files the task touched.

## Commit Policy (project guideline)

- **This directory is not yet a git repository** — run `git init` before Phase 1
  implementation starts; first commit = docs + this guide.
- Commit after every completed+verified task; don't batch unrelated tasks.
- **"Commit" means LOCAL commit only** — no `git push`, no remote creation unless the
  user explicitly asks.

## Resource Safety

- Before `start_cluster`, check host free memory; the VM allocation must follow D4.
- One cluster lifecycle operation at a time (start/stop/provision are serialized;
  `colima` itself is not reentrant).
- After cluster changes, verify from the user's perspective: `colima status --json`,
  `kubectl get pods -A`, curl the forwarded MLflow (5001) / MinIO (9001) endpoints.

## Changelog
| Date | Change | Reason |
|------|--------|--------|
| 2026-07-20 | Initial guide, modeled on `idp/` workspace guidelines (Mistakes Log, Permissions, local-commit policy); Mistakes Log seeded with 10 defects found in the draft-doc review | Establish project guidelines before Phase 1 implementation |
| 2026-07-20 | Aligned decision IDs to the canonical registry (ExternalName→D10, memory pressure→D11, serving tool→D12); bridge invariant now pins service name/namespace; added port-forward IPC commands | Independent plan review (critic) found 2 blockers: D-number collision + ExternalName triple mismatch |
