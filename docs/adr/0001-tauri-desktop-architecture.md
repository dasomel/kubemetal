# ADR-0001: Tauri & Metal GPU Desktop Architecture

- Status: Accepted
- Date: 2026-08-28

## Context
KubeMetal requires native access to Apple Silicon Metal GPU APIs and Kubernetes cluster handles while providing a responsive desktop user interface.

## Decision
Adopt Tauri 2.x (Rust backend) with React/Tailwind frontend. Metal GPU probes and OS process orchestration run natively in the Rust core.

## Consequences
- High-performance local device access with minimal memory footprint compared to Electron.
- Clean separation between Rust IPC commands and React frontend state.
