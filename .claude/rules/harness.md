# Agent Team Harness (agy-first) — full rules

Summary lives in `CLAUDE.md`; this file is the detail. Load when orchestrating lanes.
Model tiering, agy rotation, and the failure ladder are owned by the global
`<routing_doctrine>` / `<agy_cli>` (`~/.claude/CLAUDE.md`) — this file pins only the
**project-specific** lane → scope → worker → model mapping.

## Lane table

| Lane | Scope (disjoint — do not cross) | Worker | Model |
|------|--------------------------------|--------|-------|
| `rust-backend` | `src-tauri/**`, `scripts/**` (k8s manifests, mlx/prefect/ingest/airgap/e2e host scripts), `Cargo.toml`, `tauri.conf.json`, `capabilities/` | agy (primary) | `Gemini 3.6 Flash (High)` → `Claude Opus 4.6 (Thinking)` on quota exhaustion; native fallback `sonnet` |
| `frontend` | `src/**`, `package.json`, `tsconfig.json`, `vite.config.ts`, `index.html` | agy (primary) | same rotation; native fallback `sonnet` |
| `ui-design` | `DESIGN.md`, `tailwind.config.js`, component styling | `designer` subagent | `sonnet` |
| `qa/verify` | `cargo check`/`clippy`, `tsc --noEmit`, DESIGN.md lint, doc↔code sync (D-registry, IPC names) | `verifier` / `code-reviewer` subagent | `sonnet` (1st pass) |
| `approve` | final approval pass on high-risk diffs — D-registry changes, colima lifecycle, guardrails (D11/D16/D17), credential & SSRF paths (D21), K8s↔host bridge (D10) | `critic` / `code-reviewer` subagent | `opus` |
| `escalate` | only after an `opus` lane produced a demonstrably wrong/insufficient result on the hardest reasoning step | subagent | `fable` (rare) |

Re-tiering note: lanes this harness once pinned to `opus` (code/YAML authoring, manifest
normalization, 1st-pass review, doc drafts) belong on `sonnet`. Keep `opus` for the
`approve` lane and cross-cutting judgment only — an all-`opus` harness is misrouted,
not thorough.

## Rules

Dispatch mechanics (`agyp` vs raw `agy`, `--add-dir`, permission flags, rotation, failure
ladder) belong to the global `<agy_cli>` — don't restate them here. What this repo adds:

- **`agyp` injection reaches `CLAUDE.md` + this file, and nothing else.** `docs/02` §4.1
  (IPC names) and `docs/03` §4 (D-registry) are *not* injected, so a prompt saying "follow the
  registry" gives the worker nothing. Quote the specific decisions the lane must honor.
- **Lanes own disjoint paths** (table above), max 5 concurrent. If scopes must overlap, use
  worktree isolation. On lane silence: one follow-up with a timeout, and stop it before
  starting a replacement — two lanes on one file has bitten this repo twice.
- Worker summary → `.omc/logs/agy-<lane>-result.md`, stdout → `.omc/logs/agy-<lane>.log`.
  Keep raw output out of main context.
- **Workers inherit no doctrine.** Whatever the lane must satisfy — goal, file scope, what
  not to touch, the verification command, report shape — says so in the prompt.
- **Authoring never approves itself.** The qa lane re-runs the checks; a lane's own
  "done" is not evidence.

## OMC agents for these lanes

`executor` implements, `verifier`/`code-reviewer` take the qa pass, `designer` owns UI,
`critic` takes the approval lane. Detached `agyp` + log files is the default runtime;
`omc-teams` (tmux panes) only when a lane needs live watching. Autonomous modes
(`autopilot`/`ralph`/`ultrawork`) are keyword-triggered, never assumed.

Defects land in `docs/mistakes-log.md` — that file is the reason this repo stopped
repeating the same class of mistake, so a fix without a row there is unfinished.

## Plan Mode

Worth it for new IPC commands, FR-level features, D-registry changes, colima lifecycle,
the K8s↔host bridge and guardrails — anything where the wrong shape is expensive to undo.
Skip it for typos, styling, and single-file refactors that tests already cover.
