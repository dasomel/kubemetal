# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.2.0]: https://github.com/dasomel/kubemetal/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/dasomel/kubemetal/releases/tag/v0.1.0
