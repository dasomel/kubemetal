# Rust Adoption Assessment

KubeMetal is already Rust-based on the Tauri backend. The goal is targeted Rust deepening, not migration.

High-value areas: OS/hardware access, sandbox/runtime orchestration, air-gap artifact resolver and digest verification, GPU telemetry parsers, Local LLM tool authorization/evidence normalization.

Keep React/TypeScript for presentation and frontend state. Avoid duplicate business logic across Rust and TypeScript.

Acceptance: cargo test/clippy, offline app build, fixture/fault-injection tests, SBOM/provenance, and security tests for LLM tool-call authorization.