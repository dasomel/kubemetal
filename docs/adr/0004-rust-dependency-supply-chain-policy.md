# ADR-0004: Rust Dependency Supply-Chain Policy

- Status: Accepted
- Date: 2026-09-17

## Context

Issue #36 requires KubeMetal to pin and verify Rust and toolchain dependencies before
build execution. What's already in the repository, verified by reading the files
directly rather than trusting commit messages:

- `src-tauri/Cargo.lock` is committed; `make supply-chain-check` runs
  `cargo deny check` (`deny.toml`, sections `[graph]`/`[advisories]`/`[sources]` only —
  it does **not** check bans or licenses, contrary to an earlier draft of this ADR).
  OSS license policy is a separate gate, `scripts/release/check_licenses.sh`
  (`make license-check`, part of `make verify`).
- `rust-toolchain.toml` pins the Rust channel to `1.98.0`.
- `scripts/release/gen_sbom.sh` includes Rust build dependencies in the release SBOM.
- `.github/workflows/supply-chain-fixture.yml` is a **manual** (`workflow_dispatch`
  only) negative fixture: it runs `cargo build` against
  `fixtures/build-script-egress-probe/`, whose `build.rs` fails the build if it can
  reach `example.com`, proving `step-security/harden-runner`'s `egress-policy: block`
  does stop an unauthorized build-script network call when that job runs it. **This is
  not part of `.github/workflows/ci.yml`** — regular CI does not use harden-runner and
  does not block build-time egress on every push; the fixture is evidence the control
  works when invoked, not an always-on gate. An earlier draft of this ADR incorrectly
  implied egress is restricted on every CI run.
- `.github/dependabot.yml` opens weekly PRs against `/src-tauri` (cargo), `/` (npm), and
  `/` (github-actions), each capped at 5 open PRs. Dependabot PRs are dependency
  *proposals*, not auto-merges — they still go through the same review this ADR
  defines before merge.

The remaining controls this repo lacks are the human review window for newly adopted
crates and the response path for yanked or compromised crates — this ADR defines only
those two.

## Decision

Adopt the following two policies for Rust dependencies in `src-tauri/Cargo.toml` /
`src-tauri/Cargo.lock`. They apply to direct dependencies and to transitive
dependencies when a lockfile update changes their resolved version or source,
including ones proposed by Dependabot.

### 1. Cooling and review window for newly published or newly adopted crates

Before a crate is added to `src-tauri/Cargo.toml`, or a new crate is first introduced
through a `src-tauri/Cargo.lock` update, the change description must record:

1. The crate name, exact version, source, crates.io publish date, and whether it is a
   direct dependency or a transitive dependency newly selected by resolution.
2. Why the crate is needed, what existing crate or in-tree code it replaces, and why a
   well-known maintained alternative is not sufficient.
3. The result of `make supply-chain-check` (advisories/banned-source policy) and
   `make license-check` (license policy) run against the branch, plus a manual look at
   the crate's download trend, repository activity, maintainer/ownership history, and
   any open advisories not yet in the `rustsec` advisory database. No claim may be
   based only on the crate's name or README.

Newly published crates must remain in a minimum 14-day cooling period after their
crates.io publication date before adoption. Adoption may proceed during that period
only for an urgent security fix or a build-blocking defect, with an explicit exception
recorded in the change description explaining why the wait cannot be honored. A crate
that has existed for at least 14 days still needs the same review when it is newly
adopted — the cooling period is not a substitute for review, including for a
Dependabot-proposed upgrade that changes a crate's source or introduces a new
transitive dependency.

This repository has one active human maintainer (`git log --format=%an` shows
`dasomel`, plus automated `dependabot[bot]` commits) — the review above is
self-review backed by `make supply-chain-check` / `make license-check` output
recorded in the change description, not a second human approver. If a second
maintainer becomes active, their sign-off is required in addition to this checklist,
but the checklist must not assume a second human exists.

### 2. Rollback and quarantine for yanked or compromised crates

When a crate is reported as yanked, malicious, compromised, or otherwise unsafe, the
discoverer opens a private security report (per `SECURITY.md`) when disclosure is
sensitive, and stops merging any dependency update that touches the affected
resolution. The maintainer then:

1. Identifies every affected direct and transitive occurrence from
   `src-tauri/Cargo.lock`, records the bad version/source and the last known-good
   version/source, and preserves the advisory or report reference.
2. Pins the dependency to the last known-good version, regenerates
   `src-tauri/Cargo.lock` with `cargo update --locked -p <crate>@=<version>` (or
   equivalent), and re-runs `make supply-chain-check`, `make license-check`, and CI. If
   no known-good upstream version exists, the release is blocked rather than accepting
   a floating or unverified version.
3. If the application must build before an upstream fix is available, quarantines a
   known-good source by vendoring it in a clearly named, reviewable repository
   location (e.g. a `[patch]` entry pointing at a pinned git revision or vendored
   path), and documents the exact source revision, provenance, and removal condition.
   The quarantine is temporary and must not silently replace the normal crates.io
   source long-term.
4. Before releasing the rollback or quarantined build, records the `src-tauri/Cargo.lock`
   diff, source/revision provenance, and green `make supply-chain-check` / CI results in
   the change description. Re-running `.github/workflows/supply-chain-fixture.yml`
   does not verify anything about the specific rolled-back crate — it only proves the
   generic build-egress control still works — so it is not required for this step. If a
   second maintainer is active, they sign off before release; with one active
   maintainer, the recorded check results are the review evidence.

Re-adoption of a fixed version requires a fresh review under §1 and confirmation that
the upstream issue is resolved (advisory withdrawn, a new version published that
completes its own cooling window, or the maintainer's own audit of the fix). The
quarantine is removed only after that confirmation and a green `make verify`.

## Consequences

- Dependency additions have a concrete minimum observation period and an auditable
  review record grounded in commands that actually run (`make supply-chain-check`,
  `make license-check`), not in controls this repo doesn't enforce on every build.
- A yanked or compromised crate has a documented rollback path that doesn't rely on
  re-running an unrelated manual fixture as if it verified the specific crate.
- The policy is enforceable by one maintainer today, and gains a second-approver step
  automatically if the repository gains a second maintainer, without needing to be
  rewritten.

## Related

- Issue #36.
- `src-tauri/Cargo.lock`, `src-tauri/Cargo.toml`, `rust-toolchain.toml`, `deny.toml`,
  `scripts/release/check_licenses.sh`, `.github/dependabot.yml`, and
  `.github/workflows/supply-chain-fixture.yml`.
