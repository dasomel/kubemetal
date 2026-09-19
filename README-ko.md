# KubeMetal

[English](README.md) | 한국어

KubeMetal은 Apple Silicon Mac에서 Kubernetes 기반 MLOps 컨트롤 플레인과 macOS 네이티브
AI 컴퓨트를 함께 운영하기 위한 데스크톱 플랫폼입니다.

## 핵심 구조

현재 검증 완료된 기본 경로는 **Control / Compute 분리**입니다.

```text
Colima (vz) + K3s
  └─ MLflow / SeaweedFS / 플랫폼 컨트롤 플레인

macOS Host
  └─ MLX / Metal 기반 파인튜닝·서빙
```

MLX는 Apple Silicon/macOS 네이티브 런타임이므로 KubeMetal은 현재도 host-native 실행을
기본 경로로 유지합니다.

다만 Colima 0.10+의 `krunkit` GPU-accelerated container 경로는 새로운 로컬 컴퓨트
백엔드 후보입니다. KubeMetal은 이를 **실험적(Experimental) 경로**로만 취급하며,
VM/컨테이너에서 GPU를 사용할 수 있다는 사실을 곧바로 Kubernetes GPU 스케줄링 지원으로
간주하지 않습니다.

K3s 파드에서 실제 GPU 사용이 가능한지, GPU가 Kubernetes에서 발견·할당·격리·계측 가능한
자원인지 여부는
[#94 — Colima krunkit GPU Container / K3s Scheduling Feasibility](https://github.com/dasomel/kubemetal/issues/94)
에서 별도로 검증합니다.

## ComputeBackend 방향

| Backend | 역할 | 상태 |
|---|---|---|
| `host-mlx` | macOS 네이티브 MLX 파인튜닝/서빙 | **기본 / 검증 완료** |
| `host-cumetal` | Apple Silicon CUDA 호환성 실험 | **Experimental** — #84 |
| `krunkit-container` | Colima/krunkit GPU 컨테이너 | **Experimental / K3s 스케줄링 미검증** — #94 |
| `remote-kubernetes` | 원격 NVIDIA/NPU/가속기 클러스터 | **확장 경로** |

목표 구조는 다음과 같습니다.

```text
사용자 / Agent
      |
      v
Policy + Routing
      |
      v
ComputeBackend
  +-- host-mlx             [default]
  +-- host-cumetal         [experimental]
  +-- krunkit-container    [experimental]
  +-- remote-kubernetes    [extension]
      |
      v
Execution Evidence / Observability
```

로컬 Apple GPU 경로에는 DRA/Kueue를 미리 가정하지 않습니다. 실제로 Kubernetes가
관리할 수 있는 accelerator resource가 확인된 이후에만 별도 통합 대상으로 검토합니다.

## 주요 기능

- **Apple Silicon 네이티브 ML 컴퓨트:** MLX/Metal 기반 로컬 파인튜닝·서빙
- **Kubernetes MLOps 컨트롤 플레인:** Colima/K3s 위 MLflow·SeaweedFS 운영
- **외부 클러스터 연동:** kagent 기반 진단/운영 및 선택적 full-stack 배포
- **정책 기반 런타임 확장:** Local/Experimental/Remote compute backend를 분리하여 검증
- **데스크톱 네이티브 운영:** Rust(Tauri) + React 기반 UI와 호스트 메트릭/가드레일

## 빠른 시작

```bash
# 의존성 설치
pnpm install

# 데스크톱 개발 모드 실행
pnpm tauri dev
```

기본 실행 경로는 기존과 동일하게 Colima `vz` + host MLX이며, `krunkit`은 필수
의존성이 아닙니다.

## 관련 이슈

- [#24 Model Routing / Fallback / Data-Boundary](https://github.com/dasomel/kubemetal/issues/24)
- [#63 Agent Runtime / External CLI Agent](https://github.com/dasomel/kubemetal/issues/63)
- [#84 CuMetal CUDA Compatibility](https://github.com/dasomel/kubemetal/issues/84)
- [#94 Colima krunkit GPU Container / K3s Scheduling Feasibility](https://github.com/dasomel/kubemetal/issues/94)

## 문서 및 가이드

- [전체 아키텍처](docs/04-architecture.md)
- [아키텍처 진입점](docs/architecture-ko.md)
- [보안 정책](SECURITY-ko.md)
- [기여 가이드](CONTRIBUTING-ko.md)
- [행동 강령](CODE_OF_CONDUCT-ko.md)
- [아키텍처 결정 기록(ADR)](docs/adr/README-ko.md)
