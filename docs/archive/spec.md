KubeMetal 프로젝트의 **[Part 1] 오픈소스 생태계(OSS) 리서치 보고서** 및 [Part 2] 기술 기능 명세서(Technical Specification)입니다.

---

# [Part 1] OSS 생태계 분석 및 리서치 보고서

현재 MLOps 및 로컬 AI 생태계는 크게 4개의 그룹으로 나뉘어 있으며, 각 그룹의 명확한 한계점이 **KubeMetal**의 핵심 기회가 됩니다.

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
| **1. Local AI Exec Apps** | **LM Studio, Jan.ai, Ollama, AnythingLLM** | • UX가 매우 뛰어남<br>

<br>• GGUF/Metal 추론 지원 | • 단발성 추론/채팅에만 집중<br>

<br>• 파인튜닝, 파이프라인 자동화 불가<br>

<br>• 실험 추적(Experiment Tracking) 및 버전 관리 부재 |
| **2. Enterprise MLOps** | **Kubeflow, Flyte, ClearML, MLflow** | • 엔터프라이즈 파이프라인 표준<br>

<br>• DAG 및 모델 레지스트리 완비 | • **Linux/NVIDIA CUDA 중심** 설계<br>

<br>• 맥OS VM 내부 K8s Pod로 Metal GPU 패스스루 불가<br>

<br>• Kubeflow 풀스택은 RAM 16~32GB 이상 상시 점유 |
| **3. Mac K8s / VM Engines** | **Colima, Lima, OrbStack, Docker Desktop** | • macOS `vz` 엔진으로 RAM 오버헤드 최소화<br>

<br>• K3s/Docker 원클릭 실행 | • 순수 인프라 런타임 레이어만 제공<br>

<br>• MLOps 전용 대시보드 및 MLOps 도구 스택 미내장<br>

<br>• 호스트 GPU와의 네트워크 브릿지 수동 설정 필요 |
| **4. Apple Compute Engines** | **Apple MLX, `llama-server`, `vllm-mlx**` | • 애플 실리콘 유니파이드 메모리 100% 활용<br>

<br>• PyTorch 대비 20~80% 빠른 토큰 속도 | • CLI / Python 라이브러리 형태<br>

<br>• UI가 없고 K8s 클러스터 제어 기능 부재 |

### 2. KubeMetal의 포지셔닝 (Competitive Advantage)

KubeMetal은 "카테고리 1의 유저 친화적 UX" + "카테고리 2의 표준 MLOps 파이프라인" + "카테고리 3의 경량 맥 Virtualization" + "카테고리 4의 MLX 성능"을 고유하게 통합하는 **최초의 macOS 전용 하이브리드 MLOps 솔루션**입니다.

---

# [Part 2] KubeMetal 기술 기능 명세서 (Technical Specification)

* **문서 버전**: v0.1 (MVP Spec)
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

---

## 2. 기능 요구사항 명세 (Functional Requirements)

### FR-01: 로컬 K8s 클러스터 라이프사이클 제어 (Control Plane)

* **FR-01.1**: Rust 백엔드가 CLI 프로세스로 `colima` 명령을 호출하여 macOS `Virtualization.framework` (`vz`) 및 `virtiofs` 기반 K3s 클러스터를 생성/구동/중지해야 한다.
* **FR-01.2**: 사용자의 RAM 스펙에 맞춰 K8s 가상머신 자원(CPU, RAM)을 동적으로 조절할 수 있어야 한다.
* **FR-01.3**: K8s 클러스터 상태(Running/Stopped/Creating) 및 Kubeconfig 엔드포인트를 실시간 감지하여 UI에 표출해야 한다.

### FR-02: MLOps 인프라 서비스 자동 프로비저닝

* **FR-02.1**: K8s 클러스터 정상 구동 시, Helm/Kubectl을 통해 **MLflow Tracking Server**와 **MinIO Object Storage**를 파드로 자동 배포해야 한다.
* **FR-02.2**: K8s 내 배포된 MLflow UI(Port 5000) 및 MinIO Console(Port 9001)로 포트파워딩을 자동 구성하고 프론트엔드 Webview로 내장/웹 브라우저 오픈 기능을 제공해야 한다.

### FR-03: 호스트 compute Engine (MLX) 파인튜닝 & 서빙 제어

* **FR-03.1**: macOS 호스트 환경의 Python VirtualEnv 또는 독립 바이너리를 이용해 **MLX LoRA Fine-Tuning** 스크립트를 백그라운드 프로세스로 실행할 수 있어야 한다.
* **FR-03.2**: MLX 학습 실행 시, K8s MLflow Server URI (`http://localhost:5000` 또는 내부 DNS)로 학습 파라미터, 손실(Loss) 메트릭, `.safetensors` 무게 파일을 자동 전송하도록 래핑해야 한다.
* **FR-03.3**: 학습이 완료되거나 로컬 모델 선택 시 `mlx-serve` 또는 `llama-server` 프로세스를 띄워 OpenAI 호환 REST API 엔드포인트(`http://localhost:8080/v1`)를 제공해야 한다.

### FR-04: K8s ↔ macOS Host 하이브리드 브릿지 통신

* **FR-04.1**: K8s 내부 파드가 맥OS 호스트의 MLX/Inference API에 접근할 수 있도록 `ExternalName` 서비스 (`host.lima.internal` / `host.docker.internal`)를 자동 셋업해야 한다.
* **FR-04.2**: Prefect Host Worker 데몬을 호스트에 상주시켜 K8s Prefect Server로부터 전달받은 `gpu:mlx` 스케줄링 태스크를 수신 및 실행해야 한다.

### FR-05: 하드웨어 모니터링 및 안전 가드레일 (Hardware Guardrails)

* **FR-05.1**: `sysinfo` 및 macOS `powermetrics` C-Binding을 호출해 **Metal GPU 점유율**, 유니파이드 메모리 압박도(Memory Pressure)를 1초 주기 스트리밍으로 측정해야 한다.
* **FR-05.2 (OOM Protection)**: 가용 RAM이 전체의 10% 이하로 떨어지면 진행 중인 MLX 학습 프로세스를 일시정지(Pause)하고 사용자에게 대화상자 경고를 출력해야 한다.
* **FR-05.3 (Power/Thermal Guard)**: 배터리 구동 감지 시 학습 일시정지 옵션을 제공하며, 학습 중 슬립 모드 진입을 방지하기 위해 `caffeinate` 어서션을 실행해야 한다.

---

## 3. 비기능 요구사항 명세 (Non-Functional Requirements)

* **NFR-01 (자원 효율성)**: KubeMetal 데스크톱 앱(Tauri v2) 자체의 유휴(Idle) 상태 RAM 점유율은 **50MB 이하**여야 한다.
* **NFR-02 (응답 속도)**: Tauri Rust IPC 통신의 지연 시간은 **10ms 이내**여야 하며, UI 메트릭 갱신 주기는 1000ms(1초)를 유지한다.
* **NFR-03 (재현성 & Portability)**: 앱 삭제 시 K8s 가상머신 및 볼륨을 깨끗이 정리하는 `Clean Uninstall` 로직을 지원해야 한다.

---

## 4. API 및 데이터 인터페이스 명세 (Interface Spec)

### 4.1 Tauri Rust Commands (Frontend ↔ Backend IPC)

| Command 이름 | Input Parameters | Output Return | 설명 |
| --- | --- | --- | --- |
| `get_system_metrics` | None | `SystemMetricsJSON` | RAM, CPU, Metal GPU 실시간 사용량 리턴 |
| `start_colima_cluster` | `{ cpu: u32, memory: u32 }` | `Result<String, String>` | Colima `vz` K8s 클러스터 구동 |
| `get_cluster_status` | None | `ClusterStatusJSON` | K8s 및 MLflow/MinIO 파드 헬스체크 리턴 |
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
  ports:
  - name: mlx-serve
    port: 8080
    targetPort: 8080

```

---