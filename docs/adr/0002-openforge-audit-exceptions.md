# ADR-0002: OpenForge Audit Exceptions

- Status: Accepted
- Date: 2026-09-08

## Context
The OpenForge portfolio auditor scores this repository against a shared compliance
rubric. Per OpenForge ADR-0012 (intentional deviations must be documented and
time-bounded, not silently permanent), two checks reflect a real and reasoned
difference from the standard rather than an unfilled gap, and closing them by adding
files alone would misstate how this repository actually operates.

## Decision
Record two intentional, time-bounded exceptions instead of a compliance-shaped but
false change:

**DOC-004 — Language-paired Docs Ratio.** Only 2 of 59 documents in `docs/` have a
Korean pair (3 of 60 once `docs/architecture.md` is paired with
`docs/architecture-ko.md`). `docs/01`–`docs/04` and the numbered design docs are
maintainer-facing Korean working documents that change weekly during active
development; translating them on every edit would go stale immediately for no reader
benefit. The genuinely user-facing surface — README, CONTRIBUTING, SECURITY,
CODE_OF_CONDUCT, and every ADR — is already fully paired. Plan: keep pairing new
entrypoint docs (`architecture.md` done now); translate a working doc once it
stabilizes rather than speculatively.

**I18N-001 — UI Locale Resource Directory.** There is no `locales/`, `messages/`, or
`i18n/` resource directory and no `next-intl`/`react-i18next`/`vue-i18n`/`i18next`
dependency. UI strings already ship in both `ko` and `en` through an in-code
dictionary (`src/i18n/translations.ts` + `src/i18n/i18nContext.tsx`, roughly 750
keys), which is appropriate for a single desktop Tauri app with no runtime resource
loading. Plan: revisit if a third language or an external translator workflow
appears — only then does a resource-file format earn its added complexity.

Both exceptions carry a review date of **2026-12-31**.

## Consequences
- The OpenForge audit continues to score DOC-004 and I18N-001 below full marks until
  the review date; that gap is intentional and tracked here, not closed by keyword
  stuffing.
- On 2026-12-31, re-run the auditor and either renew this ADR with updated evidence
  or close the exception by doing the underlying work.
