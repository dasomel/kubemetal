# 05. MLOps 확장 리서치 (Phase 4 제안)

> 2026-07-23 기준 웹 리서치. 오케스트레이터가 핵심 주장을 1차 출처로 직접 교차 검증했고,
> agy(Gemini 검색 그라운딩) 병렬 리서치로 보강했다. 아키텍처 불변식(제어면=K8s,
> 연산=macOS 호스트) 유지를 전제로 한다.

## Q1. 파이프라인 오케스트레이션 → **Prefect 3 채택 권장**

- Prefect 3 서버는 K8s Helm 배포 시 **512Mi~1Gi RAM 베이스라인**으로 구동 가능
  ([Prefect Helm 가이드](https://docs-3.prefect.io/v3/manage/server/examples/helm)) —
  D4 VM 예산(4~12GB) 안에 들어가는 유일한 풀 오케스트레이터급.
- 워커는 **아웃바운드 폴링** 방식의 경량 프로세스라 "서버=K3s 파드, GPU 워커=macOS
  호스트 Process Worker" 하이브리드가 공식 패턴으로 성립 — FR-04.2(Prefect Host
  Worker) 계획이 그대로 유효.
- 비교: Dagster는 webserver+daemon 각 ~1GB(이중 구조), Flyte 제어면은 3~4GB+4CPU로
  과중(agy 그라운딩). ZenML은 서버 경량이나 K8s 오케스트레이션 시 자체 러너 필요.

## Q2. 평가(Evaluation) → **lm-eval-harness + MLflow 3.x GenAI**

- lm-evaluation-harness는 `local-completions`/`local-chat-completions` 모델 타입으로
  **OpenAI 호환 base_url 평가를 공식 지원** — 우리 mlx_lm.server(:8081/v1)에 무수정
  연결 가능 ([API 가이드](https://github.com/EleutherAI/lm-evaluation-harness/blob/main/docs/API_guide.md)).
  단 chat 인터페이스는 loglikelihood 미지원 → MMLU류 일부 과제는 completions 필요.
- MLflow 3.x는 GenAI **evaluate(LLM-as-a-judge 스코어러)와 OpenTelemetry 기반
  tracing UI**를 내장 — 이미 떠 있는 파드에서 추가 인프라 없이 평가·트레이스 제공.

## Q3. 데이터셋 버저닝 → **DVC + SeaweedFS S3 remote**

- DVC는 클라이언트-온리(서버 무설치)로 Git 커밋과 데이터 버전을 함께 관리, S3 호환
  remote 지원 → 기존 SeaweedFS(8333)를 remote로 그대로 사용(agy 그라운딩).
- lakeFS는 별도 서버·다중 사용자 거버넌스 지향이라 단일 사용자에 과중 — 배제.

## Q4. 모니터링/관측 → **MLflow tracing 우선, Langfuse 배제**

- Langfuse v3 셀프호스트는 web 4Gi + worker 4Gi + ClickHouse 8Gi + Postgres 4Gi +
  Redis ≈ **총 20Gi+** 요구, 경량 대안 공식 부재
  ([셀프호스팅 문서](https://langfuse.com/self-hosting)) — VM 예산 초과로 배제.
- 1차: **MLflow 3.x tracing**(파드 0개 추가). 필요 시 Evidently UI(<500Mi 파드,
  스냅샷 렌더 전용) + whylogs 프로파일, OTel Collector(100~200MB)로 확장(agy 그라운딩).

## Q5. 로컬 RAG → **LanceDB 임베디드(호스트) + mlx-embeddings**

- 단일 사용자에겐 서버형보다 **임베디드 DB가 적정**: LanceDB는 Rust 디스크 중심
  설계로 RAM 요구가 낮고 메모리 초과 데이터셋 처리 가능
  ([2026 비교](https://encore.dev/articles/best-vector-databases)). Chroma도 대안.
- 멀티 클라이언트/메타데이터 필터가 필요해지면 Qdrant 파드(K3s)로 승격 — 제어면
  불변식과 정합(벡터DB는 연산이 아니라 데이터 서비스).
- 임베딩 생성은 mlx-embeddings + mlx-community 변환본(호스트, Metal 가속).

## Q6. MLX 서빙 고도화 → **vllm-metal이 승격 경로, Ollama는 32GB+ 제약**

- **vllm-metal**: vLLM 프로젝트 커뮤니티 플러그인, MLX 백엔드 + paged attention +
  continuous batching + OpenAI 호환 서버. 2026-04 v0.2.0에서 TTFT 83×/처리량 3.6×
  개선 ([GitHub](https://github.com/vllm-project/vllm-metal),
  [Docker 발표](https://www.docker.com/blog/docker-model-runner-vllm-metal-macos/)).
  동시요청·멀티모델이 필요해지는 시점의 승격 경로.
- **Ollama**: 2026-03-30 Apple Silicon 엔진을 MLX로 전환(프리뷰, decode 58→112 tok/s).
  단 **32GB+ 통합 메모리 전용** — 8/16GB는 구 Metal 경로
  ([Ollama 블로그](https://ollama.com/blog) 및 벤치 기사) → 최소사양 16GB인 본 제품의
  기본 엔진으로는 부적합. 어댑터 서빙 유연성도 mlx-lm 대비 제한적.
- llama.cpp server는 `--parallel`+`--cont-batching`으로 동시처리 가능하나 MLX 대비
  Apple Silicon 이점 축소 추세(agy 그라운딩).

## 권장 로드맵 (Phase 4)

| 순위 | 항목 | 내용 | 비용/가치 |
|------|------|------|-----------|
| 1 | **4a 오케스트레이션** | Prefect 3 서버 파드(~512Mi) + macOS Process Worker. 파인튜닝·평가·업로드를 플로우로 자동화(FR-04.2 실현) | 중 / 높음 — 이후 모든 자동화의 기반 |
| 2 | **4b 평가 자동화** | lm-eval(local-completions→서빙 엔드포인트) + MLflow GenAI judge·tracing을 Prefect 플로우로 주기 실행, 결과를 MLflow에 기록·UI 노출 | 낮음 / 매우 높음 — "학습→몇 점?"의 격차 해소 |
| 3 | **4c 로컬 RAG** | LanceDB 임베디드 + mlx-embeddings로 문서 인덱싱→서빙 프롬프트 주입, DVC로 데이터셋 버저닝 병행 | 중 / 높음 |
| 병행 | 서빙 승격 옵션 | 동시요청 요구 발생 시 vllm-metal 백엔드 선택지 추가(mlx-lm 기본 유지) | 중 / 상황부 |

**배제 결정**: Langfuse(메모리 20Gi+), lakeFS(단일 사용자 과중), Ollama 기본 엔진
채택(32GB+ 제약·어댑터 유연성), Dagster/Flyte(제어면 RAM 과중).
