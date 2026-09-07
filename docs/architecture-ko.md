# 아키텍처

[English](architecture.md) | 한국어

KubeMetal은 컨트롤 플레인과 ML 컴퓨트를 분리한다. Colima가 관리하는 K3s VM(`vz` +
`virtiofs`)이 MLflow, SeaweedFS, Prefect를 파드로 실행하고, 모든 MLX/Metal 연산은
Tauri/Rust 백엔드가 기동하는 macOS 호스트 프로세스로 실행된다. Apple Metal GPU는 Linux
VM으로 패스스루할 수 없으므로 클러스터 내부에서는 어떤 ML 연산도 실행되지 않는다 — VM은
오케스트레이션과 저장만 담당한다.

이 문서는 짧은 진입점이다. 정본 아키텍처 자료는 다음에 있다:

- [04-architecture.md](04-architecture.md) — 전체 아키텍처 개요 (한국어)
- [03-mvp-design.md](03-mvp-design.md) — MVP 설계 및 결정 레지스트리 (D1…)
- [02-requirements.md](02-requirements.md) — FR/NFR 및 IPC 명령 표
- [adr/README.md](adr/README.md) — 아키텍처 결정 기록(ADR) 색인
