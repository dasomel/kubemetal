# OSS issue resolution — 2026-10-06

## Plan and scope

Synchronize the clean checkout, compare open issues with recent merged source, split
independent audits, reproduce small defects, implement minimal fixes, cross-review,
then run the canonical completion gate. Preserve existing architecture and owner
decisions. No commits, pushes, issue closures, dependency installation, cluster
mutation, credential changes or automatic artifact deletion.

Base: `28ce017c5bc3ff4fa4189f8af73312661830afd8` on `main`, equal to
`origin/main` after `git fetch origin`. Implementation measurements below apply to
the **uncommitted working tree**, not the base commit. They remain observations
rather than canonical JSONL records falsely attributed to an immutable revision.

Three available GPT worker agents audited independent areas, implemented separate
files, and reviewed a different worker's change. These were not AGY executions.

## Verified defect slices

| Issue | Change | Failure before / targeted evidence after | Independent review |
|---|---|---|---|
| #33 | Revalidate persisted adapter names; expose staging enumeration failures; collect entries before reconciliation writes | New regression run: 5 passed, 2 failed. Staging suite: 33 passed, 0 failed | PASS; separate locked Cargo run: 33 passed |
| #17 | Keep malformed non-string manifest paths out of set operations; retain structured FAIL and exit 1 | New regression failed with 4 errors. Support suite: 25 tests, 0 failures, 1 skipped (real bundle unavailable) | PASS; separate run: 25 tests, 1 skipped |
| #38 | Apply pending heading provenance to the merged first chunk only | Heading suite before: 23 tests, 1 failure. Entire RAG suite after: 45 passed | PASS; separate heading suite: 23 passed |

Commands: `cargo test --manifest-path src-tauri/Cargo.toml services::adapter_staging::tests`,
`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib adapter_staging -- --nocapture`,
`python3 -m unittest discover -s scripts/support -p test_verify_support_bundle.py`,
`python3 -m unittest discover -s tests/rag`, and
`python3 -m unittest discover -s tests/rag -p test_rag_chunking.py -v`.

No Tauri UI, real training, real LanceDB retrieval, reboot/soak or cluster runtime
verification was performed. These changes affect pure validation/chunking/service
code; they do not complete their parent epics.

## Remaining open queue

All 29 open issues were checked against current source and issue comments.
Comments lag source in several cases; existing work must not be reimplemented.

| Issues | Classification and remaining boundary |
|---|---|
| #2, #3, #4, #6, #20, #21, #25, #26, #27, #28, #29 | Owner-labelled `on-hold`; retain the hold. #21's retrieval evaluation slice already exists. |
| #33 | Partial. This hardening is complete; S3 read-only startup/list IPC, S4 checkpoint/warm resume and Prefect S2b remain. Existing reconcile can persist promotion state, so directly wiring it as read-only would violate the owner contract. |
| #22 | Partial. D44 integrity contract and direct training promotion are implemented; Prefect legacy direct output still lacks staging/promotion. No whole-issue closure. |
| #17 | Partial. Health UI, support export and offline verifier already exist. Correlation/freshness/dependency capability and app import design remain. |
| #38 | Partial. Korean question-tail fix, lexical/hybrid/auto, heading chunking and evaluation already exist. Optional local rewrite needs model selection; full real-index/UI/offline comparison remains. |
| #18 | Partial. Required target confirmation is implemented. Operation identity/retry deduplication and stale cluster identity require a coherent operation design; real dialog observation remains. |
| #13 | Partial. Battery/wake/orphan controls exist. Reboot/logout recovery, warm resume and real soak evidence remain; vision resume is held. |
| #31 | Partial under D40. Admission is wired; workspace quota/fairness expansion needs scope decisions. |
| #12 | Partial. Rollback IPC/UI exists; issue body has no acceptance specification for remaining evaluation/registry scope. |
| #11 | Partial. GPU benchmark UI already exists; storage/cache benchmark and generic Narwhal evidence contract remain. |
| #98 | Partial, real verification blocked by existing bundle. NOTICE/D42 follow-ups and SBOM tooling already exist. `make verify-airgap-sbom` fails: required `sbom/manifest.json` absent. Bundle has no digest lock either. Do not invent coverage or replace user artifacts. |
| #58 | Partial. Runtime integration, admission and Anthropic compatibility evidence exist. Airgap/warm-cache/SSD restore and external L2 evidence remain. |
| #94, #97 | Partial research. krunkit/smolvm installation, isolated disposable PoC and real GPU/sandbox capability evidence needed. |
| #96 | Blocked by common ComputeBackend and data-transfer policy prerequisites. |
| #84 | Runtime research blocked by installed CuMetal/Metal toolchain and actual GPU evidence. |
| #63, #131 | Research/security design incomplete: external agent resolver, sandbox/credential contracts and actual denial/cleanup evidence required. No unrestricted-host fallback implementation. |
| #54 | Gateway epic unimplemented; actual AWS/temporary credential validation requires configured authority. Existing skill replay is not gateway evidence. |

These are acceptance gaps, not claims that all future repository work is impossible.
Further architecture/runtime slices should have their own plan and corresponding
runtime evidence; speculative adapters would not complete the open issues safely.

## CI, evidence and publication boundary

Latest inspected main CI `37416339780` and Agent Behavior `37416339754` succeeded
at the base revision. No CI failure was hidden or checks weakened.

First `make verify` failed at rustfmt while concurrent workers were adding tests.
Formatting was fixed; the final run is recorded below. Earlier results are retained.
`make research-check` returned `research evidence: OK`.

Existing mistakes/agent traces/CI evidence stays in its original location. Per
`research/README.md`, the portfolio legacy catalog and inventory script belong to
OpenForge (#89); neither is duplicated nor edited in this checkout.
The canonical research standard was read from OpenForge main. This report contains
structured safe summaries, not credential-bearing raw logs.

OpenForge publication was inspected: `.github/workflows/publish-openforge-status.yml`
is manual `workflow_dispatch`. No published status, API/schema or dashboard data was
changed or generated here, so no dashboard value can be claimed as updated.
`CLAUDE.md` adds a Claude-only team harness reference beyond `AGENTS.md`; runtime
skill guidance is already available in the project skills.

## Final integration verification

`make verify` exited **0**. Observed Rust results: **391 passed, 0 failed,
1 ignored**. Python suites: RAG **45**, MLX **11**, support **25 (1 skipped)**,
Colima profile **5**, all with no failures. Airgap SBOM/digest regression suites,
rustfmt, clippy `-D warnings`, TypeScript, design lint, IPC/version parity,
license policy and production web build passed. Vite emitted its existing
bundle-size warning (596.06 kB JavaScript chunk); the gate remained successful.
`git diff --check` passed. Log: `/tmp/kubemetal-oss-final-verify.log` (local only).
No duration was measured for the complete gate, so none is inferred here.

Disposition: three defect slices complete and cross-reviewed; parent issues stay
open with the acceptance gaps above. No remote state changed. Evidence validation
passed, but runtime acceptance beyond these pure-code paths remains unverified.
