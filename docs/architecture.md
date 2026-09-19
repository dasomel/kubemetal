# Architecture

English | [한국어](architecture-ko.md)

KubeMetal separates the **MLOps control plane** from **AI compute**.

The current verified/default path is:

```text
Colima (vz) + K3s
  -> MLflow / SeaweedFS / platform control plane

macOS host
  -> MLX / Metal fine-tuning and serving
```

MLX remains a native macOS/Apple Silicon workload and is not assumed to run inside the
Linux guest or a K8s Pod.

KubeMetal is also evaluating additional compute backends. Colima 0.10+ documents a
`krunkit` VM path for GPU-accelerated containers on Apple Silicon, but KubeMetal treats
this as an **experimental capability**. GPU access inside a VM/container does not by itself
prove K3s Pod GPU access, Kubernetes resource discovery/allocation, scheduling, isolation,
or accounting. Those boundaries are tracked by
[#94](https://github.com/dasomel/kubemetal/issues/94).

The target compute abstraction is:

```text
ComputeBackend
  +-- host-mlx             [default / verified]
  +-- host-cumetal         [experimental]
  +-- krunkit-container    [experimental]
  +-- remote-kubernetes    [extension]
```

Policy-aware backend selection is tracked by #24. CuMetal validation is tracked by #84.
Kubernetes DRA/Kueue integration is intentionally deferred until #94 demonstrates a real
Kubernetes-manageable accelerator resource.

This file is a short entrypoint. The canonical architecture material lives in:

- [04-architecture.md](04-architecture.md) — full architecture overview (Korean)
- [03-mvp-design.md](03-mvp-design.md) — MVP design and decision registry (D1…)
- [02-requirements.md](02-requirements.md) — FR/NFR and the IPC command table
- [adr/README.md](adr/README.md) — Architecture Decision Records index
