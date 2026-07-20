# Agent Team Harness (agy-first) — full rules

Summary lives in `CLAUDE.md`; this file is the detail. Load when orchestrating lanes.

## Lane table

| Lane | Scope (disjoint — do not cross) | Worker |
|------|--------------------------------|--------|
| `rust-backend` | `src-tauri/**`, `scripts/k8s/**`, `Cargo.toml`, `tauri.conf.json`, `capabilities/` | agy (primary) |
| `frontend` | `src/**`, `package.json`, `tsconfig.json`, `vite.config.ts`, `index.html` | agy (primary) |
| `ui-design` | `DESIGN.md`, `tailwind.config.js`, component styling | designer agent |
| `qa/verify` | `cargo check`/`clippy`, `tsc --noEmit`, DESIGN.md lint, doc↔code sync (D1–D12, IPC names) | native subagent (verifier / code-reviewer) |

## Rules

- agy invocations: `agy -p "<self-contained prompt>" --print-timeout 10m`, worker writes
  its summary to `.omc/logs/agy-<lane>-result.md`; stdout → `.omc/logs/agy-<lane>.log`
  (never into main context). Model rotation on quota exhaustion + failure ladder per
  global `<agy_cli>`.
- Max 5 concurrent workers (agy + subagents combined). Lanes own disjoint paths;
  worktree isolation if scopes must overlap. Never two lanes on the same files —
  on lane silence, one follow-up + timeout, stop before replacing.
- Every lane prompt cites `docs/02-requirements.md` §4.1 (IPC names) and
  `docs/03-mvp-design.md` §4 (D1–D12) as binding constraints.
- Authoring and verification are ALWAYS separate lanes — a lane's self-reported success
  is not evidence; the qa lane (or orchestrator) re-runs the checks.

## OMC plugin utilization

- Default lanes: `executor` (implementation), `critic` (plan/design review, opus),
  `verifier`/`code-reviewer` (qa), `designer` (UI).
- QA cycling: `ultraqa` for test→verify→fix loops; `verify` gates "done" claims.
- Debugging: `trace`/`tracer` for cross-module root-cause; `debugger` for build errors.
- Docs: `deepinit` at phase boundaries (hierarchical AGENTS.md), not per-commit.
- agy runtime option: `omc-teams` (tmux panes) when a lane needs live monitoring;
  detached `agy -p` + log files is the default.
- Autonomous modes (`autopilot`/`ralph`/`ultrawork`): keyword-triggered only.
- Knowledge: defects → `docs/mistakes-log.md` (canonical); broader learnings → wiki /
  project-memory.

## Plan Mode

Use for: new IPC commands or FR-level features, D1–D12 registry changes, Colima
lifecycle logic, K8s↔host bridge, guardrails. Skip for: doc typos, styling,
single-file refactors with tests.
