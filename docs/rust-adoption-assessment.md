# Rust Adoption Assessment

KubeMetal is already Rust-based on the Tauri backend. The recommendation is therefore not migration but targeted Rust deepening.

## High-value areas

- Kubernetes/Colima command orchestration and typed state machines
- Air-gap artifact collection, manifest resolution, checksum/signature verification
- GPU telemetry parsers with fixture-driven typed models
- Local LLM tool/command authorization and policy evaluation
- Evidence normalization for RCA and upgrade-impact analysis

## Keep TypeScript/React

- UI presentation and rapid UI iteration
- frontend-only state/view concerns

## Boundary rule

Keep OS/hardware, security-sensitive, parser and command-execution logic in Rust; keep presentation logic in TypeScript/React. Avoid duplicating business logic in both layers.

## Validation

- cargo clippy/tests + TypeScript tests
- offline build of .app
- artifact SBOM/provenance
- parser fixture and fault-injection tests
- no shell command injection through LLM tool inputs
