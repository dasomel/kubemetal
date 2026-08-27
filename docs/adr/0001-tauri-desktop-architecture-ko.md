# ADR-0001: Tauri 및 Metal GPU 데스크톱 아키텍처 채택

- 상태: 채택됨 (Accepted)
- 날짜: 2026-08-28

## 배경 (Context)
KubeMetal은 Apple Silicon Metal GPU 디바이스 및 Kubernetes 클러스터 핸들에 대한 네이티브 접근과 고성능 반응형 데스크톱 UI가 필요합니다.

## 결정 (Decision)
Tauri 2.x (Rust 백엔드)와 React/Tailwind 프론트엔드를 채택합니다. Metal GPU 프로브 및 OS 프로세스 제어는 Rust 코어에서 네이티브로 실행됩니다.

## 결과 (Consequences)
- Electron 대비 가벼운 메모리 점유율과 고성능 로컬 디바이스 접근성 확보
- Rust IPC 명령과 React 프론트엔드 상태 간의 명확한 경계 유지
