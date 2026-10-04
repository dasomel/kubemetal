# Tauri dependency updates

Tauri's [dependency guide](https://v2.tauri.app/develop/updating-dependencies/)
requires matching major.minor for `tauri` / `@tauri-apps/api`, and exact versions
(including patch) for plugin crate / npm pairs. The CI check enforces these
resolved-lockfile contracts for core, dialog and opener. CLI and tauri-build
have independent release lines: no minor-equality rule is asserted for them.
Installed CLI enforcement was not inspected (node_modules was absent).
This check does not replace a real `tauri build` or runtime verification.

Dependabot [groups](https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-options-reference#groups)
apply within an ecosystem entry. The same group name in Cargo and npm does
not produce a combined PR. Coordinate both lockfiles in one integration PR
before merging a minor core or any plugin version update; independently merging
incompatible pairs will fail CI. Weekly schedules and limits remain unchanged.
GitHub also offers a separate multi-ecosystem-groups feature; adopting it is an
owner follow-up rather than a claim that all cross-ecosystem grouping is impossible.

## Owner-visible policy weakening (D1)

`traceExemptExactFileSets` exempts exactly `{src-tauri/Cargo.lock}` from new
operational trace requirements and evidence coverage. The risk stays high.
Any additional changed file, including Cargo.toml, disables this exception.
The previous highest-risk-wins rules cannot express a whole-change-set exemption.
The two trace gates share the predicate to avoid contradictory enforcement.
Cost: dependency-only changes lose new operational evidence even when they
alter native behavior. Escape hatch: remove the policy entry. Owner must review
this weakening before accepting the branch; it is not an exemption from tests,
version coupling, or supply-chain checks, and is not restricted to bot authors.

CI's supply-chain job runs on every pull_request without path filters and invokes
`make supply-chain-check`: `cargo deny --config ../deny.toml check advisories sources`.
This covers resolved Rust lockfile dependencies for advisories and sources; it
is not a proof that updated dependencies are safe. That networked job was not
executed locally for this change.
