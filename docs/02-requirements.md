KubeMetal 프로젝트의 **[Part 1] 오픈소스 생태계(OSS) 리서치 보고서** 및 **[Part 2] 기술 기능 명세서(Technical Specification)**입니다.

> 본 문서는 `spec.md` 초안에 대한 검토 결과를 반영하여 재작성되었습니다. 포트 충돌, 하드웨어 접근 방식의 사실관계, IPC 커맨드명 불일치, K8s 매니페스트 오류 등을 수정했습니다.

---

# [Part 1] OSS 생태계 분석 및 리서치 보고서

현재 MLOps 및 로컬 AI 생태계는 크게 5개의 그룹으로 나뉘어 있으며, 각 그룹의 명확한 한계점이 **KubeMetal**의 핵심 기회가 됩니다.

### 1. 카테고리별 오픈소스 솔루션 리서치

```
┌────────────────────────────────────────────────────────────────────────┐
│                        Local AI & MLOps OSS Landscape                  │
├──────────────────┬──────────────────┬─────────────────┬────────────────┤
│ 1. Local AI Apps │ 2. Heavy MLOps   │ 3. Mac K8s/VM   │ 4. MLX Engines │
│ (LM Studio, Jan) │ (Kubeflow, Flyte)│ (Colima, Lima)  │ (Apple MLX)    │
└─────────┬────────┴─────────┬────────┴────────┬────────┴────────┬────────┘
          │                  │                 │                 │
          └──────────────────┴────────┬────────┴─────────────────┘
                                      ▼
                        ┌──────────────────────────┐
                        │   Market Gap: KubeMetal  │
                        │ (Native Mac Hybrid MLOps)│
                        └──────────────────────────┘

```

| 카테고리 | 대표 OSS / 솔루션 | 주요 장점 | 한계점 (KubeMetal의 차별화 포인트) |
| --- | --- | --- | --- |
| **1. Local AI Exec Apps** | LM Studio, Jan.ai, Ollama, AnythingLLM | UX가 매우 뛰어남 · GGUF/Metal 추론 지원 | 단발성 추론/채팅에만 집중 · 파인튜닝/파이프라인 자동화 불가 · 실험 추적(Experiment Tracking) 및 버전 관리 부재 |
| **2. Enterprise MLOps** | Kubeflow, Flyte, ClearML, MLflow | 엔터프라이즈 파이프라인 표준 · DAG 및 모델 레지스트리 완비 | Linux/NVIDIA CUDA 중심 설계 · macOS VM 내부 K8s Pod로 Metal GPU 패스스루 불가 · Kubeflow 풀스택은 RAM 16~32GB 이상 상시 점유 |
| **3. Mac K8s / VM Engines** | Colima, Lima, OrbStack, Docker Desktop | macOS `vz` 엔진으로 RAM 오버헤드 최소화 · K3s/Docker 원클릭 실행 | 순수 인프라 런타임 레이어만 제공 · MLOps 전용 대시보드 및 도구 스택 미내장 · 호스트 GPU와의 네트워크 브릿지 수동 설정 필요 |
| **4. Apple Compute Engines** | Apple MLX, `llama-server`, `vllm-mlx` | 애플 실리콘 유니파이드 메모리 100% 활용 · PyTorch 대비 20~80% 빠른 토큰 속도 | CLI/Python 라이브러리 형태 · UI가 없고 K8s 클러스터 제어 기능 부재 |
| **5. Local GUI Fine-Tuning** | **Transformer Lab** | GUI 기반 MLX 파인튜닝 지원 · 실험 관리(experiment tracking) 내장 · KubeMetal과 가장 근접한 직접 경쟁자 | K8s 기반 표준 파이프라인 미지원 · 호스트-컨테이너 하이브리드 브릿지 부재 · MLflow/SeaweedFS 등 표준 MLOps 스택과 통합되어 있지 않음 |

### 2. KubeMetal의 포지셔닝 (Competitive Advantage)

KubeMetal은 "카테고리 1의 유저 친화적 UX" + "카테고리 2의 표준 MLOps 파이프라인" + "카테고리 3의 경량 맥 Virtualization" + "카테고리 4의 MLX 성능"을 통합하는 macOS 전용 하이브리드 MLOps 솔루션을 지향합니다. 가장 근접한 경쟁자는 GUI 기반 MLX 파인튜닝을 제공하는 **Transformer Lab**이며, KubeMetal의 차별점은 (1) K8s 표준 파이프라인(Helm/kubectl 기반 MLflow·SeaweedFS 배포), (2) 호스트-컨테이너 하이브리드 브릿지, (3) MLOps 스택 전체 통합에 있습니다.

---

# [Part 2] KubeMetal 기술 기능 명세서 (Technical Specification)

* **문서 버전**: v0.2
* **대상 시스템**: macOS 14.0 (Sonoma) 이상 (Apple Silicon M1/M2/M3/M4)
* **개발 프레임워크**: Tauri v2 (Rust Daemon) + React / TypeScript (Frontend)

---

## 1. 시스템 요구사항 (System Requirements)

### 1.1 하드웨어 스펙 기준

| 구분 | Minimum (최소) | Recommended (권장) | Enterprise Target |
| --- | --- | --- | --- |
| **Target Device** | MacBook Air / Mac mini | MacBook Pro / Mac Studio | Mac Studio / Mac Pro |
| **SoC** | Apple M1 이상 | M2 Pro / M3 Pro 이상 | M2 Max/Ultra, M3/M4 Max |
| **Unified RAM** | **16 GB** | **32 GB ~ 48 GB** | **64 GB ~ 192 GB** |
| **Disk Storage** | 여유 공간 50 GB (SSD) | 여유 공간 100 GB (SSD) | 여유 공간 500 GB (NVMe) |

### 1.2 K8s VM 자원 프로파일 (호스트 RAM 기반 자동 산정)

FR-01.2의 동적 자원 조절 시 아래 매핑을 기본 프로파일로 사용한다. CPU 코어 할당량은 사용자가 UI에서 조정 가능한 기본값으로 별도 산정한다.

| 호스트 Unified RAM | K8s VM 할당 RAM | 지원 가능 모델 규모 |
| --- | --- | --- |
| 16 GB | 4 GB | 1B ~ 4B 파라미터 |
| 32 GB ~ 48 GB | 8 GB | 7B ~ 14B 파라미터 |
| 64 GB 이상 | 12 GB | 32B ~ 70B 파라미터 |

---

## 2. 기능 요구사항 명세 (Functional Requirements)

### FR-01: 로컬 K8s 클러스터 라이프사이클 제어 (Control Plane)

* **FR-01.1**: Rust 백엔드가 CLI 프로세스로 `colima` 명령을 호출하여 macOS `Virtualization.framework` (`vz`) 및 `virtiofs` 기반 K3s 클러스터를 생성/구동/중지해야 한다.
* **FR-01.2**: 사용자의 RAM 스펙에 맞춰 [1.2 K8s VM 자원 프로파일](#12-k8s-vm-자원-프로파일-호스트-ram-기반-자동-산정)에 따라 K8s 가상머신 자원(CPU, RAM)을 동적으로 조절할 수 있어야 한다.
* **FR-01.3**: K8s 클러스터 상태(Running/Stopped/Creating) 및 Kubeconfig 엔드포인트를 실시간 감지하여 UI에 표출해야 한다. 상태 판별은 `colima status --json` 출력을 파싱하는 방식으로 구현하며, 로그 문자열 매칭에 의존해서는 안 된다.
* **FR-01.4 (CLI 경로 탐색)**: macOS GUI 앱은 로그인 셸의 `PATH`를 상속받지 않으므로, Rust 백엔드는 `colima`, `kubectl`, `helm` 등 필수 CLI 바이너리를 `/opt/homebrew/bin`, `/usr/local/bin`, `$HOME/.colima` 등 알려진 표준 경로에서 우선 탐색하고, 탐색 실패 시 사용자에게 경로를 직접 지정하도록 요청해야 한다.

### FR-02: MLOps 인프라 서비스 자동 프로비저닝

* **FR-02.1**: K8s 클러스터 정상 구동 시, Helm/Kubectl을 통해 **MLflow Tracking Server**와 **SeaweedFS Object Storage**를 파드로 자동 배포해야 한다.
* **FR-02.2**: K8s 내 배포된 MLflow UI 및 SeaweedFS Filer UI/S3 API로 아래 포트를 호스트에 자동 포트포워딩 구성하고, 프론트엔드 Webview로 내장/웹 브라우저 오픈 기능을 제공해야 한다. 포트포워딩은 앱이 관리하는 `kubectl port-forward` 자식 프로세스로 구현하며(`start_port_forward`/`stop_port_forward`), 클러스터 중지 또는 앱 종료 시 해당 프로세스를 정리해야 한다.
  * MLflow UI: 호스트 **5001번 포트** (macOS AirPlay Receiver가 기본 5000번 포트를 점유하므로 5000 사용 금지)
  * SeaweedFS Filer UI: 호스트 **8888번 포트**
  * SeaweedFS S3 API: 호스트 **8333번 포트**

### FR-03: 호스트 Compute Engine (MLX) 파인튜닝 & 서빙 제어

* **FR-03.1**: macOS 호스트 환경의 Python VirtualEnv 또는 독립 바이너리를 이용해 **MLX LoRA Fine-Tuning** 스크립트를 백그라운드 프로세스로 실행할 수 있어야 한다.
* **FR-03.2**: MLX 학습 실행 시, K8s MLflow Server URI (`http://localhost:5001` 또는 내부 DNS)로 학습 파라미터, 손실(Loss) 메트릭, `.safetensors` 무게 파일을 자동 전송하도록 래핑해야 한다.
* **FR-03.3**: 학습이 완료되거나 로컬 모델 선택 시 `mlx-lm` 패키지의 `mlx_lm.server` 또는 `llama-server` 프로세스를 띄워 OpenAI 호환 REST API 엔드포인트(기본값 `http://localhost:8080/v1`, 설정을 통해 변경 가능)를 제공해야 한다.

### FR-04: K8s ↔ macOS Host 하이브리드 브릿지 통신

* **FR-04.1**: K8s 내부 파드가 macOS 호스트의 MLX/Inference API에 접근할 수 있도록 `ExternalName` 서비스(`host.lima.internal`, Colima/vz는 Lima 기반이므로 Docker Desktop 전용 호스트 별칭이 아닌 이 이름을 사용한다)를 자동 셋업해야 한다. `ExternalName` 서비스는 CNAME 별칭만 제공하며 포트를 프록시하지 않으므로, 파드는 `host.lima.internal:8080`처럼 대상 포트를 직접 지정해 접근해야 한다 (상세 매니페스트는 [4.2](#42-k8s-external-service-manifest-spec-mac-gpu-bridgeyaml) 참조).
* **FR-04.2**: Prefect Host Worker 데몬을 호스트에 상주시켜 K8s Prefect Server로부터 전달받은 `gpu:mlx` 스케줄링 태스크를 수신 및 실행해야 한다.

### FR-05: 하드웨어 모니터링 및 안전 가드레일 (Hardware Guardrails)

* **FR-05.1 (Phase 1 · MVP)**: `sysinfo` 크레이트를 사용해 RAM 사용량과 CPU 사용률을 1초 주기로 측정하여 스트리밍해야 한다. MVP 범위에서는 Metal GPU 점유율을 측정하지 않는다.
* **FR-05.4 (Phase 3 · 선택 기능)**: Metal GPU 점유율은 macOS `powermetrics` CLI(공개된 C API가 아닌 커맨드라인 도구이며, 실행에 root 권한이 필요함)의 출력을 파싱하여 측정한다. 이를 위해 별도의 privileged helper 프로세스(예: SMJobBless 또는 launchd privileged helper)를 통해 권한을 격리하여 실행해야 하며, 사용자에게 root 권한 요구 사실과 설치 절차를 명시적으로 고지해야 한다. 본 기능은 Phase 1(MVP) 범위에서 제외한다.
* **FR-05.2 (OOM Protection · Phase 3)**: macOS의 **memory pressure 레벨**(`warn` 또는 `critical`)이 감지되면 진행 중인 MLX 학습 프로세스를 일시정지(Pause)하고 사용자에게 대화상자 경고를 출력해야 한다. macOS는 파일 캐시로 인해 가용 RAM 비율만으로는 상시 오탐이 발생하므로, 단순 "가용 RAM 10% 이하"와 같은 비율 임계값을 트리거 기준으로 사용해서는 안 된다.
* **FR-05.3 (Power/Thermal Guard · Phase 3)**: 배터리 구동 감지 시 학습 일시정지 옵션을 제공하며, 학습 중 슬립 모드 진입을 방지하기 위해 `caffeinate` 어서션을 실행해야 한다.

---

## 3. 비기능 요구사항 명세 (Non-Functional Requirements)

* **NFR-01 (자원 효율성, 목표치)**: KubeMetal 데스크톱 앱(Tauri v2) 자체의 유휴(Idle) 상태 RAM 점유율은 **50MB 이하**를 목표로 한다. 이는 설계 목표치이며, 플랫폼/의존성 버전에 따라 실측치는 별도 벤치마크로 검증한다.
* **NFR-02 (응답 속도)**: Tauri Rust IPC 통신의 지연 시간은 **10ms 이내**여야 하며, UI 메트릭 갱신 주기는 1000ms(1초)를 유지한다.
* **NFR-03 (재현성 & Portability)**: 앱 삭제 시 K8s 가상머신 및 볼륨을 깨끗이 정리하는 `Clean Uninstall` 로직을 지원해야 한다.

---

## 4. API 및 데이터 인터페이스 명세 (Interface Spec)

### 4.1 Tauri Rust Commands (Frontend ↔ Backend IPC)

| Command 이름 | Input Parameters | Output Return | 설명 |
| --- | --- | --- | --- |
| `get_system_metrics` | None | `SystemMetricsJSON` | RAM, CPU 실시간 사용량 리턴 (Metal GPU는 Phase 3, FR-05.4) |
| `get_cluster_status` | None | `ClusterStatusJSON` | colima 상태 + MLflow/SeaweedFS 배포 준비 여부(`mlflow_ready`/`seaweedfs_ready`) 리턴 |
| `start_cluster` | `{ cpu: u32, memory: u32 }` | `Result<String, String>` | Colima `vz` K8s 클러스터 구동 |
| `stop_cluster` | None | `Result<String, String>` | Colima K8s 클러스터 중지 |
| `provision_mlops_stack` | None | `Result<String, String>` | Helm/kubectl로 MLflow·SeaweedFS 파드 자동 배포 (FR-02.1) |
| `start_port_forward` | None | `Result<String, String>` | MLflow(5001) · SeaweedFS(8333/8888) `kubectl port-forward` 자식 프로세스 기동 (FR-02.2) |
| `stop_port_forward` | None | `Result<String, String>` | 추적 중인 포트포워드 자식 프로세스 종료 (FR-02.2) |
| `run_mlx_finetune` | `FineTuneConfigJSON` | `Result<u32, String>` | MLX 파인튜닝 프로세스 띄우고 PID 리턴 |
| `kill_mlx_process` | `{ pid: u32 }` | `Result<bool, String>` | 실행 중인 MLX 학습/서빙 프로세스 중지 |

### 4.2 K8s External Service Manifest Spec (`mac-gpu-bridge.yaml`)

```yaml
apiVersion: v1
kind: Service
metadata:
  name: mac-gpu-service
  namespace: default
spec:
  type: ExternalName
  externalName: host.lima.internal

```

> **비고**: `ExternalName` 서비스는 DNS CNAME 별칭만 제공하며 포트를 프록시하지 않으므로 `ports` 필드를 정의하지 않는다. 파드는 `host.lima.internal:8080`과 같이 목적지 포트를 직접 지정해 접근한다. `host.lima.internal` 이름 해석은 K3s CoreDNS가 해당 이름을 노드의 `resolv.conf`로 포워딩하는 것을 전제로 하며, 이는 실제 Colima/Lima 환경에서의 실기기 검증이 필요한 항목이다. 마찬가지로 `colima status --json`의 출력 스키마(중첩 필드 구성)도 실기기 검증이 필요하다.

---
