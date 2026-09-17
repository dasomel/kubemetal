# ADR-0004: Rust Dependency Supply-Chain Policy

- Status: Accepted
- Date: 2026-09-17

## Context

Issue #36 requires KubeMetal to pin and verify Rust and toolchain dependencies before
build execution. The repository already commits `Cargo.lock`, runs locked builds in
`.github/workflows/ci.yml`, installs and runs `cargo-deny` with `--locked`, pins Rust in
`rust-toolchain.toml`, includes Rust build dependencies in `scripts/release/gen_sbom.sh`,
and proves unauthorized build-script egress is blocked in
`.github/workflows/supply-chain-fixture.yml` using
`fixtures/build-script-egress-probe/`. The remaining controls are the human review
window for new crates and the response path for yanked or compromised crates.

## Decision

Adopt the following two mandatory policies for Rust dependencies. They apply to direct
dependencies and to transitive dependencies when a lockfile update changes their
resolved version or source.

### 1. Cooling and review window for newly published or newly adopted crates

Before a crate is added to `Cargo.toml`, or a new crate is first introduced through a
lockfile update, the proposing maintainer must record the following in the change
description:

1. The crate name, exact version, source, crates.io publish date, and whether it is a
   direct dependency or a transitive dependency newly selected by resolution.
2. Why the crate is needed, what existing crate or in-tree code it replaces, and why a
   well-known maintained alternative is not sufficient.
3. The crate's download trend, repository activity, maintainer/ownership history,
   license, and known advisories or yanks. The reviewer must inspect the resolved
   dependency set represented by `Cargo.lock` and run the repository's `cargo-deny`
   check; no claim may be based only on a crate's name or README.

Newly published crates must remain in a minimum 14-day cooling period after their
crates.io publication before adoption. Adoption may proceed during that period only for
an urgent security fix or a build-blocking defect, with an explicit exception recorded
in the change description, including the reason the wait cannot be honored. A crate
that has existed for at least 14 days still needs the same review when it is newly
adopted; the cooling period is not a substitute for review. The maintainer re-checks
the publish date, version, source, `cargo-deny` (`deny.toml`) advisory/ban/license
result, and the `Cargo.lock` diff immediately before merge, and records that check in
the change description — this repository has one active maintainer
(`git log --format=%an` shows `dasomel` plus `dependabot[bot]`), so the check is
self-review against `cargo-deny`'s automated result, not a second human approver. If a
second maintainer is active at review time, their sign-off is required in addition to
this checklist, but the checklist itself must never assume a second human exists.

### 2. Rollback and quarantine for yanked or compromised crates

When a crate is reported as yanked, malicious, compromised, or otherwise unsafe, the
discoverer must open a private security report when disclosure is sensitive and must
stop merging dependency updates that change the affected resolution. The maintainer
then:

1. Identifies every affected direct and transitive occurrence from `Cargo.lock`, records
   the bad version/source and the first known-good version/source, and preserves the
   report or advisory reference.
2. Pins the dependency to the last known-good version in the manifest or lockfile path
   that controls resolution, updates `Cargo.lock` with the locked workflow, and runs
   `cargo-deny` plus the relevant CI checks in `.github/workflows/ci.yml`. If no known-
   good upstream version exists, the release/build is blocked rather than accepting a
   floating or unverified version.
3. If the application must build before an upstream fix is available, quarantines the
   known-good source by vendoring it in a clearly named, reviewable repository location
   and documents the exact source revision, provenance, changes, and removal condition.
   The quarantine is temporary and must not silently replace the normal crates.io
   source.
4. Before releasing the rollback or quarantined build, re-runs `cargo-deny` and the CI
   checks in `.github/workflows/ci.yml` plus the `.github/workflows/supply-chain-fixture.yml`
   egress probe, and records the lockfile diff and source/revision provenance in the
   change description; no release proceeds on an unverified dependency. If a second
   maintainer is active at the time, they sign off before release; with one active
   maintainer, the recorded `cargo-deny`/CI/fixture results are the review evidence.

Re-adoption of a fixed version requires a fresh review under §1 and confirmation that
the upstream issue is resolved (advisory withdrawn, new version published and it
completes its own cooling window, or the maintainer's own audit of the fix). The
quarantine is removed only after that confirmation and after the normal locked CI path
is green.

## Consequences

- Dependency additions have a concrete minimum observation period and an auditable
  review record.
- A yanked or compromised crate has a known-good rollback path without weakening the
  repository's locked-build, `cargo-deny`, or build-egress controls.
- Emergency adoption and temporary quarantine can restore delivery, but both require
  explicit approvals and leave a review trail.

## Related

- Issue #36 and the controls listed in this ADR's Context section.
- `Cargo.lock`, `rust-toolchain.toml`, `.github/workflows/ci.yml`, and
  `.github/workflows/supply-chain-fixture.yml`.
