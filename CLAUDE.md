# KubeMetal — Claude Code Guide

> Apple Silicon-only hybrid MLOps desktop app: K8s (Colima/vz) control plane + macOS host
> MLX compute, in a Tauri v2 (Rust) + React/TypeScript app.

**Control/Compute split is a hard invariant, not a preference**: MLflow/SeaweedFS run as
pods in a Colima (`vz`+`virtiofs`) K3s VM; all ML computation runs as macOS host
processes — Metal GPU cannot be passed through to Linux VMs.

> Working procedure: global `<procedural_completion>` doctrine (`~/.claude/CLAUDE.md`)
> on substantive tasks; trivial one-shots answer directly.

## Source Map — read the file that owns the topic, don't duplicate it here

| Topic | Canonical file |
|-------|----------------|
| Proposal / roadmap | `docs/01-proposal.md` |
| FR/NFR + IPC command table | `docs/02-requirements.md` (§4.1 = IPC names) |
| MVP design + **decision registry D1–D12** | `docs/03-mvp-design.md` (§4 registry, §5 verified/unverified assumptions) |
| Architecture overview | `docs/04-architecture.md` |
| **Mistakes Log** | `docs/mistakes-log.md` — read the section matching your work area BEFORE touching it; add a row per new mistake |
| Team harness / lanes / OMC / plan-mode | `.claude/rules/harness.md` — load when orchestrating |
| UI tokens + design rules | root `DESIGN.md` (Google Labs design.md standard) |
| Run instructions | `README.md` |
| Superseded drafts | `docs/archive/` — never implement from these |

Changing a D1–D12 decision requires updating all affected docs in the same task.

## Architecture Invariants

- **K8s never runs compute** — MLX/Metal work is host processes spawned by the Rust backend.
- **Ports (D1)**: MLflow host-forward **5001** (AirPlay owns 5000), SeaweedFS S3 8333,
  Filer UI 8888, Prefect 4200, model serving 8080 (serving URLs always `127.0.0.1`,
  never `localhost`). Object storage is SeaweedFS; orchestration is Prefect 3 (D19).
- **Metrics (D2, amended)**: sysinfo RAM/CPU + sudo-free GPU via `ioreg -c IOAccelerator`
  (through `external_command`). `powermetrics`/sudo/root paths remain forbidden without
  a privileged helper.
- **VM sizing derived from detected RAM (D4)**: 16GB→4GB/2CPU, 32–48GB→8GB/4CPU,
  64GB+→12GB/6CPU — never hardcoded, backend clamps frontend input.
- **Pod→host bridge (D10)**: ExternalName `mac-gpu-service` (ns `default`) →
  `host.lima.internal`, no `ports` field. Verified on-device 2026-07-20
  (CoreDNS → 192.168.5.2); never `host.docker.internal`.

## Team & UI (summaries — detail in owning files)

- Substantive work runs as lanes (agy-first, max 5 concurrent, disjoint file scopes,
  authoring ≠ verification). Full rules: `.claude/rules/harness.md`.
- UI: `DESIGN.md` frontmatter is the only token source, mapped 1:1 into
  `tailwind.config.js`; no raw hex / default-palette classes in components;
  `npx @google/design.md lint DESIGN.md` must exit 0 after token changes; UI work is
  done only after visual confirmation in the running app or vite preview.

## Development Commands

```bash
pnpm tauri dev                                        # run app (colima required for cluster features)
pnpm tauri build                                      # .app/.dmg
cargo check  --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
npx tsc --noEmit
colima status --json                                  # cluster ground truth
npx @google/design.md lint DESIGN.md                  # design token gate
```

Type-check passing ≠ feature working: verify UI/IPC in the running app, cluster changes
against real colima (`kubectl --context colima get pods -n default`).

## Permissions

- Allowed: `src/`, `src-tauri/`, `scripts/`, `docs/`, `DESIGN.md`, root configs;
  new deps when justified.
- Forbidden: blocking calls in async commands; `alert()`; shell-PATH assumptions
  (use `resolve_cli_path`); sudo/`powermetrics` before Phase 3; hardcoded ports/VM
  sizes/CLI paths; secrets in source; starting Colima above the D4 profile;
  implementing from `docs/archive/`.

## Style & Commits

- Rust: rustfmt defaults, edition 2021, clippy-clean. TS/YAML/JSON/TOML: 2-space;
  imports external → internal → relative. Files >300 lines: split.
- Conventional Commits, scoped per task, commit after each verified task.
  **LOCAL commits only** — no push/remote unless explicitly asked.

## Resource Safety

- One cluster lifecycle op at a time (colima is not reentrant); VM allocation follows D4.
- After cluster changes verify as the user: `colima status --json`, pods, curl 5001/8333/8888.
- Port-forwards are process-bound — say which process owns a forward when reporting
  URLs as reachable.

## Changelog (newest first; keep ≤10 rows, fold older into a summary row)

| Date | Change |
|------|--------|
| 2026-07-24 | Audit of 20 takeover-session commits (Phase 4b~5a): fake DAG wiring, clippy/design/docs gates skipped, ioreg PATH regression, SSRF gap — fixes + D2 amended (sudo-free ioreg GPU), 3 process lessons logged |
| 2026-07-20 | Slimmed guide: Mistakes Log → `docs/mistakes-log.md`, harness detail → `.claude/rules/harness.md` (user directive: keep CLAUDE.md small) |
| 2026-07-20 | Design source → root `DESIGN.md` (Google standard, lint gate); visual reset to graphite "precision instrument"; endpoint links must use opener plugin |
| 2026-07-20 | Runtime verification pass: colima JSON schema corrected, tauri dev/plugin config fixed, D10 bridge + SeaweedFS verified on-device |
| 2026-07-20 | MinIO → SeaweedFS (D1 amended: 8333/8888); team harness (agy-first) + OMC policy added; MVP implemented via agy lanes + QA |
| 2026-07-20 | Initial guide (idp-style), docs v0.2 baseline, decision registry D1–D12, git repo initialized |
