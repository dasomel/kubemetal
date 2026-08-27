# 보안 정책 (Security Policy)

[English](SECURITY.md) | 한국어

## 지원 대상 버전

| 버전 | 지원 여부 |
| ---- | -------- |
| v0.x | :white_check_mark: |

## 보안 범위 및 인증 정보 격리

KubeMetal은 로컬 Kubernetes 구성(`kubeconfig`), Apple Silicon Metal GPU 디바이스 핸들, 컨테이너 런타임 소켓과 상호작용합니다.

- 개인 키, 클러스터 토큰, 민감한 엔드포인트 구성을 저장소에 커밋하지 않습니다.
- 시크릿은 macOS Keychain 또는 안전한 인증 정보 저장소에 보관합니다.
- Tauri IPC 채널을 통한 OS 프로세스 생성 시 엄격한 인수 검증을 수행합니다.

## 취약점 보고 절차 (Reporting a Vulnerability)

보안 취약점은 공개 이슈로 등록하지 마시고, GitHub Private Vulnerability Reporting을 통해 비공개로 보고해 주십시오. 48시간 이내에 접수 확인 및 조치 계획을 안내합니다.

참조: [OpenForge Security Standard](https://github.com/dasomel/openforge/blob/main/docs/security.md)
