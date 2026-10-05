# Observations from 2026-10-04/05 whose revision cannot be a machine-validated record

`research/evidence/2026-10.jsonl` only accepts a `revision` that exists as a commit in the CI
checkout (`scripts/research/check-research-evidence.py`, `git cat-file -e <rev>^{commit}`). The
events below were measured on heads of **squash-merged or deleted PR branches**. None of those
commits is in `main`'s history, and no `main` commit has a byte-identical tree (compared with
`git rev-parse <commit>^{tree}`; for `3835654` vs. `98a3623` the trees differ because that PR branch
predates #161). Remapping them to a `main` commit would assert a revision that was not tested, so
they are kept here instead of being dropped — failed and partial outcomes included, per the standard.

Everything below was read from real command or API output on the dates shown. Nothing is estimated.
Durations are as reported by the tool named in the table. This file is a dated report, not a
schema-validated record; registering it in OpenForge's `portfolio/legacy-evidence-catalog.json`
happens in the `openforge` repository.

| # | Event | Tested commit | Result | Detail |
|---|---|---|---|---|
| 1 | `cargo test` (src-tauri), inside the Codex agent sandbox | `9ea7c6a` | **fail** | 325 passed, 4 failed, 1 ignored. The 4 failures are the port-bind tests failing with `Operation not permitted` (sandbox limit). Not the same code as the later passing run: `9ea7c6a` was followed by a review-fix commit before the pass recorded in `2026-10.jsonl` (record 1, `c0ac85b`, 331 passed / 0 failed outside the sandbox). |
| 2 | Cross-vendor review by Claude Opus of the colima dedicated-profile work | `9ea7c6a` | partial | APPROVE-WITH-FIXES: 4 MED + 5 LOW findings. All 4 MED were fixed or documented in the next commit (`review_corrections` = 4). |
| 3 | Review of the same work by Gemini via `agy` | `9ea7c6a` | **fail** | No review produced: HTTP 429 `RESOURCE_EXHAUSTED` (account quota). |
| 4 | Review of the CI dependabot/Tauri-coupling change by Gemini via `agy` | `dafd507` | partial | 4 findings (1 MED, 3 LOW); 3 acted on in the next commit (the exact-file-set exemption forced the combined Rust+JS PR to need a trace; a test conflated "has paths" with "has high-risk paths"; the new check was missing from `make lint`), 1 LOW judged not applicable (a standalone `unittest` invocation form CI does not use). `review_corrections` = 3. |
| 5 | CI on PR #162 (app icon), per-check job durations from `gh pr checks` | `3835654` | pass | `behavior-contract` 9 s, `verify` 1 m 35 s, `supply-chain` 3 m 02 s. |
| 6 | Agent Behavior on four dependabot Cargo PRs (Cargo.lock-only bumps) | see below | **fail** | "High-risk change requires an operational trace change": `Cargo.lock` is covered by the `src-tauri/**` high-risk rule and a bot bump cannot add a trace. This is the failure that motivated the `Cargo.lock`-only exemption merged in #147. |

Event 6, one row per workflow run (head SHA, branch, `createdAt` → `updatedAt` from `gh run view`, all on 2026-10-01):

| Run | Head SHA | Branch | Wall time |
|---|---|---|---|
| 36891100325 | `eb14c65230922aca0b1d105f7d7eedf537669fc0` | `dependabot/cargo/src-tauri/tauri-plugin-dialog-2.8.0` | 18 s |
| 36891127608 | `d19643e020ff8305b7d9f1b0fbf669ae965ff482` | `dependabot/cargo/src-tauri/tauri-build-2.7.0` | 12 s |
| 36891140995 | `7123369d45cd83e1f864a62205c869eff0c8ae84` | `dependabot/cargo/src-tauri/tauri-plugin-opener-2.6.0` | 11 s |
| 36891160996 | `42cbd894113fea16217921d6e42369c28a612563` | `dependabot/cargo/src-tauri/tauri-2.12.0` | 21 s |

Not recorded for any row: attempt counts, human interventions, CI retries, token use, resource use.
These commits may stop resolving once GitHub garbage-collects the closed PR branches.
