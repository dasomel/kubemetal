# Agent Team Harness (agy-first) — full rules

Summary lives in `CLAUDE.md`; this file is the detail. Load when orchestrating lanes.
Model tiering, agy rotation, and the failure ladder are owned by the global
`<routing_doctrine>` / `<agy_cli>` (`~/.claude/CLAUDE.md`) — this file pins only the
**project-specific** lane → scope → worker → model mapping.

## Orchestrator vs workers

- The orchestrator is the session's top model (normally **Opus 5**, **Fable 5** when it
  drives the session). Its job is planning, D1–D24 adjudication, review, synthesis —
  **not authoring code**. Writing >20 lines of Rust/TS in main context is a misroute.
- **Team is the default for substantive work.** Direct orchestrator work only for:
  `CLAUDE.md`, `.claude/**`, `.omc/**` edits; ≤1-line patches at a known location;
  single-fact reads; one-off commands (`colima status --json`, `git log -1`).
- Always pass the model **alias** explicitly (`haiku`/`sonnet`/`opus`/`fable` — never a
  pinned id like `claude-opus-5`); agy lanes always pass `--model` explicitly.
- Tie-break is always the lower tier. Escalate one rung only on an **observed** failure,
  never in anticipation.

## Lane table

| Lane | Scope (disjoint — do not cross) | Worker | Model |
|------|--------------------------------|--------|-------|
| `rust-backend` | `src-tauri/**`, `scripts/**` (k8s manifests, mlx/prefect/ingest/airgap/e2e host scripts), `Cargo.toml`, `tauri.conf.json`, `capabilities/` | agy (primary) | `Gemini 3.6 Flash (High)` → `Claude Opus 4.6 (Thinking)` on quota exhaustion; native fallback `sonnet` |
| `frontend` | `src/**`, `package.json`, `tsconfig.json`, `vite.config.ts`, `index.html` | agy (primary) | same rotation; native fallback `sonnet` |
| `ui-design` | `DESIGN.md`, `tailwind.config.js`, component styling | `designer` subagent | `sonnet` |
| `qa/verify` | `cargo check`/`clippy`, `tsc --noEmit`, DESIGN.md lint, doc↔code sync (D1–D24, IPC names) | `verifier` / `code-reviewer` subagent | `sonnet` (1st pass) |
| `approve` | final approval pass on high-risk diffs — D1–D24 registry changes, colima lifecycle, guardrails (D11/D16/D17), credential & SSRF paths (D21), K8s↔host bridge (D10) | `critic` / `code-reviewer` subagent | `opus` |
| `escalate` | only after an `opus` lane produced a demonstrably wrong/insufficient result on the hardest reasoning step | subagent | `fable` (rare) |

Re-tiering note: lanes this harness once pinned to `opus` (code/YAML authoring, manifest
normalization, 1st-pass review, doc drafts) belong on `sonnet`. Keep `opus` for the
`approve` lane and cross-cutting judgment only — an all-`opus` harness is misrouted,
not thorough.

## Rules

- agy invocations: `agy --model "<model>" -p "<self-contained prompt>" --print-timeout 10m`,
  worker writes its summary to `.omc/logs/agy-<lane>-result.md`; stdout →
  `.omc/logs/agy-<lane>.log` (never into main context). Rotate models only on a genuine
  quota/limit signal; on ordinary failure follow the global fallback ladder (other model →
  native subagent → log cause in `.omc/logs/agy-fallback.log` and tell the user).
- Max 5 concurrent workers (agy + subagents combined). Lanes own disjoint paths;
  worktree isolation if scopes must overlap. Never two lanes on the same files —
  on lane silence, one follow-up + timeout, stop before replacing.
- Every lane prompt cites as binding constraints: `docs/02-requirements.md` §4.1 (IPC
  names), `docs/03-mvp-design.md` §4 (**D1–D24**), and the no-fabrication rule
  (D22–D24: a failed probe surfaces as an error, never as a plausible value).
- Lane prompts carry the completion contract explicitly — workers do not inherit the
  global `<procedural_completion>` doctrine for free. Each prompt states: goal, prior
  findings, disjoint file scope, what NOT to touch, the verification command to run,
  and "report under 200 words + final file paths:line numbers, no diff dumps".
- Authoring and verification are ALWAYS separate lanes — a lane's self-reported success
  is not evidence; the qa lane (or orchestrator) re-runs the checks. Never self-approve.
- `ultrathink` / extended thinking is for architecture and root-cause only (D-registry
  changes, cross-module regressions) — never for mechanical edits.

## OMC plugin utilization

- Default lanes: `executor` (implementation, `sonnet`), `verifier`/`code-reviewer`
  (qa 1st pass, `sonnet`), `designer` (UI, `sonnet`), `critic` (D-registry / architecture
  review and final approval, `opus`).
- QA cycling: `ultraqa` for test→verify→fix loops; `verify` gates "done" claims.
- Debugging: `trace`/`tracer` for cross-module root-cause; `debugger` for build errors.
- Docs: `deepinit` at phase boundaries (hierarchical AGENTS.md), not per-commit.
- agy runtime option: `omc-teams` (tmux panes) when a lane needs live monitoring;
  detached `agy -p` + log files is the default.
- Autonomous modes (`autopilot`/`ralph`/`ultrawork`): keyword-triggered only.
- Knowledge: defects → `docs/mistakes-log.md` (canonical); broader learnings → wiki /
  project-memory.

## Plan Mode

Use for: new IPC commands or FR-level features, D1–D24 registry changes, Colima
lifecycle logic, K8s↔host bridge, guardrails. Skip for: doc typos, styling,
single-file refactors with tests.
