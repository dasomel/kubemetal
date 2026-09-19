# 아키텍처

[English](architecture.md) | 한국어

KubeMetal은 **MLOps 컨트롤 플레인**과 **AI 컴퓨트**를 분리합니다.

현재 검증 완료된 기본 경로는 다음과 같습니다.

```text
Colima (vz) + K3s
  -> MLflow / SeaweedFS / 플랫폼 컨트롤 플레인

macOS Host
  -> MLX / Metal 파인튜닝·서빙
```

MLX는 macOS/Apple Silicon 네이티브 워크로드이므로 Linux 게스트나 K8s Pod 내부 실행을
기본 전제로 두지 않습니다.

동시에 KubeMetal은 추가 컴퓨트 백엔드를 검토합니다. Colima 0.10+의 `krunkit` 경로는
Apple Silicon GPU-accelerated container 후보이지만, KubeMetal에서는 **Experimental
capability**로만 취급합니다. VM/컨테이너에서 GPU를 사용할 수 있다는 사실만으로 K3s
Pod GPU 사용, Kubernetes 자원 발견·할당·스케줄링·격리·계측이 가능하다고 판단하지
않습니다. 이 경계는 [#94](https://github.com/dasomel/kubemetal/issues/94)에서 실측합니다.

목표 컴퓨트 추상화는 다음과 같습니다.

```text
ComputeBackend
  +-- host-mlx             [기본 / 검증 완료]
  +-- host-cumetal         [experimental]
  +-- krunkit-container    [experimental]
  +-- remote-kubernetes    [확장]
```

정책 기반 backend 선택은 #24, CuMetal 검증은 #84에서 관리합니다. DRA/Kueue는 #94를
통해 실제 Kubernetes 관리 가능 accelerator resource가 확인된 이후에만 별도 통합
대상으로 검토합니다.

이 문서는 짧은 진입점입니다. 정본 아키텍처 자료는 다음에 있습니다.

- [04-architecture.md](04-architecture.md) — 전체 아키텍처 개요
- [03-mvp-design.md](03-mvp-design.md) — MVP 설계 및 결정 레지스트리 (D1…)
- [02-requirements.md](02-requirements.md) — FR/NFR 및 IPC 명령 표
- [adr/README.md](adr/README.md) — 아키텍처 결정 기록(ADR) 색인
