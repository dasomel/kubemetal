# OSS issue resolution — 2026-10-17

## Plan

Preserve the clean local `main` at `333a658563bd72adb668c0789867cfca3772bac0`,
including its seven commits ahead of `origin/main`. Fetch completed; upstream has
no commits missing locally. Audit all 29 open issues against source and owner
comments, retain explicit holds, implement reproducible safe slices, obtain an
independent review, and run the relevant regression and canonical completion gates.

Three GPT workers perform runtime triage/implementation, research triage/review,
and CI analysis/validation. These are available native agents, not AGY executions.
No commits, pushes, issue closures, releases or shared-cluster changes are planned.
Runtime and research epics require their actual acceptance evidence; passing unit
tests alone will not be reported as whole-issue completion.

First implementation candidate: #18 operation replay must bind operation identity
to the authorized action and target. Scope: deploy-operation service, its callers,
regressions and relevant documentation. Preserve target confirmation and existing
cluster execution boundaries. Validate a failing reproduction, corrected regression,
independent review and `make verify`.

## Evidence policy

Use the existing research schema and preserve failed attempts. Working-tree
measurements must say they include uncommitted changes. Original legacy artifacts
remain untouched. The portfolio catalog belongs to the neighboring OpenForge
checkout; preserve its existing untracked `Cargo.lock`. No published portfolio
status or dashboard value is assumed to change from local evidence alone.

Results and remaining issue dispositions will be appended after observation.

## Results (2026-10-18, working tree incl. uncommitted changes)

- `make verify` first run: exit 2, only `cargo fmt --check` on the untracked
  `support_bundle/verification_tests.rs`. After `cargo fmt`: exit 0; 429 Rust tests
  passed, 2 ignored (on-device MLX, opt-in). Logs: `.omc/logs/verify-wt-2026-10-18{,b}.log`.
- Independent Opus review (read-only): no blocker/major. Slices #18, #17 PASS with minors;
  #11, #38 PASS. Minor, unaddressed: kagent install replay not bound to values/chart
  version; support-bundle path check is TOCTOU-open for a local attacker (needs `openat`).
- Review correction applied: #18 cross-target test now asserts the "different request"
  message; `deploy_operation` tests 25/25 after the change.
- Not run: real app, real colima, or CI. Nothing committed or pushed.
- Not done: per-issue triage of the 29 open issues against acceptance criteria; PR #188 CI failure.

## #38 eval harness run (2026-10-22, revision e706c3b, clean tree)

`python3 scripts/rag/eval_retrieval.py --chunking both`, corpus = this repo's `docs/`
(fixed 763 chunks / heading 682 chunks), 12 hand-labeled queries, one run, lexical (FTS5) only.
dense/hybrid/auto: not measured (need embedding model + LanceDB).

| mode | type | recall@3 | mrr | ndcg@3 | p50 ms | p95 ms |
|---|---|---|---|---|---|---|
| fixed | ALL | 0.750 | 0.692 | 0.678 | 9.873 | 11.267 |
| fixed | natural_language | 0.500 | 0.385 | 0.355 | 9.967 | 10.201 |
| heading | ALL | 0.833 | 0.695 | 0.719 | 9.318 | 12.763 |
| heading | natural_language | 0.667 | 0.473 | 0.500 | 9.639 | 10.330 |

Caveats: n=12 and single-sample latency (host noise included), so this is a regression
baseline, not evidence that heading chunking beats fixed. Not added to
`research/evidence/*.jsonl`: its schema validator lives in the neighboring OpenForge checkout.
