# KubeMetal: Apple Silicon 전용 하이브리드 MLOps 오픈소스 프로젝트 기획서

---

## 1. 프로젝트 개요 (Overview)

* **프로젝트명 (가칭)**: **KubeMetal** *(또는 SiliconOps)*
* **한 줄 정의**: Apple Silicon(Metal/MLX)의 네이티브 연산 성능과 Kubernetes/MLflow 기반 표준 MLOps 제어면을 하나로 통합하는 **macOS 전용 하이브리드 MLOps 데스크톱 솔루션**
* **핵심 가치**:
* 클라우드 GPU 비용 **$0**
* Apple Silicon 유니파이드 메모리(Unified Memory) 및 Metal/MLX 연산 성능 **100% 활용**
* Colima + K3s 기반 경량 K8s 클러스터와 호스트 GPU를 결합한 **Zero-Config MLOps 워크플로우**



---

## 2. 해결하고자 하는 문제 (Problem & Opportunity)

| 기존 방식 | 한계점 | **KubeMetal의 해결책** |
| --- | --- | --- |
| **전통적 Kubeflow (Full)** | 리눅스 VM 메모리 점유율이 너무 크고, K8s Pod 내부에서 애플 Metal GPU 패스스루 불가 | **Control/Compute 분리 구조**: K8s는 제어(Control)에만 집중하고, 연산은 macOS 호스트(MLX)가 처리 |
| **단순 Local LLM 앱** (LM Studio 등) | 단발성 추론/채팅에만 집중되어 있어 파인튜닝, 파이프라인 자동화, 모델 버전 관리 불가능 | **표준 MLOps 스택 통합**: MLflow, Prefect, MinIO를 자동 내장하여 엔터프라이즈급 워크플로 제공 |

---

## 3. 시스템 아키텍처 (System Architecture)

### 하이브리드 제어/연산 분리 구조

```
┌─────────────────────────────────────────────────────────────────────────┐
│ [ UI Layer ]  Unified Desktop App (Tauri v2 + React / TS)              │
│  • Cluster Manager  • MLX Studio  • Pipeline Visualizer  • Model Hub   │
└────────────────────────────────────┬────────────────────────────────────┘
                                     │ Local IPC / gRPC / WebSockets
┌────────────────────────────────────▼────────────────────────────────────┐
│ [ Control Agent ]  macOS Native Daemon (Rust / Python)                 │
│ ┌──────────────────────────────┐     ┌───────────────────────────────┐ │
│ │ K8s Controller (Colima / vz) │     │ Hardware Guardrail Monitor    │ │
│ │ • MLflow (Experiment Track)  │     │ • Metal GPU / RAM (powermetrics)│
│ │ • MinIO (Artifact Storage)   │     │ • Thermal / Battery Control   │ │
│ │ • Prefect Server (Scheduler) │     └───────────────────────────────┘ │
│ └──────────────────────────────┘                                       │
└────────────────────────────────────┬────────────────────────────────────┘
                                     │ Host Process Spawner / Internal Bridge
┌────────────────────────────────────▼────────────────────────────────────┐
│ [ Compute Engine ]  macOS Host Native Execution                        │
│  • Apple MLX Engine (LoRA Fine-Tuning / Quantization)                   │
│  • mlx-serve / llama-server (Inference Endpoint)                       │
│  • Prefect Host Worker (Task Execution Agent)                           │
└─────────────────────────────────────────────────────────────────────────┘

```

---

## 4. 핵심 기술 스택 (Tech Stack)

| 레이어 | 기술 스택 | 선정 이유 |
| --- | --- | --- |
| **GUI Framework** | **Tauri v2 (Rust + React)** | Electron 대비 메모리 점유율 1/10 수준(RAM 30~50MB), 빠른 반응 속도 |
| **Control Daemon** | **Rust / Python** | macOS `Virtualization.framework` 및 CLI(`colima`, `kubectl`) 제어, `powermetrics` C-Binding 연동 |
| **K8s Runtime** | **Colima (`vz` + `virtiofs`) + K3s** | macOS 네이티브 가상화로 RAM 오버헤드 최소화, 동적 메모리 할당 |
| **Compute Engine** | **Apple MLX / `llama-server**` | 유니파이드 메모리 Zero-copy 지원으로 PyTorch MPS 대비 토큰 속도 20~80% 우수 |
| **Pipeline & MLOps** | **MLflow + Prefect** | Kubeflow 대비 가볍고(RAM 1~2GB), Prefect Host Worker로 호스트 MLX 제어 매끄러움 |
| **Storage** | **MinIO** | S3 호환 로컬 오브젝트 스토리지를 파드로 올려 데이터셋 및 모델 아티팩트 관리 |

---

## 5. 핵심 UI/UX 기능 모듈

1. **Cluster & Infra Health Dashboard**
* Colima(K3s) 클러스터 원클릭 생성/정지/스펙 조절
* macOS 유니파이드 메모리/Metal GPU 실시간 사용량과 K8s Pod 메모리 시각화


2. **MLX Training & Fine-Tuning Studio**
* Hugging Face 모델 검색 및 로컬 다운로드
* Dataset 업로드 $\rightarrow$ MLX LoRA 하이퍼파라미터 설정 $\rightarrow$ 원클릭 파인튜닝
* 학습 손실 커브(Loss Curve)를 K8s MLflow로 실시간 스트리밍


3. **Hybrid Pipeline Visualizer**
* Prefect 기반 DAG(Directed Acyclic Graph) 시각화
* K8s(데이터 전처리) $\rightarrow$ Host(MLX 파인튜닝) $\rightarrow$ K8s(MLflow 등록) $\rightarrow$ Host(서빙 핫리로드) 단계별 상태 모니터링


4. **Model Registry & Playground**
* MLflow 연동 아티팩트 버전 관리 (Staging / Production)
* `mlx-serve` 엔드포인트를 활용한 로컬 프롬프트 테스트 Playground



---

## 6. 디바이스 확장성 및 macOS 가드레일 (MacBook / Mac mini)

Mac mini, Mac Studio뿐만 아니라 **MacBook Air/Pro 라인업** 지원을 위한 하드웨어 프로파일링 및 가드레일 포함:

* **RAM 사양별 자동 프로파일링 (Auto-Config)**
* **16GB RAM**: K8s 메모리 6GB 제한, 소형 모델(1B~8B) LoRA 위주 프로필 설정
* **32GB~48GB RAM**: K8s 메모리 12GB 제한, 중형 모델(14B~32B) 프로필 설정
* **64GB+ RAM**: 대형 모델(70B) 및 풀 파이프라인 모드


* **전력 & 발열 가드레일 (Battery & Thermal Guard)**
* 배터리 모드 진입 감지 시 학습 일시정지 옵션 제공 (`IOPSCopyPowerSources`)
* 고온 진입 시 MLX Batch Size 자동 축소
* 학습 진행 중 슬립 모드 방지 (`caffeinate` 연동)



---

## 7. 단계별 개발 로드맵 (Roadmap)

```
[Phase 1: MVP Core Engine]
├── Tauri v2 백엔드 및 Colima (vz) 원클릭 라이프사이클 제어
├── macOS Metal GPU / RAM 모니터링 데몬 구현
└── K8s 내 MLflow + MinIO 1클릭 셋업 스크립트 구축

[Phase 2: MLX Training & Hybrid Pipeline]
├── 호스트 MLX LoRA 파인튜닝 엔진 연동 (Python/MLX)
├── Prefect Host Worker 기반의 하이브리드 파이프라인 통합
└── MLflow 로깅 및 Model Registry 아티팩트 자동 등록

[Phase 3: GUI Optimization & Packaging]
├── 통합 대시보드 UI 완성 (Training Studio & Pipeline Visualizer)
├── macOS 애플리케이션 번들링 (.dmg) 및 CLI 패키지 자동 내장 (Zero-Config)
└── 하드웨어 가드레일 (배터리/발열/슬립 방지) 추가

```