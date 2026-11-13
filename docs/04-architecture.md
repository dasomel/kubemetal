# 04. 전체 아키텍처

KubeMetal의 4계층 아키텍처, IPC 커맨드 흐름, 포트 맵, K8s↔호스트 브릿지를 정리합니다.
설계 결정(D1~D12)의 canonical 출처는 [docs/03-mvp-design.md §4](03-mvp-design.md#4-설계-결정-및-주의사항)이며,
본 문서는 그 결정을 인용만 하고 재서술하지 않습니다.

## 1. 전체 아키텍처 (4계층)

```mermaid
flowchart TB
    subgraph UI["UI Layer — Tauri v2 + React/TypeScript (5개 탭)"]
        A["대시보드 / 모델 허브 / MLX 스튜디오 / 파이프라인 / 접근 콘솔"]
    end

    subgraph AGENT["Control Agent — macOS Native (Rust)"]
        B["tauri::command 핸들러 (24종)<br/>colima / metrics / provision / port_forward /<br/>modelhub / mlx / guardrails / access"]
    end

    subgraph K8S["K8s Control Plane — Colima(vz) + K3s"]
        C1["MLflow Pod (5000)"]
        C2["SeaweedFS Pod (S3 8333 / Filer 8888)"]
        C3["mac-gpu-service (ExternalName)"]
        C4["seaweedfs-s3-credentials Secret"]
    end

    subgraph COMPUTE["Compute Engine — macOS Host (MLX)"]
        D1["MLX LoRA Fine-Tuning (venv + finetune_wrapper.py)"]
        D2["mlx_lm.server 서빙 (기본 :8080)"]
        D3["가드레일 루프 + caffeinate"]
    end

    A -- "Tauri IPC (invoke)" --> B
    B -- "colima / kubectl CLI" --> K8S
    C3 -. "host.lima.internal:8080" .-> COMPUTE
    B -- "host process spawn (process_group)" --> COMPUTE
    D3 -. "SIGSTOP/SIGCONT/SIGTERM (그룹 전체)" .-> D1
```

- **UI Layer**: React/TS 프론트엔드, 5개 탭(대시보드/모델 허브/MLX 스튜디오/파이프라인/
  접근 콘솔)으로 구성. `useColima`, `useMetrics` 등 훅으로 백엔드 IPC 커맨드를 호출.
- **Control Agent**: Rust `tauri::command` 계층. Colima/K3s 라이프사이클, 시스템 메트릭,
  매니페스트 프로비저닝, 포트포워딩, 모델 허브, MLX 파인튜닝/서빙, 하드웨어 가드레일,
  서비스 접근 콘솔을 담당(24개 커맨드, §2 참고).
- **K8s Control Plane**: Colima(`vz` + `virtiofs`) 위 K3s. MLflow/SeaweedFS 파드,
  SeaweedFS S3 크리덴셜 Secret, `mac-gpu-service` ExternalName이 존재한다. **현재 검증된
  기본 경로에서는** GPU 연산 워크로드를 K3s 내부에서 실행하지 않고 컨트롤 플레인 역할에
  집중한다.
- **Compute Engine**: 현재 기본 구현은 macOS 호스트에서 직접 실행되는 MLX 프로세스다.
  파인튜닝 래퍼는 `process_group(0)`으로 기동되어 가드레일 루프가 SIGSTOP/SIGCONT를
  그룹 전체로 보내도 자식 학습 프로세스까지 함께 멈춘다(D17). 추가 backend는 기존 경로를
  대체하지 않고 별도 capability/evidence gate를 통과한 뒤 확장한다.


> **시뮬레이션 NVIDIA GPU (D52)**: `make sim-gpu-up`은 앱 자체 k3s 노드에 `nvidia.com/gpu`를 *스케줄링 전용*으로
> 광고해 NVIDIA 타깃 매니페스트(`resources.limits`)를 로컬에서 스케줄 테스트하게 해줄 뿐이다. GPU 패스스루가
> 아니며 파드는 어떤 디바이스도 받지 않는다 — K8s는 연산을 실행하지 않고 실제 GPU 작업은 호스트 CuMetal/MLX가 맡는다.

### 1.1 ComputeBackend 확장 모델

KubeMetal의 장기 방향은 특정 런타임 하나를 모든 AI workload의 실행 환경으로 고정하지 않고
명시적인 `ComputeBackend` 계약으로 분리하는 것이다.

```mermaid
flowchart TB
    REQ["User / Agent Request"] --> POLICY["Policy + Routing<br/>#24"]
    POLICY --> BACKEND{"ComputeBackend"}
    BACKEND --> MLX["host-mlx<br/>Default / Verified"]
    BACKEND --> CUMETAL["host-cumetal<br/>Experimental / #84"]
    BACKEND --> KRUNKIT["krunkit-container<br/>Experimental / #94"]
    BACKEND --> REMOTE["remote-kubernetes<br/>Extension"]
    MLX --> EVIDENCE["Execution Evidence / Observability"]
    CUMETAL --> EVIDENCE
    KRUNKIT --> EVIDENCE
    REMOTE --> EVIDENCE
```

| Backend | 실행 경계 | 현재 상태 | 핵심 검증 |
|---|---|---|---|
| `host-mlx` | macOS host native | **기본 / 실측 검증** | MLX 학습·서빙, 메모리/thermal guardrail |
| `host-cumetal` | macOS host compatibility layer | **Experimental** — 탐지 슬라이스 구현됨(D50: `get_cumetal_status`, 읽기 전용); D51: `run_cumetal_smoke` clang 등록/JIT smoke 구현됨(AOT `cumetalc`는 Metal 툴체인 필요로 보류) | CUDA API/workload correctness·성능 (#84) |
| `krunkit-container` | Colima guest/container | **Experimental** | GPU container → K3s Pod → scheduling boundary (#94) |
| `remote-kubernetes` | 외부 K8s accelerator cluster | **확장 경로** | capability/data-boundary/routing (#24) |

#### krunkit 검증 경계

Colima 0.10+의 `krunkit` GPU-accelerated container 경로는 새로운 가능성이지만 다음을
서로 다른 capability로 취급한다.

```text
GPU container 실행 가능
        !=
K3s Pod에서 GPU 사용 가능
        !=
Kubernetes가 GPU를 allocatable resource로 관리 가능
        !=
DRA / Kueue 적용 가능
```

따라서 #94에서 아래를 실측하기 전에는 README/UI/API에서 "Kubernetes GPU 지원"으로
표현하지 않는다.

1. K3s Pod 내부 실제 GPU 실행 provenance
2. accelerator discovery / allocation semantics
3. multi-Pod isolation / accounting
4. restart/recreate 안정성
5. host MLX 대비 성능·메모리·thermal 특성

DRA/Kueue는 위 조건을 만족하는 **실제 Kubernetes 관리 가능 resource**가 존재함이 확인된
이후 별도 integration issue로 분리한다. 단순히 VM 또는 container에서 GPU API 호출이
성공했다는 이유로 가상의 `gpu=1` resource abstraction을 만들지 않는다.

## 2. IPC 커맨드 흐름

전체 24개 커맨드의 시그니처/설명은 [docs/02-requirements.md §4.1](02-requirements.md#41-tauri-rust-commands-frontend--backend-ipc)이 canonical 출처다. 아래는 기능군별 요약.

| 기능군 | 커맨드 | 설명 |
|--------|--------|------|
| 클러스터 제어 | `get_system_metrics` | sysinfo 기반 RAM/CPU 사용률 조회 (D2) |
| | `get_cluster_status` | `colima --profile <profile> status --json` 파싱 + MLflow/SeaweedFS 파드 준비 상태 조회 |
| | `start_cluster` | 감지된 호스트 RAM 기준 clamp된 cpu/memory로 `colima --profile <profile> start` 실행 |
| | `stop_cluster` | `colima --profile <profile> stop` 실행 (포트포워딩 정리 선행) |
| | `provision_mlops_stack` | `scripts/k8s/`의 Secret + MLflow + SeaweedFS + mac-gpu-bridge 매니페스트 4종을 `kubectl apply` (D18: `resolve_bundled_resource`로 번들 `_up_/` 평탄화 경로 해석) |
| | `start_port_forward` / `stop_port_forward` | MLflow/SeaweedFS S3/Filer 3개 포트포워딩 자식 프로세스 spawn/종료 |
| 모델 허브 (Phase 2b) | `search_hf_models`, `download_hf_model`, `get_model_downloads`, `list_local_models` | Hugging Face 검색·다운로드(`~/.kubemetal/models/`)·진행 상태·로컬 목록 조회 |
| 모델 다운로드 staging (D53) | `list_model_download_staging` → `useModelHub` → `ModelDownloadStagingCard` | 별도 읽기 전용 snapshot. 서버의 공유 root 계산 → blocking worker → canonical parent/no-follow directory handles → bounded metadata scan. ID·nullable 크기·관측 시각·상태 근거·부분 오류/상한 표시. snapshot은 observed/not_created/unknown을 구별하며, 열린 부모 아래 루트 미생성만 not_created이다. 부모 누락·관측 후 루트 소멸·열기 실패는 unknown+오류, 열거 조기 중단은 truncated이다. 현재 프로세스 등록과 device/inode가 일치하는 시도만 active, 재시작·PID 재사용·나이는 unknown. catalog/`Vec<DownloadStatus>` 계약 유지, 삭제 없음(#221 cleanup AC UNMET). IPC owner 승인 후 merge. |
| | `upload_model_to_storage`, `register_model_mlflow` | SeaweedFS S3(`models` 버킷) 업로드 → MLflow Model Registry 등록 |
| MLX 스튜디오 (Phase 2c) | `check_mlx_env`, `setup_mlx_env` | `~/.kubemetal/venv`의 python3/mlx-lm 상태 조회·설치 |
| | `run_mlx_finetune` | venv python으로 `scripts/mlx/finetune_wrapper.py`(D18 경로 해석) 실행, `process_group(0)`으로 기동(D17). D47: 유일한 파인튜닝 경로(Prefect 플로우 제거). D49: mlx-lm save-completed 로그에서 immutable 검증 snapshot을 기록하고 checkpoint 이벤트를 emit; Failed/Killed source만 새 attempt로 weights warm-start한다. mlx-vlm resume은 지원하지 않는다. D45: staging `out/`에만 출력; exit 0·기대 done 경로·manifest 검증 후 삭제와 같은 admission mutex 아래 최종 이름으로 배타적 승격해야 done |
| | `get_mlx_status`, `kill_mlx_process` | 환경/학습/서빙 통합 상태 조회, SIGTERM→SIGKILL 종료(학습은 그룹 전체 대상; 리더 선종료 시 TERM 전 자식의 시작 시각·PGID·native argv의 MLX 신원을 재검증하며 증거가 없으면 거부, D17) |
| | `start_model_serving`, `stop_model_serving` | `mlx_lm server` 기동/정지. D48: 어댑터를 서빙하기 전 `adapter_serving_gate`(`services/mlx_artifacts.rs`, `verify_manifest` 재사용)로 manifest를 검증한다. 변조/누락/추가 파일이 있으면 spawn 없이 거부, manifest 없는 레거시 어댑터는 검증 없이 서빙(UI 미표시). 해싱은 `spawn_blocking`. `revert_to_last_serving`은 실행 중 서버를 멈추기 전에 같은 게이트를 먼저 건다 |
| | `list_registered_models` | MLflow Model Registry 조회 → 파이프라인 뷰 "등록" 단계 노출 (FR-08) |
| | `get_cumetal_status` | D50: CuMetal(Lulzx/cuda-metal) **읽기 전용 탐지** — `xcrun --find metal/metallib`, `cumetal`/`cumetalc` 경로, `cumetal version`/`doctor`(모두 `external_command` + 타임아웃). 모두 충족해야 `available`, 아니면 첫 실패 사유와 함께 `unavailable`. 설치·컴파일·실행·NVIDIA 텔레메트리 없음 |
| | `run_cumetal_smoke` | D51: vectorAdd/SAXPY/reduction `.cu`(임베드)를 clang CUDA 등록/JIT 경로로 compile -> link -> run, 단계별 별도 상태 + parity + `CUMETAL_PROVENANCE` 파싱 + 버전 manifest. 학습/서빙/벤치마크 점유 중 거부. `device=apple_gpu`·`launch_success=true` provenance가 있어야 `passed`(자기 보고 기반). NVIDIA 텔레메트리 없음 |
| 접근 콘솔 (Phase 2d) | `get_service_access` | MLflow/SeaweedFS S3·Filer/Model Serving 4종 헬스 + S3 크리덴셜 Secret 조회 (FR-09) |
| 하드웨어 가드레일 (Phase 3) | `get_guardrail_status`, `set_guardrail_config` | memory pressure(D16)·배터리 상태 조회, 배터리 시 자동 일시정지 설정 |
| | `pause_mlx_training`, `resume_mlx_training` | 학습 프로세스 그룹에 SIGSTOP/SIGCONT (D17) |

### 프론트 훅 매핑

| 훅 | 갱신 주기 | 호출 커맨드 |
|----|-----------|-------------|
| `useMetrics` | 1000ms | `get_system_metrics` |
| `useColima` | 5000ms | `get_cluster_status` (+ `start_cluster` 트리거) |

발열 가드레일과 Metal GPU 모니터링은 계획과 다른 방식으로 구현됐다(docs/01-proposal.md §7
Phase 3): 발열은 배치 축소가 아니라 `thermalState` `serious` 이상에서의 옵트인 학습 일시정지(D28,
`set_guardrail_config`의 `thermal_pause`), GPU는 `powermetrics`가 아니라 sudo-free `ioreg`
파싱으로 `get_system_metrics`에 실린다(D2 개정, FR-05.4, §7).

## 3. 포트 맵 (D1)

| 서비스 | 호스트 포트 | 대상 |
|--------|-------------|------|
| MLflow | 5001 | `svc/mlflow` 5000 (5000은 macOS AirPlay Receiver 점유) |
| SeaweedFS S3 API | 8333 | `svc/seaweedfs` 8333 |
| SeaweedFS Filer UI | 8888 | `svc/seaweedfs` 8888 |
| 모델 서빙 (Phase 2) | 8080 | 호스트 `mlx_lm.server` / `llama-server` (설정 가능) |

## 4. K8s ↔ 호스트 브릿지

```mermaid
sequenceDiagram
    participant Pod as K8s Pod
    participant DNS as CoreDNS (10.43.0.10)
    participant Svc as mac-gpu-service (ExternalName)
    participant Host as macOS Host (192.168.5.2)

    Pod->>DNS: resolve mac-gpu-service.default.svc.cluster.local
    DNS->>Svc: CNAME → host.lima.internal
    Svc->>Host: host.lima.internal → 192.168.5.2 (Lima 게이트웨이)
    Pod->>Host: 직접 연결 (예: :8080) — ExternalName은 포트 프록시 없음
```

`mac-gpu-service`는 `default` 네임스페이스의 `ExternalName` Service로, `host.lima.internal`을
가리키는 DNS CNAME 별칭일 뿐 포트 프록시를 수행하지 않는다 — 클라이언트가 대상 포트를
직접 지정해 연결해야 한다.

## 5. 실측 검증된 사실 (colima 0.10.3, tauri 2.11.5)

- (2026-07-20) `colima --profile <profile> status --json`은 기동 중일 때만 exit 0 + 평면 JSON
  (`{"kubernetes": bool, ...}`)을 출력한다. `status` 필드는 존재하지 않으며, 미기동 시
  exit 1 + stdout 없음.
- (2026-07-20) `mac-gpu-service` → `host.lima.internal` → `192.168.5.2` CNAME 체인이
  파드 내부 busybox nslookup으로 정상 해석됨을 확인했다.
- (2026-07-20) MLflow(5001), SeaweedFS S3(8333)·Filer UI(8888) 포트포워딩 후 각각 HTTP
  200 / `ListAllMyBucketsResult` 응답을 curl로 확인했다.
- (2026-07-21) `pnpm tauri build`로 생성한 `.app` 번들 실측: `resource_dir()`은 항상
  `Contents/Resources`를 가리키지만, `tauri.conf.json`의 `../scripts/...` 리소스는
  `Contents/Resources/_up_/scripts/...`로 평탄화되어 담긴다(D18). 번들 앱 기동 후 5초간
  프로세스 생존을 확인했다(패닉 없음).
- (2026-09-19) Colima `krunkit` 경로는 **연구/검증 대상으로만 등록**했다(#94).
  현재 KubeMetal repository에는 krunkit 기반 K3s Pod GPU 사용, Kubernetes allocatable
  accelerator, DRA/Kueue 동작을 실측한 evidence가 없으므로 지원 기능으로 표기하지 않는다.

세부 내용은 [docs/03-mvp-design.md §5](03-mvp-design.md#5-미검증-전제-실기기-검증-필요) 참고.

## 6. Phase 3 잔여 범위

- 발열 가드레일 — 구현됨. 계획했던 "MLX 배치 크기 자동 축소"가 아니라 `thermalState`가
  `serious` 이상일 때의 옵트인 학습 일시정지(D28)와 스폰 전 admission 거부(D40)다. 배치 크기
  자동 축소는 구현하지 않았다.
- Metal GPU 사용률 모니터링 — 구현됨. `powermetrics`/privileged helper가 아니라 sudo-free
  `ioreg -c IOAccelerator` 파싱(D2 개정, FR-05.4). `powermetrics`는 채택하지 않는다.

`run_mlx_finetune`/`kill_mlx_process`(파인튜닝 실행·프로세스 그룹 종료, D17),
`pause_mlx_training`/`resume_mlx_training`(메모리 압박·배터리 가드레일 연동, D16)은
모두 구현 완료 상태이며 §2 표 참고.

## 7. GPU 메트릭 가용성: Apple Silicon vs NVIDIA

이슈 #11의 "macOS에서 측정 불가한 지표를 NVIDIA와 비교해 명시" 항목에 대한 문서다. 코드에서 확인한
것만 적고, 근거가 없으면 `미검증`(macOS)/`미확인`(NVIDIA)으로 둔다(D22). 이 위치는 D41
텔레메트리(`GpuTelemetryBackend`)와 IPC 요약(§2)이 이미 있는 아키텍처 문서라서 골랐다.

**NVIDIA 코드 경로는 없다.** `GpuTelemetryBackend` 구현체는 `AppleMetalBackend` 하나이고
`gpu_backend`는 항상 `"apple_metal"`이다(`src-tauri/src/commands/metrics.rs:113-116`, `:249-266`, `:280`).
NVIDIA 백엔드는 실기기가 없어 보류했다(D41, `docs/03-mvp-design.md` D41 행). 아래 NVIDIA 열은 앱 지원 여부가
아니라 `nvidia-smi`/NVML이 일반적으로 노출하는 지표이며, 이 저장소에는 그 근거(문서·코드·측정)가 없다.
필드명은 일반 지식 기준이므로 NVIDIA 열의 "제공"은 저장소로 검증되지 않은 값이다(`미검증`).

| 지표 | macOS (이 앱, sudo-free) | NVIDIA (`nvidia-smi`/NVML, 저장소 근거 없음) | KubeMetal 현재 보고 |
|------|--------------------------|----------------------------|---------------------|
| GPU 사용률 | `ioreg -l -d 1 -r -c IOAccelerator`의 `Device Utilization %` 파싱 (`metrics.rs:172-173`, `:238`) | 제공 (`utilization.gpu`) | `gpu_usage_percentage` (`Option<f32>`). 읽기 실패 시 응답에서 생략되고(`None`) 프로세스당 1회 stderr 경고만 남는다. 측정된 0은 `0`으로 남는다 (`:104-106`, `:163-210`) |
| GPU 메모리 사용량 | 같은 출력의 `In use system memory` (통합 메모리 중 GPU 사용분) (`:239`) | 제공 (`memory.used`) | `gpu_memory_used_gb` (`Option<f64>`, GB, 소수 2자리). 실패 시 응답에서 생략 (`:107-109`, `:211-217`) |
| GPU 메모리 총량 | 해당 필드 없음. 통합 메모리라 별도 VRAM 개념이 아니다 | 제공 (`memory.total`) | 보고 안 함. `total_memory_gb`는 시스템 RAM이다 (`:100`, `:303`) |
| 온도(수치 °C) | 이 기기(M4 Pro, macOS 26, 2026-07-27)에서 `pmset -g therm`, `sysctl`, `ioreg AppleSMC` 모두 비어 있음을 실측 (`:121-124`) | 제공 (`temperature.gpu`) | 수치 없음 |
| 발열 압력 | `NSProcessInfo.thermalState` (`:129-133`) | NVIDIA 쪽 동등 개념(슬로다운 사유 등)의 이름/의미는 미확인 | `thermal_state`: `nominal`/`fair`/`serious`/`critical`, 읽기 실패·미정의 값은 `None` (`:112`, `:139-153`). UI는 "조회 불가"로 표시 (`src/components/dashboard/MetricsPanel.tsx:185-186`) |
| 전력(W) | `powermetrics`는 root 필요해 채택 안 함 (D2, `docs/02-requirements.md` FR-05.4). sudo-free 대체 경로는 미검증 | 제공 (`power.draw`) | 보고 안 함 |
| 클럭 | 미검증 | 제공 (`clocks.sm` 등) | 보고 안 함 |
| 프로세스별 GPU 사용 | 미검증 | 제공 (`--query-compute-apps`) | 보고 안 함 |
| ECC/오류 카운터 | 미검증 | 지원 GPU에서 제공. 모델별 지원 범위는 미확인 | 보고 안 함 |
| MIG | 미검증 (해당 개념이 macOS에 있는지 근거 없음) | MIG 지원 GPU에서 제공. 모델별 범위는 미확인 | 보고 안 함 |
| GPU 코어 수(인벤토리) | `system_profiler SPDisplaysDataType`의 `Total Number of Cores` (`:70-83`) | 해당 지표 이름 미확인 | `HardwareSpec.gpu_cores` (`Option<u32>`, 실패 시 `None`) (`:91`) |
| 처리량 벤치마크 | MLX matmul GFLOPS 실측 `run_gpu_benchmark` (`:462`, 결과 구조체 `GpuBenchmarkResult` `:318-328`, `docs/02-requirements.md` §4.1) | 미확인 | `gflops` 등. 실패는 전부 `Err` |

### 읽는 법과 주의

- GPU 사용률/메모리는 읽기 실패를 **0이 아니라 생략(`None`)** 으로 낸다. `ioreg` 미발견·실행 실패·
  비정상 종료·파싱 실패 경로는 모두 `(None, None)`을 돌려주고(`:163-210`), 한쪽 필드만 읽히면 그
  필드만 싣는다(`gpu_readings`, `:211-217`). 프론트는 값이 없으면 발열과 같은 "조회 불가"로
  표시하며 `?? 0`으로 채우지 않는다(`MetricsPanel.tsx`, `Header.tsx`).
- 사용률·메모리 수치는 한 번 파싱 불완전 경고가 나간 뒤에는 로그에 더 남지 않는다(`:155-157`).
- `ioreg` 출력 형식 의존이다. 형식이 바뀌면 파싱이 조용히 비게 된다(`:201-203`의 경고 문구가 이를 인정한다).

### 이 절이 다루지 않는 이슈 #11 잔여 항목

storage/cache I/O 벤치마크, generic accelerator evidence schema(재현 가능 export), Narwhal 연동 샘플,
벤치마크 프런트 UI. 모두 별도 설계 결정이 필요한 미구현 항목이다(이슈 #11 오너 코멘트).

## 설계 결정 레지스트리

D1~D18 전체 목록과 근거는 [docs/03-mvp-design.md §4](03-mvp-design.md#4-설계-결정-및-주의사항)를
canonical 출처로 참고할 것 — 본 문서에서 중복 서술하지 않는다.

현재 구현 이후의 compute/runtime 확장 검토는 기존 D1~D18을 소급 변경하지 않고 GitHub
issue/evidence gate로 관리한다. 주요 추적 이슈는 #24(Model/Compute Routing),
#84(CuMetal), #94(krunkit/K3s GPU feasibility)다.

D43: 관리 프로필은 `scripts/colima-profile.txt`에서 읽고 context는 `colima-<profile>`로 파생한다. 기본 프로필의 이전 측정은 전용 프로필 D10 브리지 검증을 대신하지 않는다.

## #38 retrieval comparison

`scripts/rag/eval_compare.py`는 opt-in CPU 비교 도구다. 커밋된
`scripts/rag/eval/fixtures/`의 작은 한/영/code/config 문서와 수동 지정한 binary
문서 qrels를 사용한다. qrels는 구현 단계의 임시 기대값이며 사람의 검토는 대기 중이다. 제품 모델 선택·routing·RRF 정책을 변경하지 않는다.
기존 stdlib `eval_retrieval.py`는 그대로 유지한다.

```sh

**실측 (measured-local, 2026-11-14, M4 Pro, 오케스트레이터 실행):** 로컬에 이미 있던
`sentence-transformers/all-MiniLM-L6-v2` 스냅샷(1110a243)으로 opt-in 비교를 오프라인(`HF_HUB_OFFLINE=1`) 실행했다.
status `ok`, lancedb 0.34.0, sentence-transformers 5.6.0, 모델 로드 4.4 s, fixture 4문서·5질의 × 5회 반복.

| 모드 | recall@1 | recall@3 | MRR | nDCG@3 | p50 (ms) |
|---|---|---|---|---|---|
| exact | 0.20 | 0.20 | 0.20 | 0.20 | 3.9 |
| lexical | 0.80 | 0.90 | 1.00 | 0.92 | 6.2 |
| dense | 0.80 | 1.00 | 1.00 | 0.98 | 11.0 |
| hybrid | 0.80 | 1.00 | 1.00 | 0.98 | 14.3 |
| auto | 0.80 | 1.00 | 1.00 | 0.98 | 7.4 |

읽는 법과 한계: exact는 파일명 질의 1건만 맞히도록 설계된 모드라 0.20이 정상이다. lexical은 한국어 질의에서
정답 2개 중 1개만 찾았다(recall@3 0.5). **이 fixture는 4문서·5질의짜리라 "실제 경로로 도구가 돈다"는 증거일 뿐
검색 품질 평가가 아니다** — 반복 5회는 정확도 표본을 늘리지 않고(결정적) 지연 시간만 다시 잰다. 실제 코퍼스·사람이
검토한 qrels·rerank 비교는 #38에 남아 있다.

# 이미 보유한 로컬 sentence-transformers snapshot만 사용. 설치/다운로드하지 않는다.
python3 -B scripts/rag/eval_compare.py \
  --fixture scripts/rag/eval/fixtures --model-path /absolute/path/to/local/snapshot \
  --repeats 5 --output /tmp/retrieval-comparison.json
```

동일 production fixed chunker로 fixture를 한 번 나누고, CPU 모델을 한 번 적재한 뒤
embedding·임시 LanceDB table을 한 번 준비한다. fixture-relative source로 tie 순서를
고정하며 실제 table의 전체 records를 입력과 대조한다. 모든 모드는 production
`rag_host.cmd_query`를 호출하며 같은 table의 corpus/chunk metadata를 사용한다.
exact/lexical은 모델이나 vector 검색 없이 table의 원문 records를 검색한다.
dense/hybrid는 동일 vector records와 로컬 model snapshot을 사용하고, auto는
production 분류·검색 경로에 따라 해당 조건을 따른다. exact/lexical(BM25)/
dense/hybrid/auto의 순서를 query·반복마다 회전시킨다. hybrid는 production의 전역
lexical+dense RRF이며 별도 lexical 구현이나 candidate rerank가 아니다.

JSON은 corpus/query/chunk/model/config/production/harness/index-record 해시와 index
버전, dependency 버전, 설정, 원문 chunks/qrels, 요청별 원시 chunk ranks·점수·routing,
문서 collapse 후 recall@1/3/5·binary nDCG@1/3/5·MRR 및 표본 수 n을 보존한다.
`fixture_prepare_ms`, `model_load_ms`, `index_prepare_ms`는 측정 query 구간에서 분리된다. `query_ms`는
상주 모델을 사용한 cmd_query 전체(DB open/FTS 생성/encode/search/JSON 포함) 시간이다.
별도 subprocess 요청도 같은 index에서 production cmd_query를 실행하고 원시 응답,
내부 query/model-load 시간, 부모가 잰 전체 `wall_ms`를 남긴다. 이 worker는 CPU·로컬
전용 로더를 주입하므로 제품 CLI와 로더 설정은 다르지만, Python 기동·import·모델
재적재를 포함하는 단발 CLI 비용을 드러낸다. subprocess와 상주 query는 각각 실행된다.
전체 5회 결과의 n은 모드별 `질의 수 × 5`이며 독립 질의 수와 다르다.

빈 검색 결과는 유효한 0점이다. 누락/변조 metadata, production 오류, subprocess 실패는
요청 오류로 남고 전체 exit는 nonzero다. auto의 상주·subprocess route/rule과
양쪽 응답 또는 dense 실패 원인을 보존하며, 응답 차이도 비교 오류로 기록한다.
출력 디렉터리는 준비 단계 전에 생성·검사하고 완성된 JSON만 원자적으로 교체한다. 실패 요청은 평균에 포함하지 않으므로 부분
report는 성공 비교로 해석하면 안 된다. dependency/model 부재는 `unavailable`·nonzero다.
온라인 접근과 remote model code는 비활성화하며 snapshot/fixture 변경도 거부한다.

원시 JSON의 `raw`, `queries`, `chunks`를 `eval_compare.summarize`에 넘기면 기존
평가기 metric 함수로 report를 재계산한다. `tests/rag/test_eval_compare.py`는 결정적
stub embedding/SDK로 실제 cmd_query 분기를 검사하며 기존 `make test`/`make verify`의
`unittest discover -s tests/rag`에 포함된다. **이 테스트는 실제 LanceDB/embedding
모델 성능 측정이 아니다.** 실제 모델 비교와 raw→report 일치는 오케스트레이터가 위
opt-in 명령 후 확인해야 한다. 작은 fixture 점수는 제품 전체 품질로 일반화하지 않는다.

```sh
# 위 opt-in 실행 후 저장된 raw만으로 report 재계산 대조 (모델 재실행 없음)
python3 -B -c 'import json,sys; sys.path.insert(0,"scripts/rag"); from eval_compare import summarize; e=json.load(open(sys.argv[1])); assert e["status"] == "ok"; assert summarize(e["raw"],e["queries"],e["chunks"]) == e["report"]; print("raw/report match")' /tmp/retrieval-comparison.json
```
