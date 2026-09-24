# KubeMetal Adoption Guide

> Start local. Prove the macOS/Kubernetes control-compute split before attempting advanced external-cluster integration.

## 1. Product boundary

KubeMetal is Apple Silicon-only. Kubernetes/Colima/K3s hosts control-plane services such as MLflow and SeaweedFS, while MLX/Metal compute runs natively on macOS because the Linux VM does not receive Apple Metal GPU passthrough.

## 2. Local first success

Use the README for build/install prerequisites, then verify this sequence:

1. Start the local Colima/K3s environment from KubeMetal.
2. Confirm MLflow and SeaweedFS are reachable.
3. Select/download a small model through the Model Hub path.
4. Confirm the host-side MLX environment is usable.
5. Run a bounded LoRA/fine-tuning or serving workflow.
6. Register/observe the result through MLflow.
7. Confirm service access/port-forward controls work from the desktop application.

A mocked adapter or healthy Kubernetes pod alone does not prove native MLX/Metal runtime behavior.

## 3. External clusters come second

The stack's home is the app's own K3s. External clusters integrate in tiers (D30):

- **L0 remote read (D34)** — one read-only ServiceAccount on the external cluster, zero workloads; diagnosis runs from local kagent. Works even where L1 does not fit.
- **L1 agent-only (default)** — kagent CRDs only, no host bridge; nothing in the cluster may depend on the Mac's local stack.
- **L2 full stack (opt-in)** — D26 stack deploy plus the D10 host bridge. Use it only after preflight passes for kubeconfig context, StorageClass, GitOps/ArgoCD ownership (D27), Kyverno policy, and a verified host-bridge address.

## 4. Read next

- `docs/IMPLEMENTATION-STATUS.md` — implemented scope
- `docs/04-architecture.md` — architecture
- `docs/07-e2e-test-scenario.md` — E2E evidence
- `docs/11-kagent-mlops-integration.md` — kagent integration
- `docs/13-agent-coding-review.md` — agent-assisted engineering review
- `docs/14-idp-integration-review.md` — IDP integration

## 5. Runtime boundaries

Tauri command exposure, filesystem/process/network access, credentials, model/tool authorization, destructive cluster operations, signing identity, and local-network permission are security/runtime boundaries. Verification must match the boundary changed.
