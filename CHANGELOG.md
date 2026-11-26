# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-11-27

### Added

- Check for a newer stable GitHub release at launch and from a new "Check for Updates…" app-menu item — when one exists, the release page opens in your browser; nothing is downloaded or installed automatically, and the check is one unauthenticated request to GitHub per launch.
- Run local inference on oMLX as the preferred managed runtime, with `mlx_lm.server` kept as the fallback — start/stop it from MLX Studio (always bound to `127.0.0.1`), load and unload models in its model pool, and see diagnostics, connection profiles, and readiness and air-gap readiness checks.
- Add a local inference benchmark panel that measures reproducible runs including streaming time-to-first-token, and exports the results as an evidence manifest.
- Add a private bridge from the app's K3s cluster to the host inference runtime, started and stopped from MLX Studio, so pods can call the local model without it listening beyond loopback.
- Inspect KV cache disk usage and cache compatibility, and clean up inference caches through a guarded cleanup action.
- Check host resources before starting a model load, a fine-tune or model serving — the app refuses to start new work under critical or unreadable memory pressure, or under serious thermal state when thermal pause is enabled.
- Stage, verify and promote fine-tune output — an adapter becomes available only after its file manifest is verified, model serving refuses an adapter whose manifest no longer matches, and a read-only staging card shows every attempt and its state.
- Save verified checkpoints during mlx-lm fine-tuning and resume an interrupted run from the last one (vision fine-tuning with mlx-vlm shows that resume is unavailable).
- Delete adapter checkpoints from the app after the app re-checks that they are not in use or protected.
- Roll model serving back to the last configuration that passed its health check.
- Detect MLX training or serving processes left behind after the app exits, list them, and terminate one after it is re-verified; MLflow runs of stopped trainings are marked as killed.
- Show recent guardrail pause, resume and wake events in the guardrail card.
- Add a GPU benchmark card that measures matrix-multiply throughput, refuses to run while training or serving uses the GPU, and copies or exports the result as JSON.
- Add a health panel to the dashboard showing the status of every component, labelled as stale when a refresh fails.
- Generate support bundles from the health panel, with secrets redacted, a file manifest, a capture of the host's capabilities, and an offline verifier (`make verify-support-bundle`).
- Show the resolved cluster context, namespace and risk before provisioning, stopping the cluster or installing kagent, and require an explicit confirmation; a second cluster change while one is running is rejected.
- Add lexical (BM25), hybrid, exact and automatic retrieval modes to RAG search — automatic mode explains why it picked a mode, and routes Korean questions to hybrid search.
- Add opt-in heading-aware and Python-source-aware document chunking, and an opt-in local query rewrite that falls back to the original query on timeout; files skipped or unreadable during indexing are reported.
- Write a provenance manifest for every dataset ingestion, recording source hashes, chunking settings, the embedding model and the device used.
- Stage Model Hub downloads and promote them only when complete, with a card listing staged downloads.
- Add an installable rca-agent that proposes root-cause hypotheses only from cited cluster evidence, and otherwise answers "insufficient evidence".
- Add optional CuMetal detection and a CUDA smoke test in MLX Studio (experimental; requires CuMetal to be installed).
- Advertise a simulated NVIDIA GPU on the app's own K3s node for scheduling tests (`make sim-gpu-up` / `make sim-gpu-down`) — it allocates no real GPU and runs no compute in the cluster.

### Changed

- Run the managed VM in a dedicated `kubemetal` Colima profile (context `colima-kubemetal`) — an existing `default` VM is left untouched and not migrated; remove it yourself with `colima delete` if you no longer need it, which deletes its data.
- Pin the K3s API port to 36443 on new VMs, and add `make cluster-repair` to fix clusters that crash-looped after a stored port change.
- Start fine-tuning from the Orchestration card through the same verified path as MLX Studio.
- Split MLX Studio into sub-tabs.
- Replace the blank Dock tile with the KubeMetal app icon.

### Removed

- Remove the Prefect fine-tune flow; Prefect still runs evaluation and ingestion.

### Fixed

- Fix the downloaded `.app` being refused by macOS as "damaged" — release builds are now ad-hoc signed as a whole bundle, so the first launch shows the usual unidentified-developer prompt (right-click → Open). The app is still not notarized.
- Fix RAG search reporting every collection as "not found" with the bundled LanceDB, and hybrid search mixing up chunks from files with the same name.
- Fix GPU memory always showing 0 — an unreadable GPU value is now shown as unavailable instead of 0.
- Fix stopping a training run — the app no longer shows "termination failed" or "exited abnormally" after a successful stop, the panel no longer stays on "training", and a training that dies while paused no longer stays "paused".
- Fix fine-tuning and evaluation results going to an unserved address when MLflow moved off port 5001.
- Fix agent diagnostics on external clusters reporting 0 pods by looking in the cluster's own namespace.
- Fix the health panel reporting unmeasured, incomplete, or installed-but-not-ready components as healthy.
- Fix the header showing v0.1.0 instead of the app's version.
- Reject quantized models for vision fine-tuning before the run starts, instead of failing mid-training.
- Fix the promql, observability and security agents claiming capabilities their tools do not have.

### Security

- Run MLflow, SeaweedFS and Prefect pods as non-root with a read-only root filesystem, all capabilities dropped, and a default-deny network policy.
- Lock air-gap bundle images to their registry digests, require an SBOM for every bundled image, and refuse to install a bundle that has no manifest.
- Ship release archives with SBOMs, third-party license notices and a verified manifest, gated on license policy and HIGH/CRITICAL vulnerabilities.
- Update Rust dependencies for RUSTSEC-2026-0386 (tokio), RUSTSEC-2026-0592/0594 (url, cookie) and GHSA-7fx9-626j-vqph (zerovec).

## [0.2.0] - 2026-08-09

### Added

- Support remote Kubernetes cluster diagnosis using local AI agents without deploying workloads on the target cluster — returned 6 nodes and discovered 124 tools via remote server, and successfully queried a second cluster over an SSH tunnel.
- Add host port conflict detection that automatically avoids occupied ports when starting port forwarding or model serving — shifted MLflow from 5001 to 5002 while four other services retained their ports, and shifted model serving from 8080 to 8081.
- Enable in-app AI ops agent installation and connect its backend to local MLX model serving — verified execution with the local agent returning responses using 3,970 tokens.

### Fixed

- Pre-install Custom Resource Definition charts before main agent deployment — resolves installation failures on new clusters.
- Automatically supply provider API key secrets during agent setup — resolves container configuration errors and brings agent pods to 1/1 Running state.
- Correct the namespace target for the agent UI port forward — resolves the active forward counter sitting at 4/5.
- Require strict HTTP 200 responses for service health checks — eliminates false positives where unrelated containers returning 404 were marked healthy.
- Include missing Custom Resource Definition charts and update Postgres tags in air-gap bundles — fixes bundle completeness reports and offline installation failures.

## [0.1.0] - 2026-07-30

### Added

- Release initial version of KubeMetal for Apple Silicon host-native MLX compute paired with Colima K3s control plane.
- Support automated provisioning of MLflow, SeaweedFS S3 storage, and Prefect orchestration stacks.
- Provide local MLX model fine-tuning and OpenAI-compatible inference serving for text and vision models.
- Support multi-modal document ingestion and local RAG capabilities.
- Add air-gap packaging tools and offline deployment validation probes.
- Support AI ops cluster diagnostics and multi-cluster deployment target abstractions.
- Support bilingual user interface in English and Korean.

[0.3.0]: https://github.com/dasomel/kubemetal/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/dasomel/kubemetal/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/dasomel/kubemetal/releases/tag/v0.1.0
