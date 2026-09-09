---
name: kubemetal-hybrid-runtime-change
description: Change KubeMetal host/cluster ML runtime behavior without violating the Apple Silicon control/compute split, deploy-target rendering, command-execution, port, and real-runtime evidence contracts. Use for Tauri/Rust host ML, MLX serving/training, Colima/K3s manifests, bridge, external-cluster, or runtime integration changes.
license: Apache-2.0
compatibility: Requires the KubeMetal checkout, macOS Apple Silicon for real MLX evidence, and the project toolchain documented by Makefile/README.
metadata:
  openforge-scope: project
  openforge-owner: dasomel/kubemetal
  openforge-maturity: verified
  openforge-version: "1"
---

# KubeMetal Hybrid Runtime Change

## Use When

- Changing host-side MLX training/serving or Tauri process execution.
- Changing Colima/K3s deployment, host bridge, external-cluster integration, or runtime ports.
- Adding a workflow that crosses macOS host compute and Kubernetes control-plane resources.

## Do Not Use When

- The task is documentation-only and does not change runtime behavior.
- The change is unrelated UI styling with no runtime/IPC contract impact.

## Inputs

- Relevant D-registry decisions in `docs/03-mvp-design.md`.
- Architecture/requirements docs and current `Makefile` targets.
- Target execution tier: local Colima, external L1 agent-only, or external L2 full-stack.

## Workflow

1. Read `AGENTS.md`, the matching section of `docs/mistakes-log.md`, and all D-registry decisions affected by the change.
2. Preserve the hard boundary: Kubernetes owns control-plane services; MLX/Metal computation remains a macOS host process.
3. Use the repository command-execution abstraction for host CLIs; do not spawn bare binaries from new code paths.
4. For cluster manifests/configuration, keep target-specific substitutions in `render.sh` and the manifest inventory in `scripts/k8s/kustomization.yaml`; do not create a parallel source of truth.
5. Preserve documented port ownership and `127.0.0.1` binding rules. Do not infer bridge addresses or fabricate successful state.
6. For Pod-to-host integration, use the documented ExternalName/EndpointSlice pattern appropriate to the resolved target rather than encoding an IP as an ExternalName.
7. Keep external clusters at the intended integration tier; do not silently turn L1 agent-only into L2 full-stack.
8. Update affected D-registry/architecture documentation in the same task when a recorded decision changes.
9. Run `make verify` and any more specific gate such as `make verify-airgap` that the change can invalidate.
10. Exercise the real macOS/Tauri/MLX/Colima path when the property depends on runtime behavior, and report any unverified tier explicitly.

## Verification

Separate evidence into static/build gates, unit tests, real macOS/Tauri/MLX behavior, and real Kubernetes/Colima behavior. A green build does not prove user-facing reachability, process lifetime, GPU behavior, or cluster bridging.

## Stop / Escalate When

- The proposed design runs Metal/MLX compute inside the Linux VM.
- It requires sudo/root/powermetrics or wider host/process/network authority not covered by an approved design.
- The target bridge, device capability, or runtime state cannot be measured and would require a guess.
- The change alters a D-registry decision without updating its owning design record.

## References

- `AGENTS.md`
- `docs/03-mvp-design.md`
- `docs/04-architecture.md`
- `docs/mistakes-log.md`
- `Makefile`
