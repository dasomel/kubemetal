# KubeMetal

Apple Silicon 전용 하이브리드 MLOps 데스크톱 앱 — Kubernetes 표준 제어면과 macOS 호스트
네이티브 MLX 연산을 하나의 Tauri v2(Rust) + React/TypeScript 앱으로 통합합니다.

## 핵심 컨셉: Control/Compute 분리

KubeMetal은 **제어(Control)와 연산(Compute)을 물리적으로 분리**합니다. MLflow, SeaweedFS
같은 MLOps 스택은 Colima(`vz` + `virtiofs`) 위에서 구동되는 경량 K3s 클러스터 안에
파드로 배포되어 표준 K8s 매니페스트로 관리됩니다. 반면 MLX 기반 파인튜닝·서빙 같은 실제
연산은 K8s 파드 내부가 아니라 **macOS 호스트 프로세스**로 직접 실행됩니다.

이렇게 나누는 이유는 선호가 아니라 하드웨어 제약입니다. Apple Silicon의 Metal GPU는 리눅스
VM으로 패스스루할 수 없기 때문에, K8s Pod 안에서는 MLX 연산을 수행할 방법이 없습니다.
따라서 K8s는 실험 추적(MLflow)·아티팩트 저장(SeaweedFS) 같은 표준 MLOps 제어면 역할만
맡고, GPU를 쓰는 모든 작업은 Rust 백엔드가 스폰하는 호스트 프로세스로 위임됩니다.

이 하이브리드 구조 덕분에 클라우드 GPU 비용 없이 로컬 데스크톱에서 시작해, 향후 원격
GPU 서버나 멀티노드 K3s 클러스터로 자연스럽게 확장할 수 있는 경로를 열어둡니다.

## 요구 사항

- macOS 14+ (Apple Silicon)
- Homebrew
- colima, kubectl — `brew install colima kubectl`
- Node 22+ / pnpm
- Rust (rustup)

## 구동 방법

1. 의존성 설치
   ```bash
   pnpm install
   ```
2. 개발 모드 실행 — `beforeDevCommand`로 vite 개발 서버가 자동 기동됩니다.
   ```bash
   pnpm tauri dev
   ```
3. 앱 대시보드에서 **클러스터 시작** 버튼을 누르면 감지된 호스트 RAM 기반으로 자동
   산정된 CPU/메모리 값으로 다음 명령이 내부적으로 실행됩니다.
   ```bash
   colima start --cpu <N> --memory <M> --vm-type=vz --mount-type=virtiofs --kubernetes
   ```
4. **MLOps 스택 프로비저닝** 버튼을 눌러 MLflow / SeaweedFS / mac-gpu-bridge 매니페스트를
   클러스터에 적용합니다.
5. **포트포워딩 시작** 버튼을 눌러 아래 주소로 접속합니다.
   - MLflow: http://localhost:5001
   - SeaweedFS S3 API: http://localhost:8333
   - SeaweedFS Filer UI: http://localhost:8888

### 트러블슈팅 (CLI로 직접 확인)

```bash
colima status --json
kubectl --context colima get pods -n default
```

## 빌드 / 패키징

```bash
pnpm tauri build   # .app / .dmg 번들 생성
```

## 프로젝트 구조

```text
kubemetal/
├── src/               # Frontend (React + TypeScript + Tailwind)
├── src-tauri/         # Backend (Rust Native Control Agent)
├── scripts/k8s/       # MLflow / SeaweedFS / mac-gpu-bridge 매니페스트
└── docs/              # 기획서 · 요구사항 · MVP 설계 · 아키텍처 문서
```

## 문서 안내

| 문서 | 내용 |
|------|------|
| [docs/01-proposal.md](docs/01-proposal.md) | 프로젝트 기획서 — 문제 정의, 아키텍처, 기술 스택, 로드맵 |
| [docs/02-requirements.md](docs/02-requirements.md) | OSS 리서치 + FR/NFR 명세, IPC 커맨드 표, 매니페스트 스펙 |
| [docs/03-mvp-design.md](docs/03-mvp-design.md) | Phase 1 MVP 설계 — 디렉터리 구조, Rust/TS 참조 코드, 설계 결정 레지스트리(D1~D12) |
| [docs/04-architecture.md](docs/04-architecture.md) | 전체 아키텍처 — 계층 다이어그램, IPC 흐름, 포트 맵, K8s↔호스트 브릿지 |

## 개발 로드맵

- **Phase 1 (완료)**: Tauri v2 백엔드 + Colima(vz) 원클릭 라이프사이클 제어, sysinfo 기반
  RAM/CPU 모니터링, K8s 내 MLflow/SeaweedFS 1클릭 셋업
- **Phase 2**: 호스트 MLX LoRA 파인튜닝 엔진 연동, Prefect Host Worker 기반 하이브리드
  파이프라인, MLflow Model Registry 자동 등록
- **Phase 3**: 통합 대시보드 UI 완성, .dmg 패키징, 배터리/발열/슬립 방지 가드레일,
  (선택) powermetrics 기반 Metal GPU 모니터링

자세한 로드맵은 [docs/01-proposal.md §7](docs/01-proposal.md#7-단계별-개발-로드맵-roadmap) 참고.
