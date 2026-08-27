# Current Implementation Status

Last verified: 2026-08-28 against `main`.

This file summarizes capabilities that are implemented in the current repository state. Roadmap/design items remain authoritative in the numbered design documents and GitHub Issues.

## Product boundary

KubeMetal is an Apple Silicon-only hybrid MLOps desktop application built with Tauri v2 + React/TypeScript. It separates Kubernetes-based control-plane services from native macOS MLX/Metal compute.

```text
Kubernetes / Colima / K3s
  -> MLflow, SeaweedFS, manifests, services

macOS host
  -> MLX fine-tuning, model serving, hardware-aware compute
```

The split is intentional: Apple Silicon Metal GPU compute is not passed through to the Linux VM used for Kubernetes.

## Implemented desktop workspaces

- Dashboard
- kagent Ops
- Pipeline
- Model Hub
- MLX Studio
- Data
- Access Console
- Air-Gap Management

## Implemented local MLOps path

- Colima/K3s lifecycle from the desktop app
- MLflow and SeaweedFS provisioning
- model search/download flow
- host-side MLX environment setup
- LoRA fine-tuning
- model serving via host process
- MLflow model registration
- service access and port-forward controls
- data ingestion / chunking / LanceDB RAG / SeaweedFS backup / DVC workflow surfaces

## External cluster integration

The default external-cluster path is agent-only integration (D30): install kagent into the target cluster and use KubeMetal as an observation/diagnostics/operations surface without redeploying the full local MLOps stack.

An opt-in full-stack external deployment path (D26) is also implemented with preflight checks for kubeconfig context, StorageClass, GitOps ownership, Kyverno policy and host bridge reachability.

## macOS operating constraints

The project treats code signing and local-network permission as functional runtime boundaries. A stable signing identity is required for reliable LAN cluster access from a packaged application.

Hardware guardrails include memory-pressure/battery/sleep handling, thermal-state protection and Metal GPU monitoring using a sudo-free IOAccelerator path.

## Air-gap and release evidence

The application includes an Air-Gap Management surface for container images, charts, binaries and version verification. Release tooling also generates third-party notices and CycloneDX/SPDX SBOM artifacts for bundled dependencies.

## Measured reference

The README records packaged-app measurements on Apple M4 Pro / 64GB hardware, including VLM throughput/TTFT, LoRA memory behavior and the VM resource profile. These are reference measurements rather than universal performance guarantees.

## Related evidence

- `README.md`
- `docs/04-architecture.md`
- `docs/07-e2e-test-scenario.md`
- `docs/11-kagent-mlops-integration.md`
- `docs/13-agent-coding-review.md`
- `docs/14-idp-integration-review.md`

Refresh this snapshot when a design decision becomes implemented or when the control/compute boundary changes.