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

* **문서 버전**: v0.3 — 사용자 피드백("설치가 아니라 유기적 연동") 반영, FR-06~FR-09 신설 (Phase 2)
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

* **FR-02.1**: K8s 클러스터 정상 구동 시, Helm/Kubectl을 통해 **MLflow Tracking Server**와 **SeaweedFS Object Storage**를 파드로 자동 배포해야 한다. 스택의 정식 배포 대상은 자체 k3s(colima)다 — 외부 클러스터의 기본 통합 수준은 에이전트 온리이며, 풀스택 외부 배포는 전제조건을 갖춘 옵트인 경로다(D26/D30).
* **FR-02.2**: K8s 내 배포된 MLflow UI 및 SeaweedFS Filer UI/S3 API로 아래 포트를 호스트에 자동 포트포워딩 구성하고, 프론트엔드 Webview로 내장/웹 브라우저 오픈 기능을 제공해야 한다. 포트포워딩은 앱이 관리하는 `kubectl port-forward` 자식 프로세스로 구현하며(`start_port_forward`/`stop_port_forward`), 클러스터 중지 또는 앱 종료 시 해당 프로세스를 정리해야 한다.
  * MLflow UI: 호스트 **5001번 포트** (macOS AirPlay Receiver가 기본 5000번 포트를 점유하므로 5000 사용 금지)
  * SeaweedFS Filer UI: 호스트 **8888번 포트**
  * SeaweedFS S3 API: 호스트 **8333번 포트**

### FR-03: 호스트 Compute Engine (MLX) 파인튜닝 & 서빙 제어

* **FR-03.1**: macOS 호스트 환경의 Python VirtualEnv 또는 독립 바이너리를 이용해 **MLX LoRA Fine-Tuning** 스크립트를 백그라운드 프로세스로 실행할 수 있어야 한다.
* **FR-03.2**: MLX 학습 실행 시, K8s MLflow Server URI (`http://localhost:5001` 또는 내부 DNS)로 학습 파라미터, 손실(Loss) 메트릭, `.safetensors` 무게 파일을 자동 전송하도록 래핑해야 한다.
* **FR-03.3**: 학습이 완료되거나 로컬 모델 선택 시 `mlx_lm.server`(텍스트) 또는 `mlx_vlm.server`(비전+텍스트, D29) 프로세스를 띄워 OpenAI 호환 REST API 엔드포인트(기본값 `http://127.0.0.1:8080/v1`)를 제공해야 한다. 두 서버 모두 `--host 127.0.0.1`을 명시한다 — mlx_vlm.server의 기본 host는 0.0.0.0이다(실측). 런타임 선택은 `MlxRuntime`으로 전달되며 미지정 시 mlx-lm이다.

### FR-04: K8s ↔ macOS Host 하이브리드 브릿지 통신

* **FR-04.1**: K8s 내부 파드가 macOS 호스트의 MLX/Inference API에 접근할 수 있도록 `ExternalName` 서비스(`host.lima.internal`, Colima/vz는 Lima 기반이므로 Docker Desktop 전용 호스트 별칭이 아닌 이 이름을 사용한다)를 자동 셋업해야 한다. `ExternalName` 서비스는 CNAME 별칭만 제공하며 포트를 프록시하지 않으므로, 파드는 `host.lima.internal:8080`처럼 대상 포트를 직접 지정해 접근해야 한다 (상세 매니페스트는 [4.2](#42-k8s-external-service-manifest-spec-mac-gpu-bridgeyaml) 참조).
* **FR-04.2**: Prefect Host Worker 데몬을 호스트에 상주시켜 K8s Prefect Server로부터 전달받은 `gpu:mlx` 스케줄링 태스크를 수신 및 실행해야 한다.

### FR-05: 하드웨어 모니터링 및 안전 가드레일 (Hardware Guardrails)

* **FR-05.1 (Phase 1 · MVP)**: `sysinfo` 크레이트를 사용해 RAM 사용량과 CPU 사용률을 1초 주기로 측정하여 스트리밍해야 한다.
* **FR-05.4 (Phase 4 · 구현됨, 2026-07-24 개정)**: Metal GPU 점유율/메모리는 sudo 없이 접근 가능한 `ioreg -l -d 1 -r -c IOAccelerator`(D16과 동일한 sudo-free 원칙) 출력을 파싱하여 `get_system_metrics`의 `gpu_usage_percentage`/`gpu_memory_used_gb` 필드로 제공한다. root 권한이 필요한 `powermetrics` CLI 기반 측정 및 별도 privileged helper는 채택하지 않으며(D2), 앞으로도 사용하지 않는다.
* **FR-05.2 (OOM Protection · Phase 3)**: macOS의 **memory pressure 레벨**(`warn` 또는 `critical`)이 감지되면 진행 중인 MLX 학습 프로세스를 일시정지(Pause)하고 사용자에게 대화상자 경고를 출력해야 한다. macOS는 파일 캐시로 인해 가용 RAM 비율만으로는 상시 오탐이 발생하므로, 단순 "가용 RAM 10% 이하"와 같은 비율 임계값을 트리거 기준으로 사용해서는 안 된다.
* **FR-05.3 (Power/Thermal Guard · Phase 3)**: 배터리 구동 감지 시 학습 일시정지 옵션을 제공하며, 학습 중 슬립 모드 진입을 방지하기 위해 `caffeinate` 어서션을 실행해야 한다.

### FR-06: 서비스 연동 자동 구성 (Phase 2a)

> 배경: OSS를 개별 설치하는 것만으로는 사용자 편의성이 없다. 설치된 서비스 간 연동까지 앱이 자동으로 구성해야 "유기적 연동"이 성립한다.

* **FR-06.1**: `provision_mlops_stack` 실행 시, MLflow Tracking Server의 artifact store를 SeaweedFS S3 API로 자동 와이어링해야 한다. Endpoint는 `http://seaweedfs:8333`(K8s 서비스 DNS 기준), 버킷명은 `mlflow`로 고정하며, 더미 액세스 키/시크릿 키는 K8s 매니페스트의 환경변수(`AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY` 또는 SeaweedFS 대응 변수)로 주입한다.
* **FR-06.2**: SeaweedFS `mlflow` 버킷이 존재하지 않으면 프로비저닝 과정에서 자동 생성해야 한다.
* **FR-06.3**: `get_cluster_status`는 MLflow↔SeaweedFS 연동 상태(예: `artifact_store_linked: bool`)를 추가로 리턴해, UI가 "배포됨"과 "연동됨"을 구분해 표시할 수 있어야 한다.

### FR-07: 모델 허브 (Phase 2b)

* **FR-07.1**: Hugging Face Hub API를 통해 모델을 검색하고 목록(이름, 크기, 태그)을 UI에 표출해야 한다.
* **FR-07.2**: 선택한 모델을 호스트 파일시스템으로 다운로드해야 한다.
* **FR-07.3**: 다운로드된 모델 아티팩트를 SeaweedFS S3 API(8333)로 업로드해야 한다.
* **FR-07.4**: 업로드된 모델을 MLflow Model Registry에 등록해야 한다(FR-06 연동 전제).
* **FR-07.5**: 등록된 모델의 목록/상세(버전, 스토리지 경로, 등록 상태)를 조회하는 UI를 제공해야 한다.

### FR-08: 파이프라인 가시화 (Phase 2c)

* **FR-08.1**: 전처리 → 학습(Host MLX) → 등록(K8s MLflow) → 서빙(Host) 단계별 상태를 카드 또는 그래프 형태로 시각화해야 한다.
* **FR-08.2**: Prefect 미도입 구간(Phase 2c)에서는 앱 내부에서 관리하는 오케스트레이션 상태(각 단계의 진행/완료/실패)만 표시하며, Prefect 기반 DAG 시각화는 Prefect 도입 이후로 범위를 명시적으로 분리한다.

### FR-09: 통합 접근 (Phase 2d)

> 배경: 로컬 단일 사용자 맥락에서는 Keycloak 같은 IdP 도입이 과도할 수 있다. "SSO"의 의도(서비스별 로그인 없이 한 번에 접근)를 로컬 환경에 맞게 재정의한다.

* **FR-09.1**: 로컬 단일 사용자 컨텍스트에서 "SSO"는 **서비스 크리덴셜 자동 프로비저닝 + 앱에서 원클릭 인증 접근**(임베디드 웹뷰 또는 브라우저 오픈, 크리덴셜 자동 주입)으로 정의한다.
* **FR-09.2**: 각 서비스(MLflow UI, SeaweedFS Filer UI 등) 접근에 필요한 크리덴셜은 프로비저닝 시 앱이 자동 발급/저장하며, 사용자가 별도로 로그인 절차를 수행하지 않아야 한다.
* **FR-09.3**: Keycloak 등 정식 IdP 기반 SSO는 멀티유저 지원 또는 원격/클러스터 확장 시나리오에서 별도 검토 항목으로 명시하며, 본 Phase 2d 범위에는 포함하지 않는다.

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
| `get_system_metrics` | None | `SystemMetricsJSON` | RAM/CPU 실시간 사용량 + Metal GPU 사용률/메모리(`ioreg` 기반, Phase 4, FR-05.4) + 발열 압력(`thermal_state`, `NSProcessInfo` 기반 — CLI 경로가 전부 비어 있음을 실측, D2 개정) 리턴. 발열은 읽기 실패 시 null이며 "정상"으로 폴백하지 않는다 |
| `get_cluster_status` | None | `ClusterStatusJSON` | colima 상태 + MLflow/SeaweedFS 배포 준비 여부(`mlflow_ready`/`seaweedfs_ready`) 리턴 |
| `start_cluster` | `{ cpu: u32, memory: u32 }` | `Result<String, String>` | Colima `vz` K8s 클러스터 구동 |
| `stop_cluster` | None | `Result<String, String>` | Colima K8s 클러스터 중지 |
| `provision_mlops_stack` | None | `Result<String, String>` | 활성 배포 대상(D26)에 MLOps 스택 적용. 매니페스트는 `scripts/k8s/render.sh`가 대상에 맞춰 렌더링하고(ns/브리지/StorageClass/레지스트리) 결과를 stdin으로 `kubectl apply`에 흘린다. 브리지가 미검증이면 렌더 전에 Err (FR-02.1) |
| `get_deploy_target` (D26) | None | `Result<DeployTarget, String>` | 저장된 배포 대상을 읽는다. 없으면 colima 기본값 — 기존 사용자의 동작이 바뀌지 않는다. 읽는 즉시 활성 (context, namespace) 캐시에 반영해 포트포워드·크리덴셜 조회 경로가 따라간다 |
| `save_deploy_target` (D26) | `{ target: DeployTarget }` | `Result<DeployTarget, String>` | 대상을 앱 설정 디렉터리(`deploy-target.json`)에 저장하고 활성 캐시를 갱신 |
| `preflight_deploy_target` (D26) | `{ context: String, namespace: String }` | `Result<PreflightReport, String>` | 대상 클러스터 사전점검을 **전부 실측**으로 수행: 노드/InternalIP, StorageClass와 기본값 유무, ArgoCD CRD 및 대상 ns를 소유한 Application, Kyverno Enforce 정책 목록, 브리지 후보. 배포를 막아야 할 사유는 `blockers`로 올린다(기본 SC 없음 / ArgoCD 소유 / 브리지 후보 없음) |
| `detect_host_bridge` (D26, D10 개정) | `{ context: String, namespace: String }` | `Result<BridgeState, String>` | 2단계 탐지. (1) `ifconfig -a`를 파싱해 노드 서브넷과 같은 대역의 **호스트 인터페이스 자기 주소**를 후보로 낸다 — `route -n get`은 쓰지 않는다(인터페이스가 내려가 있으면 기본 경로로 폴백해 LAN 공유기 주소를 호스트로 반환한다, 실측). (2) 후보 주소에만 임시 리스너를 바인드하고 클러스터 안에 프로브 파드를 띄워 도달을 확인한다 — 검증 주체는 호스트가 아니라 **클러스터**여야 한다. 성공한 후보만 `Verified`, 전부 실패하면 사유와 함께 `Unverified`(배포 거부 상태). colima는 D10 실측값이 있어 `KeepBase`로 즉시 반환 |
| `start_port_forward` | None | `Result<String, String>` | MLflow(5001) · SeaweedFS(8333/8888) `kubectl port-forward` 자식 프로세스 기동 (FR-02.2) |
| `stop_port_forward` | None | `Result<String, String>` | 추적 중인 포트포워드 자식 프로세스 종료 (FR-02.2) |
| `search_hf_models` (Phase 2b) | `{ query: String, limit: u32 }` | `Result<Vec<HfModel>, String>` | Hugging Face Hub API로 모델 검색 (FR-07.1) |
| `download_hf_model` (Phase 2b) | `{ repo_id: String }` | `Result<String, String>` | 모델을 `~/.kubemetal/models/`로 비동기 다운로드 시작 (FR-07.2) |
| `get_model_downloads` (Phase 2b) | None | `Result<Vec<DownloadStatus>, String>` | 진행 중/완료된 다운로드 상태 조회 |
| `list_local_models` (Phase 2b) | None | `Result<Vec<LocalModel>, String>` | 로컬에 다운로드된 모델 목록 조회 |
| `upload_model_to_storage` (Phase 2b) | `{ repo_id: String }` | `Result<String, String>` | 로컬 모델을 SeaweedFS S3(8333) `models` 버킷으로 업로드 (FR-07.3) |
| `register_model_mlflow` (Phase 2b) | `{ repo_id: String }` | `Result<String, String>` | 업로드된 모델을 MLflow Model Registry(5001)에 등록 (FR-07.4) |
| `check_mlx_env` (Phase 2c) | None | `MlxEnvStatus {python_ok, venv_exists, mlx_lm_installed, mlx_lm_version}` | `~/.kubemetal/venv`의 python3/mlx-lm 설치 여부·버전 조회 |
| `setup_mlx_env` (Phase 2c) | None | `Result<String, String>` | 백그라운드로 `python3 -m venv` + `pip install -U mlx-lm` 실행, 진행 상태를 `MlxState.env_setup`에 기록 |
| `run_mlx_finetune` (Phase 2c) | `FineTuneConfig {model_path, data_path, iters: u32, batch_size: u32, learning_rate: f64, adapter_name, runtime?: MlxRuntime}` | `Result<u32, String>` | venv python으로 `scripts/mlx/finetune_wrapper.py`(mlx_lm lora 래퍼) 실행, PID 리턴. model_path/data_path는 홈 디렉터리 하위 실존 경로만 허용 |
| `get_mlx_status` (Phase 2c) | None | `MlxStatus {env, env_setup_state, env_setup_error, training: Option<TrainingStatus>, serving: Option<ServingStatus>, last_serving_error: Option<String>}` | 환경/학습/서빙 상태 통합 조회. `last_serving_error`는 서빙 프로세스가 사용자 요청 없이 예기치 않게 종료됐을 때 exit code/stderr 요약을 담는다 |
| `kill_mlx_process` (Phase 2c) | `{ pid: u32 }` | `Result<bool, String>` | 실행 중인 MLX 학습/서빙 프로세스에 SIGTERM→(1s)→SIGKILL 전송, State 정리 |
| `start_model_serving` (Phase 2c) | `{ model_path: String, adapter_path: Option<String>, port: u16, runtime: Option<MlxRuntime> }` | `Result<String, String>` | 실기기 실측(2026-07-21): 진입 시 `TcpListener::bind(("127.0.0.1", port))`로 포트 선점 여부를 먼저 확인해 실패하면 spawn 없이 Err(다른 프로세스가 사용 중인 포트 안내, 예: 8080이 Tomcat 등에 선점된 경우). `model_path`가 `adapter_config.json`을 가진 어댑터 디렉터리로 판정되면 그 파일의 `model` 필드에서 베이스 모델 경로를 읽어 자동 승격(베이스를 `--model`, 해당 디렉터리를 `--adapter-path`)하고, 필드가 없으면 Err("베이스 모델을 함께 지정하세요"). 그렇지 않으면 `model_path`를 `--model`로, `adapter_path`가 있으면 `--adapter-path`로 넘겨 venv `python -m mlx_lm server` 기동. 이미 서빙 중이면 Err. 기동한 자식 프로세스는 백그라운드 태스크(`run_serving_reader`)가 `wait()`하며, `stop_model_serving`/`kill_mlx_process`로 의도적으로 멈춘 경우가 아니면 종료 시 `last_serving_error`에 원인을 기록 |
| `stop_model_serving` (Phase 2c) | None | `Result<String, String>` | 진행 중인 모델 서빙 프로세스 종료 |
| `suggest_serving_port` (Phase 2c) | None | `Result<u16, String>` | 8080~8099 범위를 순회하며 `TcpListener::bind(("127.0.0.1", p))`가 성공하는 첫 포트를 리턴(즉시 drop, 제안값일 뿐 예약은 아님). 모두 실패하면 Err. 모델 서빙 카드가 마운트 시 1회 호출해 포트 입력 초기값으로 사용(실패 시 8080 유지, 사용자가 직접 수정하면 그 값을 존중) |
| `list_registered_models` (Phase 2c) | None | `Result<Vec<RegisteredModel>, String>` | MLflow Model Registry(5001)의 `registered-models/search` 결과를 `{name, latest_version, last_updated_ms}`로 매핑해 파이프라인 뷰의 "등록" 단계에 노출 (FR-08) |
| `get_service_access` (Phase 2d) | `State<MlxState>`(암묵) | `Result<Vec<ServiceAccess>, String>` | MLflow(`localhost:5001`)·SeaweedFS S3(`localhost:8333`)·SeaweedFS Filer(`localhost:8888`) 3종은 `curl -w %{http_code}`(000이면 unreachable)로 헬스를 판정한다. 실기기 실측(2026-07-21): mlx_lm server는 IPv4(127.0.0.1)에만 bind하는데 `localhost`는 이 머신에서 `::1`(IPv6)로 먼저 풀리고 Tomcat 등 무관한 프로세스가 IPv6 8080을 선점할 수 있어, TCP 응답만으로 "ok" 판정하면 정상 서버를 unreachable로 오판하거나(포트 미점유 시) 반대로 무관한 프로세스의 404 응답을 서빙 헬시로 오판한다(포트 점유 시). Model Serving은 하드코딩 포트 대신 `MlxState.serving`에서 실제 기동 포트를 조회해 `http://127.0.0.1:{port}/v1`을 구성하고 `127.0.0.1` 고정 + `/v1/models` HTTP 200일 때만 ok로 판정(`check_serving_health`); 서빙이 기동돼 있지 않으면 health `unreachable` + `credential_hint`로 "서빙이 실행 중이 아닙니다 — MLX 스튜디오 탭에서 서빙을 시작하세요."를 안내하고 `url`은 빈 문자열(프런트가 원클릭 접근 버튼을 숨김). SeaweedFS S3 크리덴셜(`kubectl get secret seaweedfs-s3-credentials -o json` base64 디코드)도 함께 조회해 접근 콘솔에 노출 (FR-09) |
| `get_guardrail_status` (Phase 3) | None | `GuardrailStatus {memory_pressure_level, on_battery, battery_pause_enabled, training_paused, caffeinate_active}` | `sysctl -n kern.memorystatus_vm_pressure_level`(D16)과 `pmset -g batt` 실측 기반 하드웨어 가드레일 상태 조회 (FR-05.2/05.3) |
| `set_guardrail_config` (Phase 3) | `{ battery_pause: bool }` | `Result<(), String>` | 배터리 구동 시 학습 자동 일시정지 여부를 `GuardrailState`에 저장 (FR-05.3) |
| `pause_mlx_training` (Phase 3) | None | `Result<bool, String>` | 진행 중인 MLX 학습 pid에 SIGSTOP 전송, `TrainingStatus.status`를 `paused`로 갱신 |
| `resume_mlx_training` (Phase 3) | None | `Result<bool, String>` | 일시정지된 MLX 학습 pid에 SIGCONT 전송, `TrainingStatus.status`를 `running`으로 갱신 |
| `get_prefect_status` (Phase 4a/4b) | None | `PrefectStatus {server_ready, env_installed, eval_env_installed, runner_running, runner_pid, recent_runs: Vec<FlowRunInfo{id,name,state_type,state_name}>}` | `kubectl get deploy prefect -o json`의 `status.availableReplicas>0`(colima.rs 패턴)로 `server_ready`, venv `python -c "import prefect"`로 `env_installed`, venv `python -c "import lm_eval"`로 `eval_env_installed`(Phase 4b, D20) 판정. `server_ready`일 때만 Prefect REST `POST /flow_runs/filter`(`{"limit":5,"sort":"START_TIME_DESC"}`)로 `recent_runs` 최신 5건 조회, 실패 시 빈 배열 |
| `setup_prefect_env` (Phase 4a) | None | `Result<String, String>` | 기존 MLX venv(~/.kubemetal/venv, D15)에 백그라운드로 `pip install -U prefect` 실행, 진행 상태를 `PrefectState.env_setup`에 기록(`setup_mlx_env`와 동일 패턴). venv 자체가 없으면(=`setup_mlx_env` 미실행) 즉시 Err |
| `start_prefect_runner` (Phase 4a) | None | `Result<String, String>` | venv python으로 번들 리소스 `scripts/prefect/host_runner.py`를 `process_group(0)`으로 스폰(D17과 동일 원칙 — 내부에서 띄우는 `finetune_wrapper.py`/`mlx_lm` 자식까지 그룹 상속), `PREFECT_API_URL=http://127.0.0.1:4200/api` 주입, PID를 `PrefectState.runner_pid`에 기록. reaper 태스크가 예기치 않은 종료 시 `PrefectState.last_runner_error`에 원인 기록 |
| `stop_prefect_runner` (Phase 4a) | None | `Result<String, String>` | `runner_pid`가 이끄는 프로세스 그룹 전체에 SIGTERM→(1s)→SIGKILL 전송(D17), State 정리 |
| `trigger_finetune_flow` (Phase 4a) | `FineTuneConfig {model_path, data_path, iters, batch_size, learning_rate, adapter_name}`(mlx.rs와 동일 타입 재사용) | `Result<String, String>` | `validate_home_subpath`/`validate_adapter_name`(mlx.rs 재사용, D15) 검증 후 Prefect REST `GET /deployments/name/finetune/finetune`로 deployment id 조회 → `POST /deployments/{id}/create_flow_run`(body `{"parameters": {...}}`)로 flow run 생성, flow run id 리턴. 실기기 실측(2026-07-23): 5-iter 스모크 학습이 이 경로로 완주(state COMPLETED)함을 확인 |
| `setup_eval_env` (Phase 4b) | None | `Result<String, String>` | 기존 MLX venv에 백그라운드로 `pip install -U "lm-eval[api]"` 실행(`api` extra 필수 — 실기기 실측 2026-07-23, 없으면 `local-completions` 모델 타입이 `tenacity` 미설치로 즉시 실패), 진행 상태를 `PrefectState.eval_env_setup`에 기록(`setup_prefect_env`와 동일 패턴, venv 자체가 없으면 즉시 Err). D20 |
| `trigger_evaluate_flow` (Phase 4b) | `{ tasks: String, limit: u32, serving_port: u16 }` | `Result<String, String>` | `serving_url = http://127.0.0.1:{serving_port}/v1` 구성(mlx_lm.server는 IPv4 전용이라 `127.0.0.1` 고정) 후 Prefect REST `GET /deployments/name/evaluate/evaluate`로 deployment id 조회 → `POST /deployments/{id}/create_flow_run`(body `{"parameters": {"serving_url", "tasks", "limit"}}`)로 flow run 생성, flow run id 리턴. 실기기 실측(2026-07-23): tasks=gsm8k, limit=4로 트리거해 flow run이 COMPLETED로 완주함을 확인. D20 |
| `get_eval_results` (Phase 4b) | None | `Result<Vec<EvalMetric>, String>` | MLflow REST `experiments/get-by-name`(`kubemetal-eval`) → `runs/search`(`max_results:10`, `order_by:["attribute.start_time DESC"]`)로 최근 10 run을 조회해 `EvalMetric {run_id, task, metric, value, timestamp_ms}`로 평탄화(`host_runner.py`가 남기는 `"task/metric/filter"` 메트릭 키의 첫 `/`를 기준으로 task/metric 분리). experiment 미생성·MLflow 연결 실패 시 빈 배열. D20 |
| `get_rag_status` (Phase 4c) | `State<RagState>`(암묵) | `Result<RagStatus {env_installed, env_setup: EnvSetupStatus, indexed_collections: Vec<String>}, String>` | venv `python -c "import lancedb; import sentence_transformers"` 성공 여부로 `env_installed`, `RagState.env_setup`으로 `setup_rag_env` 진행 상태를, LanceDB 디렉터리의 `*.lance` 서브디렉터리를 스캔해 `indexed_collections`를 조회 |
| `setup_rag_env` (Phase 4c) | None | `Result<String, String>` | 기존 MLX venv(D15)에 백그라운드로 `pip install -U lancedb sentence-transformers "dvc[s3]"` 실행, 진행 상태를 `RagState.env_setup`에 기록(`setup_mlx_env`와 동일 패턴). venv 자체가 없으면 즉시 Err |
| `index_documents` (Phase 4c) | `{ docsPath: String, collectionName: Option<String>, embeddingModel: Option<String> }` | `Result<IndexResult {status, collection, indexed_docs, total_chunks, db_path}, String>` | venv python으로 번들 리소스 `scripts/rag/rag_host.py index` 실행 — `docs_path` 하위 문서를 청킹 후 sentence-transformers로 임베딩해 LanceDB 컬렉션에 저장 |
| `query_rag` (Phase 4c) | `{ query: String, collectionName: Option<String>, topK: Option<u32>, embeddingModel: Option<String> }` | `Result<Vec<RagSearchResult {text, filename, source, chunk_index, score}>, String>` | venv python으로 `rag_host.py query` 실행 — 질의를 임베딩해 LanceDB 벡터 검색, 상위 K개 청크를 리턴 |
| `dvc_commit_dataset` (Phase 4c) | `{ dataPath: Option<String>, bucketName: Option<String>, commitMessage: Option<String> }` | `Result<String, String>` | venv python으로 `rag_host.py dvc-commit` 실행 — 대상 디렉터리를 DVC `init --no-scm` + `add` + SeaweedFS S3 리모트로 `push`. S3 크리덴셜은 CLI 인자가 아니라 `KUBEMETAL_S3_ACCESS_KEY`/`KUBEMETAL_S3_SECRET_KEY` 환경변수로 자식 프로세스에 주입한다(D21, ps 노출 방지) |
| `get_dvc_status` (Phase 4c) | None | `Result<DvcStatus {initialized, remote_url: Option<String>, current_tag: Option<String>, dataset_path: Option<String>, tags: Vec<DvcVersionTag>, last_error: Option<String>}, String>` | LanceDB 디렉터리의 `.dvc` 존재 여부로 `initialized`, `.dvc/config`의 `endpointurl` 값을 파싱해 `remote_url`, `git for-each-ref refs/tags`로 `tags`/`current_tag`를 실측 조회한다. 파이프라인의 `dvc init`은 항상 `--no-scm`(git 미생성)이므로 실제 운용에서 `tags`는 통상 빈 배열이 정상이며, 조회 불가 항목은 하드코딩 대신 정직하게 `None`/빈 값을 리턴한다 |
| `run_data_ingest` (Phase 5a) | `{ config: IngestConfig }` — `IngestConfig`는 camelCase 필드(`sourceType, sourcePath, collectionName?, embeddingModel?, chunkSize?, chunkOverlap?, enableDvcBackup?, dvcRemoteUrl?, dvcBucket?`) | `Result<IngestFlowResult, String>` | 번들 리소스 `scripts/data/ingest_host.py`를 venv python으로 실행 — extract → clean_chunk → lancedb_index → dvc_backup 4-노드 DAG를 순차 실행해 노드별 상태(`DagNodeState {node_id, name, status, duration_sec, items_processed, details}`)를 포함한 `IngestFlowResult`를 리턴. `source_type`이 web/rss면 Rust(`validate_ingest_url`)와 Python(`ingest_host.py::_validate_url`) 양쪽에서 scheme allowlist(http/https만)+사설/루프백 호스트 거부(D21) 이중 검증을 수행한다. DVC 백업 크리덴셜은 `dvc_commit_dataset`과 동일하게 env var로 주입 |
| `get_ingest_status` (Phase 5a) | `State<DataIngestState>`(암묵) | `Result<IngestStatusResponse {env_installed, default_db_path, active_collections: Vec<IngestedDatasetInfo>, last_result: Option<IngestFlowResult>}, String>` | venv 존재 여부로 `env_installed`, LanceDB 디렉터리 스캔으로 `active_collections`, 가장 최근 `run_data_ingest` 결과를 `DataIngestState.last_result`에서 조회 |
| `list_ingested_datasets` (Phase 5a) | None | `Result<Vec<IngestedDatasetInfo {collection_name, total_chunks, db_path, is_lance_table}>, String>` | LanceDB 디렉터리를 스캔해 `.lance` 서브디렉터리(테이블) 및 `_fallback.json`(lancedb 패키지 미설치 시 폴백) 파일을 데이터셋으로 나열 |
| `get_hardware_spec` (Phase 5b) | None | `Result<HardwareSpec {brand_name, cpu_cores, total_memory_gb, gpu_cores: Option<u32>}, String>` | `external_command("sysctl")`(D5, 비블로킹)로 `machdep.cpu.brand_string`/`hw.ncpu`/`hw.memsize`를 조회하고 `system_profiler SPDisplaysDataType`에서 GPU 코어 수를 파싱한다. 프로세스 수명 동안 1회만 조회 후 캐시(`system_profiler`가 수 초 소요). sysctl 조회 실패는 Err, GPU 파싱 실패만 `None` — 특정 기종 스펙을 폴백으로 채우지 않는다(D22) |
| `list_kubeconfig_contexts` (Phase 5b) | None | `Result<Vec<String>, String>` | `kubectl config get-contexts -o name` 결과를 그대로 반환. 실패 시 Err — 존재하지 않는 컨텍스트를 폴백으로 생성하지 않는다(D22) |
| `get_kagent_diagnostics` (Phase 5b) | `{ context: Option<String> }`(기본 `colima`) | `Result<KagentDiagnosticReport {target_context, kagent_ready, pod_issues_count, recent_diagnosis, recommended_action, active_agents, available_agents}, String>` | `kubectl get pods -n kagent`/`-n default -o json` 2회 실측. `kagent_ready`는 kagent ns 파드가 1개 이상이면서 **전부 Ready condition=True**일 때만 true(phase만 보면 CrashLoopBackOff도 Running으로 보인다). `active_agents`는 Ready 파드의 `app.kubernetes.io/name` 라벨에서만 수집하고, `recent_diagnosis`는 클러스터가 준 `waiting.reason`/`message`만 인용한다. `available_agents`는 `toggle_kagent_agent`가 실제로 설치 가능한 3종. 조회 실패는 Err(정상으로 폴백하지 않는다) — D22 |
| `toggle_kagent_agent` (Phase 5b) | `{ agentName: String, enable: bool, context: Option<String> }` | `Result<String, String>` | `enable`이면 내장 Agent CRD 매니페스트(security/promql/observability)를 임시 파일로 써 `kubectl apply`, 아니면 `kubectl delete agent.kagent.dev`. 지원 목록 밖의 이름은 Err. 반환 문자열은 kubectl stdout 인용이며 파드 기동 상태를 주장하지 않는다 — 실제 상태는 `get_kagent_diagnostics` 재조회로 확인(D22) |
| `get_airgap_status` (Phase 5b) | None | `Result<AirgapStatusReport {airgap_dir, total_assets_count, downloaded_count, total_size_mb, assets: Vec<AirgapAssetItem {.., exists, size_mb, size_bytes, corrupt}>}, String>` | `~/.kubemetal/airgap` 하위의 바이너리/차트/이미지 파일 크기를 실측 조회(`.tar.gz` 우선, 없으면 `.tar` 폴백). 존재만으로 보유로 세지 않는다 — 1KiB 미만은 `corrupt`로 분류해 `downloaded_count`/`total_size_mb`에서 제외하고 `size_bytes`로 실제 크기를 노출한다(다운로드 실패 응답이 저장된 경우). D23 |
| `trigger_airgap_download` (Phase 5b) | `AppHandle`(암묵) | `Result<String, String>` | 번들 리소스 `scripts/airgap/download_airgap_bundle.sh`를 `resolve_bundled_resource`(D18)로 해석해 실행. **상대경로 금지** — `.app`의 CWD는 프로젝트 루트가 아니다. 스크립트가 부분 실패하면 Err, 성공 시 스크립트 마지막 출력 줄을 그대로 반환(D23) |
| `trigger_airgap_install` (Phase 5b) | `AppHandle`(암묵) | `Result<String, String>` | 같은 규약으로 `scripts/airgap/install_from_airgap.sh` 실행(D18/D23) |
| `check_latest_airgap_versions` (Phase 5b) | None | `Result<Vec<AirgapLatestVersionReport {name, current_version, latest_version, has_update}>, String>` | GitHub Releases API(kagent/k3s/kubescape)를 curl로 조회. 태그는 접두 `v`와 빌드 메타데이터(`+k3s1`)를 제거해 보유 버전과 같은 축으로 비교한다(정규화 없이는 k3s가 상시 "업데이트 있음"이 된다). 조회 실패 시 `latest_version="조회 실패"` + `has_update=false` — 실패를 "최신"으로 표시하지 않는다(D23) |

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
