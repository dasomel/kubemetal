# KubeMetal: Apple Silicon 전용 하이브리드 MLOps 오픈소스 프로젝트 기획서

> **v0.3 Draft** — 본 문서는 `init.md` 초안에 대한 내부 검토 결과를 반영해 재작성한 프로젝트 기획서다. v0.3에서는 "OSS 설치 도구"에서 "설치된 스택의 유기적 연동 워크스페이스"로 제품 목표를 재정의했다(사용자 피드백 반영, §5·§7 참고).

---

## 1. 프로젝트 개요 (Overview)

* **프로젝트명 (가칭)**: **KubeMetal** *(또는 SiliconOps)*
* **한 줄 정의**: Apple Silicon(Metal/MLX)의 네이티브 연산 성능과 Kubernetes/MLflow 기반 표준 MLOps 제어면을 하나로 통합하고, 설치된 스택 구성요소들을 **유기적으로 연동**해 하나의 운영 화면에서 다루는 **macOS 전용 하이브리드 MLOps 통합 워크스페이스**
* **핵심 가치**
  * 클라우드 GPU 비용 **$0**
  * Apple Silicon 유니파이드 메모리(Unified Memory) 및 Metal/MLX 연산 성능 **최대 활용**
  * Colima + K3s 기반 경량 K8s 클러스터와 호스트 GPU를 결합한 **Zero-Config MLOps 워크플로우**
  * **"설치"가 아니라 "연동"**: OSS 스택(MLflow/SeaweedFS 등)을 개별 배포하는 데 그치지 않고, 서비스 간 연동(아티팩트 스토어 자동 와이어링), 모델 다운로드→저장→등록 파이프라인, 통합 인증 접근까지 앱 UI에서 이어지는 하나의 흐름으로 제공

---

## 2. 해결하고자 하는 문제 (Problem & Opportunity)

| 기존 방식 | 한계점 | **KubeMetal의 해결책** |
| --- | --- | --- |
| **전통적 Kubeflow (Full)** | 리눅스 VM 메모리 점유율이 너무 크고, K8s Pod 내부에서 애플 Metal GPU 패스스루 불가 | **Control/Compute 분리 구조**: K8s는 제어(Control)에만 집중하고, 연산은 macOS 호스트(MLX)가 처리 |
| **단순 Local LLM 앱** (LM Studio 등) | 단발성 추론/채팅에만 집중되어 있어 파인튜닝, 파이프라인 자동화, 모델 버전 관리 불가능 | **표준 MLOps 스택 통합**: MLflow, Prefect, SeaweedFS를 자동 내장하여 엔터프라이즈급 워크플로 제공 |
| **Transformer Lab** | GUI + MLX 파인튜닝 + 실험 관리를 이미 제공하는 직접 경쟁 오픈소스로, 단일 앱 내 자체 실험 트래킹 위주 | **K8s 표준 파이프라인 + 하이브리드 브릿지**: MLflow/Prefect/SeaweedFS 같은 업계 표준 컴포넌트를 K8s 위에서 그대로 운용하면서 원격/멀티노드 확장 경로를 열어둠 |

> KubeMetal이 이 영역에서 유일한 시도라고 주장하지는 않는다. 차별점은 "MLX 로컬 연산"과 "K8s 표준 제어면"을 하이브리드로 연결해, 로컬 데스크톱에서 시작해 향후 원격/클러스터 환경으로 자연스럽게 확장 가능한 파이프라인을 제공하는 데 있다.

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
│ │ • MLflow (Experiment Track)  │     │ • RAM/CPU (sysinfo, Phase 1)  │ │
│ │ • SeaweedFS (Artifact Store) │     │ • Metal GPU (powermetrics,    │ │
│ │ • Prefect Server (Scheduler) │     │   Phase 3 선택 기능·root 필요)│ │
│ └──────────────────────────────┘     └───────────────────────────────┘ │
└────────────────────────────────────┬────────────────────────────────────┘
                                     │ Host Process Spawner / Internal Bridge
┌────────────────────────────────────▼────────────────────────────────────┐
│ [ Compute Engine ]  macOS Host Native Execution                        │
│  • Apple MLX Engine (LoRA Fine-Tuning / Quantization)                   │
│  • mlx_lm.server / llama-server (Inference Endpoint, :8080)             │
│  • Prefect Host Worker (Task Execution Agent)                           │
└─────────────────────────────────────────────────────────────────────────┘
```

### 3.1 포트 구성

호스트에서 K8s 내부 서비스로의 포트포워딩 및 로컬 서빙 엔드포인트는 아래와 같이 고정한다.

| 서비스 | 포트 | 비고 |
| --- | --- | --- |
| MLflow Tracking Server | **5001** | macOS AirPlay Receiver가 기본 5000번 포트를 점유하므로 5001로 포워딩 |
| SeaweedFS S3 API | **8333** | |
| SeaweedFS Filer UI | **8888** | |
| 모델 서빙 엔드포인트 (`mlx_lm.server` / `llama-server`) | **8080** | 설정으로 변경 가능 |

### 3.2 K8s 채택 근거

MLflow/SeaweedFS/Prefect Server를 K8s(Colima + K3s VM) 위에 올리는 것은 이 세 컴포넌트만 놓고 보면 VM 오버헤드 대비 실익이 논쟁적일 수 있다(단순 `docker compose`로도 동일 스택을 띄울 수 있기 때문). 그럼에도 K8s를 채택하는 이유는 다음과 같다.

1. **표준 매니페스트 기반 제어면**: Helm 차트/YAML 매니페스트로 MLOps 스택을 선언적으로 관리해, 클라우드 K8s 환경과 동일한 운영 경험과 재사용 가능한 설정을 제공한다.
2. **원격/멀티노드 확장 대비**: 로컬 단일 노드로 시작하지만, 향후 원격 GPU 서버나 여러 Mac을 K3s 멀티노드 클러스터로 묶어 확장하는 경로를 처음부터 열어둔다. Docker Compose로는 이 확장 경로를 자연스럽게 얻기 어렵다.
3. **컴포넌트 생명주기 표준화**: Pod 재시작, 리소스 제한(`ResourceQuota`), 헬스체크 등 K8s의 운영 프리미티브를 그대로 활용한다.

다만 저사양 기기(16GB RAM)에서는 VM 오버헤드가 부담이 될 수 있어, 로드맵의 검토 항목으로 **"K8s 없이 Docker 런타임만 사용하는 Lite 모드"**를 별도로 검토한다 (§7 참고).

---

## 4. 핵심 기술 스택 (Tech Stack)

| 레이어 | 기술 스택 | 선정 이유 |
| --- | --- | --- |
| **GUI Framework** | **Tauri v2 (Rust + React)** | Electron 대비 메모리 점유율이 낮고(RAM 약 30~50MB), 빠른 반응 속도 |
| **Control Daemon** | **Rust / Python** | macOS `Virtualization.framework` 및 CLI(`colima`, `kubectl`) 제어. `powermetrics`는 CLI 도구이며 공개 C API가 없어, 필요 시 서브프로세스 실행 + stdout 파싱 방식으로 연동 (Phase 3, root 권한 필요) |
| **K8s Runtime** | **Colima (`vz` + `virtiofs`) + K3s** | macOS 네이티브 가상화로 RAM 오버헤드 최소화, 동적 메모리 할당 |
| **Compute Engine** | **Apple MLX / `llama-server`** | 유니파이드 메모리 zero-copy 지원. PyTorch MPS 대비 토큰 처리 속도는 워크로드(모델 크기, 배치, 양자화)에 따라 상이하며, 구체적 수치는 자체 벤치마크로 검증 예정 |
| **Pipeline & MLOps** | **MLflow + Prefect** | Kubeflow 대비 가볍고(RAM 약 1~2GB), Prefect Host Worker로 호스트 MLX 제어가 매끄러움 |
| **Storage** | **SeaweedFS** | S3 호환 + 단일 바이너리 경량 구동(master/volume/filer/S3 일체형)으로 파드로 올려 데이터셋 및 모델 아티팩트 관리 |

---

## 5. 핵심 UI/UX 기능 모듈

1. **Cluster & Infra Health Dashboard**
   * Colima(K3s) 클러스터 원클릭 생성/정지/스펙 조절
   * macOS RAM/CPU 사용량(Phase 1, sysinfo 기반)과 K8s Pod 메모리 시각화
   * Metal GPU 점유율은 Phase 3에서 선택적 privileged helper를 통해 표시 (root 권한 필요)

2. **MLX Training & Fine-Tuning Studio**
   * Hugging Face 모델 검색 및 로컬 다운로드
   * Dataset 업로드 → MLX LoRA 하이퍼파라미터 설정 → 원클릭 파인튜닝
   * 학습 손실 커브(Loss Curve)를 K8s MLflow(포트 5001)로 실시간 스트리밍

3. **Model Hub** *(신설, FR-07)*
   * Hugging Face 모델 검색 → 호스트 다운로드 → SeaweedFS 업로드 → MLflow Model Registry 등록까지 이어지는 단일 흐름을 앱 UI에서 원클릭으로 처리
   * 등록된 모델 목록/상세(버전, 스토리지 위치, 등록 상태) 조회
   * "다운로드 도구"가 아니라 로컬 모델 자산의 저장소–레지스트리 연동을 자동화하는 허브

4. **Hybrid Pipeline Visualizer** *(FR-08)*
   * 전처리 → 학습(Host MLX) → 등록(K8s MLflow) → 서빙(Host 핫리로드) 단계별 상태를 카드/그래프로 시각화
   * Phase 2c까지는 Prefect 도입 전이므로 앱 내부 오케스트레이션 상태만 표시, 이후 Prefect 기반 DAG 시각화로 확장

5. **Model Registry & Playground**
   * MLflow 연동 아티팩트 버전 관리 (Staging / Production)
   * `mlx_lm.server` 또는 `llama-server` 엔드포인트(기본 포트 8080)를 활용한 로컬 프롬프트 테스트 Playground

6. **Integrated Access Console** *(신설, FR-09)*
   * MLflow UI, SeaweedFS Filer UI 등 설치된 각 서비스로 크리덴셜 입력 없이 원클릭 진입(임베디드 뷰 또는 브라우저 오픈)
   * 서비스별 크리덴셜은 프로비저닝 시 앱이 자동 발급/저장하며, 사용자는 개별 로그인 절차를 신경 쓰지 않음
   * 로컬 단일 사용자 범위의 "통합 접근"이며, 멀티유저/원격 확장 시 Keycloak급 IdP 기반 SSO 검토 (§7 Phase 2d)

---

## 6. 디바이스 확장성 및 macOS 가드레일 (MacBook / Mac mini)

Mac mini, Mac Studio뿐만 아니라 **MacBook Air/Pro 라인업** 지원을 위한 하드웨어 프로파일링 및 가드레일을 포함한다. 아래 기본값은 하드코딩이 아니라 **앱 실행 시 감지된 총 RAM을 기준으로 자동 산정**되는 값이며, 사용자가 수동으로 조정할 수 있다.

* **RAM 사양별 자동 프로파일링 (Auto-Config)**

  | 감지된 RAM | K8s VM 메모리 제한 (기본값) | 권장 모델 규모 |
  | --- | --- | --- |
  | 16GB | 4GB | 1B~4B 모델 LoRA 파인튜닝 |
  | 32GB~48GB | 8GB | 7B~14B |
  | 64GB+ | 12GB | 32B~70B |

* **메모리 압박(OOM) 가드레일**
  * 절대 여유 RAM 비율이 아니라 macOS의 **memory pressure 레벨(`warn` / `critical`)**을 기준으로 판단한다. macOS는 파일 캐시 등으로 인해 평상시에도 RAM 점유율이 상시 높게 나타나므로, 단순 "가용 RAM 10% 이하" 같은 비율 기준은 오탐이 많다.
  * `warn` 레벨 진입 시 신규 학습/서빙 작업 시작을 보류하고 사용자에게 알림, `critical` 레벨 진입 시 실행 중인 MLX 작업의 배치 크기 축소 또는 일시정지를 트리거한다.

* **전력 & 발열 가드레일 (Battery & Thermal Guard)**
  * 배터리 모드 진입 감지 시 학습 일시정지 옵션 제공 (`IOPSCopyPowerSources`)
  * 고온 진입 시 MLX 배치 크기 자동 축소
  * 학습 진행 중 슬립 모드 방지 (`caffeinate` 연동)

---

## 7. 단계별 개발 로드맵 (Roadmap)

```
[Phase 1: MVP Core Engine]
├── Tauri v2 백엔드 및 Colima (vz) 원클릭 라이프사이클 제어
├── macOS RAM/CPU 모니터링 데몬 구현 (sysinfo 기반)
└── K8s 내 MLflow(5001) + SeaweedFS(8333/8888) 1클릭 셋업 스크립트 구축

[Phase 2: 유기적 연동 — 설치에서 통합 운영으로]
├── 2a. 서비스 연동 자동 구성 (FR-06)
│   ├── 프로비저닝 시 MLflow tracking server의 artifact store를 SeaweedFS S3
│   │   (endpoint http://seaweedfs:8333, 버킷 mlflow)로 자동 와이어링
│   ├── 버킷 자동 생성, 연동 상태를 get_cluster_status로 노출
│   └── (D13, docs/03-mvp-design.md §4)
├── 2b. 모델 허브 (FR-07)
│   ├── Hugging Face 모델 검색 → 다운로드(호스트 저장) → SeaweedFS 업로드
│   │   → MLflow Model Registry 등록
│   └── 등록 모델 목록/상세 UI
├── 2c. MLX 파인튜닝 + 파이프라인 가시화 (FR-08)
│   ├── 호스트 MLX LoRA 파인튜닝 엔진 연동 (Python/MLX)
│   ├── MLflow 로깅 및 Model Registry 아티팩트 자동 등록
│   └── 전처리→학습→등록→서빙 단계별 상태 카드/그래프 표시
│       (Prefect 도입 전에는 앱 내 오케스트레이션 상태만)
└── 2d. 통합 접근 (FR-09)
    ├── 서비스 크리덴셜 자동 프로비저닝 + 앱에서 원클릭 인증 접근(임베디드/브라우저)
    └── Keycloak급 IdP 기반 SSO는 멀티유저/원격 확장 시 별도 검토

[Phase 3: GUI Optimization & Packaging]
├── 통합 대시보드 UI 완성 (Training Studio & Pipeline Visualizer)
├── macOS 애플리케이션 번들링 (.dmg) 및 CLI 패키지 자동 내장 (Zero-Config)
├── 하드웨어 가드레일 (배터리/발열/슬립 방지) 추가
└── (선택) powermetrics 기반 Metal GPU 점유율 모니터링 — privileged helper 방식, root 권한 필요

[검토 항목 / Backlog]
└── 16GB급 저사양 기기 대상 "K8s 없이 Docker 런타임만 사용하는 Lite 모드" 도입 검토
```
