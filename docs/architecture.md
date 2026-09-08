# Architecture

English | [한국어](architecture-ko.md)

KubeMetal splits control plane and ML compute. A Colima-managed K3s VM (`vz` + `virtiofs`)
runs the control plane — MLflow, SeaweedFS, Prefect — as pods, while all MLX/Metal
compute runs as macOS host processes spawned by the Tauri/Rust backend. Apple Metal GPU
access cannot pass through to a Linux VM, so no ML computation ever runs inside the
cluster; the VM only orchestrates and stores.

This file is a short entrypoint. The canonical architecture material lives in:

- [04-architecture.md](04-architecture.md) — full architecture overview (Korean)
- [03-mvp-design.md](03-mvp-design.md) — MVP design and decision registry (D1…)
- [02-requirements.md](02-requirements.md) — FR/NFR and the IPC command table
- [adr/README.md](adr/README.md) — Architecture Decision Records index
