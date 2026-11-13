# 08. kagent(K8s 에이전틱 운영) 실험 가능성 검토

> 2026-07-24. 근거: [CNCF 블로그](https://www.cncf.io/blog/2025/04/15/kagent-bringing-agentic-ai-to-cloud-native/),
> [kagent.dev](https://kagent.dev/), Helm/구성 레퍼런스(DeepWiki kagent-dev/kagent).

## 결론: **실험 가능 — 조건부** (우리 스택과의 궁합이 오히려 좋음)

kagent는 K8s 안에서 도구(K8s/Prometheus/Istio/Argo + MCP)·에이전트·UI를 CRD로 선언해
운영 작업을 자동화하는 프레임워크다(AutoGen 기반, Helm 설치: `kagent-crds` + 본 차트,
ghcr OCI). LLM 공급자 8종을 지원하며 **Ollama 공급자가 임의 base URL을 받는다** —
즉 OpenAI 호환 엔드포인트를 가리킬 수 있다.

### 우리 아키텍처와의 핵심 궁합

- **LLM = 우리 로컬 서빙**: kagent 파드(K3s) → **D10 브릿지(mac-gpu-service →
  host.lima.internal)** → 호스트 mlx 서빙(`:8081/v1`). 외부 API 키·비용 없이
  완전 로컬 에이전틱 운영 실험이 성립하며, D10 브릿지의 첫 실전 소비자가 된다.
- **리소스**: kagent 컨트롤러+UI는 수백 MB급 — 현 VM(12GB, 사용 ~2GB)에 수용 가능.
  LLM 연산은 호스트에서 하므로 "제어면=K8s/연산=호스트" 불변식 유지.

### 리스크 (실험 전 검증 순서)

1. **[최우선 검증] 도구 호출(tool calling) 지원**: kagent는 function calling에
   의존한다. `mlx_lm.server`의 `tools` 파라미터 처리 여부를 먼저 실측해야 하며,
   미지원이면 대안 = vllm-metal(OpenAI 호환+tool calling) 또는 호스트 Ollama를
   kagent 전용으로 병행.
2. **모델 품질**: 0.5B는 에이전트 계획/도구 선택에 부족 — 실험은
   `mlx-community/Qwen2.5-7B-Instruct-4bit`(~4.5GB, 64GB 호스트 무난) 권장.
   16GB 프로필 기기에서는 3B급 한계 명시 필요.
3. **도구 커버리지**: 내장 Prometheus/Argo 에이전트는 해당 스택 부재로 미사용 —
   1차 실험 범위는 **K8s 트러블슈팅 에이전트**(현 클러스터 진단)로 한정.
4. kagent 버전 변동(2025-04 발표 이후 활발) — 설치 시점 차트 버전 고정 필요.

## 제안 실험 플랜 (Phase 5c 후보, 반나절 스코프)

1. `mlx_lm.server` tools 파라미터 실측 → 미지원 시 vllm-metal로 서빙 엔진 교체 실험 병행
2. 7B-4bit 모델 다운로드(모델 허브) → 8081 서빙
3. `helm install kagent-crds` + `kagent`(ns kagent, 버전 고정, Ollama 공급자
   base URL = `http://mac-gpu-service.default.svc.cluster.local:8081/v1`)
4. 시나리오: 고의 결함 주입(예: 이미지 오타 Deployment) → kagent K8s 에이전트에게
   "파드가 왜 안 뜨는지 진단·수정 제안" → 결과 평가
5. 성공 시: 접근 콘솔에 kagent UI 링크 추가, 파이프라인 탭 상태 노출(포트포워드 규약 편입)

**배제/보류**: Prometheus·Argo 에이전트(스택 부재), 멀티 에이전트 시나리오(1차 범위 외).

## 실험 결과 (2026-07-24 실기기 수행 — 성공)

| 단계 | 결과 |
|------|------|
| A. 7B 서빙 | `Qwen2.5-7B-Instruct-4bit`(4.0GB) 다운로드, 8081 서빙 교체 |
| B. tools 게이트 | **통과** — 7B에서 `tool_calls`에 name/arguments 구조화 배열 정상 반환. 0.5B의 빈 배열은 서버가 아니라 **모델 역량 문제**였음(도구 호출 토큰 생성 실패) |
| C. 설치 | kagent **0.9.12** 고정(ns kagent), 17개 파드 전부 Ready. LLM 공급자 = `providers.openAI.config.baseUrl` → **`http://mac-gpu-service.default.svc.cluster.local:8081/v1`** — **D10 브릿지의 첫 실전 소비 성립**(파드→호스트 Metal LLM, 외부 API 0). VM 메모리 41%→60%(7.2/12GB) |
| D. 결함 진단 | `broken-nginx`(가짜 이미지 태그) 주입 → k8s-agent가 A2A(JSONRPC)로 도구 6회 호출(`k8s_get_events`×4, `k8s_get_pod_logs`×2), 79초 만에 **ImagePullBackOff 원인 정확 적중** + `kubectl set image` 해결책 제시 |

관찰: 중간 사고 텍스트에 chat-template 토큰 노이즈(품질 무영향), 에이전트 17종 중
우리 스택과 무관한 것 다수(istio/cilium/argo/promql 등) — 정식 편입 시 values로
비활성화해 메모리 절감 여지.

## 후속 옵션 (사용자 결정 대기)

1. **5c 정식 편입**: 불필요 에이전트 비활성화 values 고정 → provision 세트에
   kagent 추가(옵트인 토글), 접근 콘솔에 kagent UI 링크·포워드 규약 편입.
2. **실험 유지만**: 현 설치 상태로 두고 수동 사용(현재 상태).
3. **정리**: `helm uninstall`로 회수(메모리 -5GB).

Sources: [CNCF blog](https://www.cncf.io/blog/2025/04/15/kagent-bringing-agentic-ai-to-cloud-native/) ·
[kagent.dev](https://kagent.dev/) · [Helm install](https://deepwiki.com/kagent-dev/kagent/3.2-helm-installation) ·
[Config reference](https://deepwiki.com/kagent-dev/kagent/10-configuration-reference)

---

## 0.x → 1.x 전환 영향도 맵 및 검증 체크리스트 (#193)

> 현재 리포지토리는 차트 버전 `0.9.12`(`scripts/helm/kagent-version.txt:1`)와 CRD `kagent.dev/v1alpha2` Declarative 스키마(`src-tauri/src/services/kagent_agents.rs:110`)에 고정되어 있다. 외부 조사 덤프에 언급된 `alpha17`(`1.0.0-alpha17`)은 조사 당시 기준의 프리릴리스 관찰값일 뿐이며, 1.x 정식 차트·CRD 스키마·하위 호환성은 본 리포지토리에서 실측되지 않았다(`[unverified]`).

### 1. 현재 리포지토리 결합 지점 (`0.9.12` / `v1alpha2` 기준)

| 영역 | 결합 파일 및 라인 (`file:line`) | 현재 결합 내용 (실측·고정된 규약) |
|------|--------------------------------|-----------------------------------|
| **설치 (Helm·진단·UI 포워드)** | `scripts/helm/kagent-version.txt:1`<br>`src-tauri/src/commands/kagent.rs:21`<br>`src-tauri/src/commands/kagent.rs:30`<br>`src-tauri/src/commands/kagent.rs:338`<br>`src-tauri/src/commands/kagent.rs:413`<br>`src-tauri/src/commands/kagent.rs:425`<br>`src-tauri/src/commands/kagent.rs:427`<br>`src-tauri/src/commands/kagent.rs:168`<br>`Makefile:244`<br>`Makefile:249`<br>`Makefile:251`<br>`Makefile:289`<br>`src-tauri/src/commands/port_forward.rs:37`<br>`scripts/helm/kagent-values.yaml:19` | • 차트 버전 `0.9.12` 단일 출처(`scripts/helm/kagent-version.txt:1`)를 Rust(`include_str!`)와 `Makefile`(`KAGENT_VERSION`)이 공유<br>• `oci://ghcr.io/kagent-dev/kagent/helm` 레지스트리에서 `kagent-crds` 선행 설치 후 `kagent` 본 차트를 `-n kagent`에 설치<br>• `scripts/helm/kagent-values.yaml:19`에서 `providers.openAI.apiKey` 더미 값 주입 및 불필요 기본 에이전트(`scripts/helm/kagent-values.yaml:23`~`42`) 비활성화<br>• `get_kagent_diagnostics`가 파드의 `metadata.labels["app.kubernetes.io/name"]`으로 활성 에이전트 식별(`src-tauri/src/commands/kagent.rs:168`)<br>• UI 포트포워드가 `-n kagent svc/kagent-ui`의 포트 `8080`을 호스트 `8090`으로 연결(`Makefile:289`, `src-tauri/src/commands/port_forward.rs:37`) |
| **ModelConfig (LLM 백엔드)** | `src-tauri/src/commands/kagent.rs:438`<br>`src-tauri/src/commands/kagent.rs:508`<br>`src-tauri/src/commands/kagent.rs:601`<br>`src-tauri/src/commands/kagent.rs:670`<br>`src-tauri/src/commands/kagent.rs:702` | • 고정 리소스명 `default-model-config`(`src-tauri/src/commands/kagent.rs:438`)를 `kubectl -n kagent get modelconfig default-model-config -o json`으로 조회(`src-tauri/src/commands/kagent.rs:508`)<br>• 응답 JSON에서 `spec.openAI.baseUrl` 및 `spec.model` 경로를 직접 파싱해 브리지 URL·포트·모델 ID 일치 판정(`src-tauri/src/commands/kagent.rs:601`)<br>• `configure_kagent_model`이 `providers.default: openAI` 및 `providers.openAI.{apiKey, apiKeySecretKey: OPENAI_API_KEY, apiKeySecretRef: kagent-openai, config.baseUrl, model, provider: OpenAI}` 구조의 임시 values 파일로 `helm upgrade` 실행(`src-tauri/src/commands/kagent.rs:670`)<br>• 단위 테스트가 `scripts/helm/kagent-values.yaml`의 `providers:` 및 `apiKey:` 존재를 강제(`src-tauri/src/commands/kagent.rs:702`) |
| **Agent (CRD 매니페스트·토글)** | `src-tauri/src/services/kagent_agents.rs:9`<br>`src-tauri/src/services/kagent_agents.rs:21`<br>`src-tauri/src/services/kagent_agents.rs:62`<br>`src-tauri/src/services/kagent_agents.rs:83`<br>`src-tauri/src/services/kagent_agents.rs:110`<br>`src-tauri/src/services/kagent_agents.rs:211`<br>`src-tauri/src/commands/kagent.rs:285`<br>`src-tauri/src/commands/kagent.rs:314`<br>`scripts/k8s/security-agent.yaml:1`<br>`scripts/k8s/remote-reader/remote-reader-agent.yaml:23` | • `TOGGLEABLE_AGENTS` 4종(`security-agent`, `promql-agent`, `observability-agent`, `rca-agent`) 매니페스트와 독립 YAML(`scripts/k8s/security-agent.yaml:1`, `scripts/k8s/remote-reader/remote-reader-agent.yaml:23`) 모두 `apiVersion: kagent.dev/v1alpha2`, `kind: Agent` 사용<br>• `spec.type: Declarative` 및 `spec.declarative.{runtime: python, modelConfig: default-model-config, systemMessage, deployment.resources, tools}` 구조에 결합<br>• 단위 테스트 `assert_parses_as_yaml_document`가 `apiVersion: kagent.dev/v1alpha2\n` 접두사를 단언(`src-tauri/src/services/kagent_agents.rs:211`)<br>• 비활성화 시 `kubectl delete agent.kagent.dev <name> -n kagent` 호출(`src-tauri/src/commands/kagent.rs:314`) |
| **RemoteMCPServer (MCP 도구 서버)** | `src-tauri/src/services/kagent_agents.rs:50`<br>`src-tauri/src/services/kagent_agents.rs:128`<br>`scripts/k8s/security-agent.yaml:28`<br>`scripts/k8s/remote-reader/remote-reader-agent.yaml:6`<br>`scripts/k8s/remote-reader/remote-reader-agent.yaml:53`<br>`scripts/k8s/remote-reader/remote-reader-tools.yaml:38`<br>`scripts/k8s/remote-reader/setup-remote-reader.sh:45`<br>`scripts/k8s/remote-reader/setup-remote-reader.sh:162` | • Agent의 `spec.declarative.tools[]`가 `type: McpServer` 및 `mcpServer: { apiGroup: kagent.dev, kind: RemoteMCPServer, name: kagent-tool-server, toolNames: [...] }` 구조로 도구 바인딩(`src-tauri/src/services/kagent_agents.rs:50`, `128`)<br>• 원격 클러스터 리더가 `apiVersion: kagent.dev/v1alpha2`, `kind: RemoteMCPServer`와 `spec.{protocol: STREAMABLE_HTTP, url, sseReadTimeout, terminateOnClose, timeout}` 선언(`scripts/k8s/remote-reader/remote-reader-agent.yaml:6`)<br>• 도구 서버 배포가 `ghcr.io/kagent-dev/kagent/tools:0.2.1` 이미지를 사용하며(`scripts/k8s/remote-reader/remote-reader-tools.yaml:38`), `scripts/k8s/remote-reader/setup-remote-reader.sh:45`가 클러스터의 `deploy/kagent-tools -n kagent` 이미지와 일치 여부를 검사하고 `status.discoveredTools[*].name` 경로를 안내(`scripts/k8s/remote-reader/setup-remote-reader.sh:162`) |
| **Air-Gap (오프라인 번들·버전 추적)** | `scripts/airgap/images-helm.txt:9`<br>`src-tauri/src/commands/colima.rs:265`<br>`src-tauri/src/commands/colima.rs:572`<br>`src-tauri/src/commands/colima.rs:579`<br>`src-tauri/src/commands/colima.rs:701`<br>`scripts/airgap/download_airgap_bundle.sh:127`<br>`scripts/airgap/install_from_airgap.sh:178`<br>`scripts/airgap/test_sbom.sh:42`<br>`scripts/airgap/test_digest_lock.sh:150`<br>`scripts/airgap/test_digest_lock.sh:224`<br>`scripts/airgap/test_digest_lock.sh:357` | • `scripts/airgap/images-helm.txt:9`~`17`에 `cr.kagent.dev/kagent-dev/kagent/{controller,app,ui}:0.9.12`, `ghcr.io/kagent-dev/kagent/tools:0.2.1`, `ghcr.io/kagent-dev/kmcp/controller:0.3.0`, `postgres:18.3-alpine` 고정<br>• `STATIC_AIRGAP_TARGETS`(`src-tauri/src/commands/colima.rs:265`~`311`)가 차트 파일명(`charts/kagent-crds-0.9.12.tgz`, `charts/kagent-0.9.12.tgz`)과 이미지 아카이브 경로를 보유하며 `static_airgap_targets_match_images_helm_txt`(`src-tauri/src/commands/colima.rs:701`)가 드리프트를 차단<br>• `get_airgap_latest_versions`가 `"0.9.12"` 리터럴로 릴리스 비교(`src-tauri/src/commands/colima.rs:572`, `579`)<br>• 에어갭 수집(`scripts/airgap/download_airgap_bundle.sh:127`), 오프라인 설치(`scripts/airgap/install_from_airgap.sh:178`), SBOM·다이제스트 락 테스트 픽스처(`scripts/airgap/test_sbom.sh:42`, `scripts/airgap/test_digest_lock.sh:150`, `224`, `357`)가 `0.9.12` 차트 파일명에 결합 |

### 2. 1.x 전환 시 미검증 사항 및 검증 체크리스트 (`[unverified]`)

외부 조사 덤프의 `alpha17`(`1.0.0-alpha17`) 기준 관찰 내용은 조사 시점의 스냅샷일 뿐이므로, 실제 1.x 업그레이드 착수 시 아래 항목을 모두 클러스터에서 실측해야 한다.

- [ ] **Helm 차트 구조 및 values 호환성 `[unverified]`**: `oci://ghcr.io/kagent-dev/kagent/helm/{kagent-crds,kagent}` 두 차트 분리 구조 유지 여부 `[unverified]`, `providers.openAI.{apiKey, apiKeySecretKey, apiKeySecretRef, config.baseUrl, model, provider}` values 경로 및 `kagent-openai` Secret 생성 가드 동작 유지 여부 `[unverified]`, `scripts/helm/kagent-values.yaml:23`~`42`의 기본 에이전트 비활성화 키 이름 유지 여부 `[unverified]`, `svc/kagent-ui:8080` 서비스 이름·포트 및 파드 `app.kubernetes.io/name` 라벨 규약 유지 여부 `[unverified]`.
- [ ] **CRD API 버전(`v1alpha2` → `v1alpha3`/`v1beta1`/`v1`) 및 전환 웹훅 `[unverified]`**: 1.x(조사 당시 `alpha17` 포함)에서 `Agent`, `ModelConfig`, `RemoteMCPServer`의 `apiVersion` 승격 여부 및 `v1alpha2` served/storage 유지 여부 `[unverified]`. 변경 시 `src-tauri/src/services/kagent_agents.rs:21`·`62`·`83`·`110`·`211`, `scripts/k8s/security-agent.yaml:1`, `scripts/k8s/remote-reader/remote-reader-agent.yaml:6`·`23` 일괄 갱신 필요.
- [ ] **ModelConfig 스키마 및 로컬 MLX 브리지 연동 `[unverified]`**: 차트가 생성하는 기본 리소스명(`default-model-config`) 유지 여부 `[unverified]`, `spec.openAI.baseUrl` 및 `spec.model` JSON 경로(`src-tauri/src/commands/kagent.rs:601`) 유지 여부 `[unverified]`, D10 브리지(`http://mac-gpu-service.<ns>.svc.cluster.local:<port>/v1`) 경유 OpenAI 호환 추론 및 tool calling 정상 동작 여부 `[unverified]`.
- [ ] **Declarative Agent 스키마 및 런타임 동작 `[unverified]`**: `spec.type: Declarative` 및 `spec.declarative.{runtime, modelConfig, systemMessage, deployment.resources, tools}` 필드 구조 유지 여부 `[unverified]`, `rca-agent`의 리소스 미지정 시 컨트롤러 기본 메모리 한도 및 OOM 회피 동작 유지 여부 `[unverified]`.
- [ ] **RemoteMCPServer 및 `kagent-tools` 호환성 `[unverified]`**: 내장 도구 서버 리소스명(`kagent-tool-server`)과 도구 이름(`k8s_get_resources`, `k8s_describe_resource`, `k8s_get_events` 등) 유지 여부 `[unverified]`, `RemoteMCPServer`의 `spec.{protocol: STREAMABLE_HTTP, url, sseReadTimeout, terminateOnClose, timeout}` 및 `status.discoveredTools[*].name` 스키마 유지 여부 `[unverified]`, `deploy/kagent-tools` 배포 이름·컨테이너 이미지(`ghcr.io/kagent-dev/kagent/tools`)·플래그(`--port`, `--metrics-port`) 유지 여부 `[unverified]`.
- [ ] **Air-Gap 번들 및 이미지 레지스트리 동기화 `[unverified]`**: 1.x 차트가 참조하는 컨테이너 이미지 목록(`controller`, `app`, `ui`, `tools`, `kmcp`, `postgres` 태그 및 레지스트리 경로) 실측 추출 `[unverified]`, `scripts/helm/kagent-version.txt:1` 갱신 시 `scripts/airgap/images-helm.txt:9`, `src-tauri/src/commands/colima.rs:265`·`572`·`579`, `scripts/airgap/download_airgap_bundle.sh:127`, `scripts/airgap/install_from_airgap.sh:178`, `scripts/airgap/test_sbom.sh:42`, `scripts/airgap/test_digest_lock.sh:150`·`224`·`357` 동시 갱신 및 `make verify` 통과 확인.

## Round 4 / #17 installed-not-ready — 실클러스터 관측 (measured-local, 2026-11-13)

PR #238(kagent가 설치됐지만 준비되지 않으면 degraded)의 집계를 앱 전용 colima(`colima-kubemetal`)에서 관측했다.
증거와 한계는 `evidence/health/round4-20261113/README.md`.

- 기준: default·kagent 네임스페이스 모두 Ready, 카드 **정상**(설치됨 예 / 준비됨 예 / 파드 문제 0).
- kagent 네임스페이스에 스케줄되지 않는 진단 Pod 1개 추가 → 카드 **저하**(준비됨 아니요, 파드 문제 0, 다른 항목 불변).
- Pod가 사라진 뒤 → 카드 **정상** 복귀.
- 한계: 실제 컨트롤러 고장이 아니라 readiness 집계 관측이다. 진단 Pod는 삭제 명령 전에 Kubernetes pod GC로 먼저 사라졌다.
  IPC 원문 JSON은 수집하지 않았고 화면 캡처로 읽었다.
- 부수 발견: 이 Mac의 저장된 배포 대상이 `colima-default`/`kubemetal`로 남아 있었다(그 컨텍스트는 더 이상 없다).
  이 상태에서는 스택이 떠 있어도 MLflow/SeaweedFS가 "준비: 아니요"로 보인다 — D43(전용 프로필) 이후 저장값이
  갱신되지 않은 것으로 보이며[추정], 별도 확인이 필요하다.
### 3. Artifact-only probe / no install (#193 실측 대조, 2026-11-13)

> `evidence/kagent/round4-chart/`에 원본 stdout/stderr/rc, SHA256 해시, 렌더 결과, CRD OpenAPI diff 보존 `[measured]`. 설치(`helm install`/`upgrade`)·클러스터(`kubectl apply`) 변경 없이 순수 OCI 아티팩트(`0.9.12` vs `1.0.0-alpha17`)만 비교 검증함.

#### A. 다운로드 아티팩트 해시 및 검증 커맨드 `[measured]`

- `oci://ghcr.io/kagent-dev/kagent/helm` 레지스트리에서 `helm pull` 수행 (임시 디렉터리 `/tmp/kagent-chart-probe`) `[measured]`.
- 아티팩트 SHA256 `[measured]`:
  - `kagent-0.9.12.tgz`: `a6aede4fd39aab4ea0292b68a59ed2b0e41afc50f1e8de8a9a2ead2a89bd5075`
  - `kagent-1.0.0-alpha17.tgz`: `9260dad01ef544c684ca7d749e31b46443100192c318281c4fdffe8e79d86543`
  - `kagent-crds-0.9.12.tgz`: `977e59c4ad7f751f2badea53d5f698220f37a1af28fb282fd7917103a084e7f3`
  - `kagent-crds-1.0.0-alpha17.tgz`: `8c236ee9cd3730a2275814d048fccf51a35d197ff5e8f624623bdf4cc415c431`
- `helm show crds`는 두 버전 모두 0 bytes 반환 (차트의 CRD YAML이 `crds/`가 아닌 `templates/`에 패키징됨) `[measured]`.

#### B. CRD 그룹·버전·종류 전수 대조 `[measured]`

| Kind | 0.9.12 (`kagent-crds:0.9.12`) | 1.0.0-alpha17 (`kagent-crds:1.0.0-alpha17`) | 판정 및 호환성 |
|------|--------------------------------|---------------------------------------------|----------------|
| **`Agent`** | `kagent.dev` (`v1alpha1`, `v1alpha2`) | `api.kagent.dev` (`v1alpha3` 단독) | **비호환 (Breaking)**: 그룹 변경, `v1alpha2` 미제공(served=false/storage=false조차 없음). `src-tauri/src/services/kagent_agents.rs:110` 거부 |
| **`AgentHarness` / `Harness`** | `kagent.dev/v1alpha2` (`AgentHarness`) | `api.kagent.dev/v1alpha3` (`Harness`) | **비호환 (Breaking)**: Kind 이름 `AgentHarness` → `Harness` 변경 |
| **`AgentTemplate`** | 없음 | `api.kagent.dev/v1alpha3` (`AgentTemplate`) | **신규 도입**: 공용 에이전트 템플릿 선언 리소스 |
| **`EgressPolicy`** | 없음 | `api.kagent.dev/v1alpha3` (`EgressPolicy`) | **신규 도입**: 에이전트 외부 통신 제어 정책 |
| **`ModelConfig`** | `kagent.dev` (`v1alpha1`, `v1alpha2`) | `api.kagent.dev` (`v1alpha3` 단독) | **그룹/버전 변경**: `api.kagent.dev/v1alpha3` 승격. 스키마 필드는 호환 |
| **`ModelProviderConfig`** | `kagent.dev/v1alpha2` | `api.kagent.dev/v1alpha3` | **그룹/버전 변경** |
| **`RemoteMCPServer`** | `kagent.dev/v1alpha2` | `api.kagent.dev/v1alpha3` | **그룹/버전 변경**: `api.kagent.dev/v1alpha3` 승격. 스키마 필드는 호환 |
| **`SandboxAgent` / `SandboxTemplate`** | `kagent.dev/v1alpha2` (`SandboxAgent`) | `api.kagent.dev/v1alpha3` (`SandboxTemplate`) | **비호환 (Breaking)**: Kind 이름 변경 |
| **`Memory`, `ToolServer`** | `kagent.dev/v1alpha1` | 없음 | **제거됨** |
| **`kmcp-crds`, `substrate-crds`** | subchart로 내장 (2.2MB) | chart 의존성에서 완전 제거 (146KB) | **제거됨** |

#### C. 코드베이스 필드별 호환성 및 Breaking Change 분석 `[measured]`

1. **`Agent` 매니페스트 (`src-tauri/src/services/kagent_agents.rs:110`, `scripts/k8s/security-agent.yaml:1`)**:
   - `apiVersion: kagent.dev/v1alpha2`는 CRD 부재로 즉시 거부됨 `[measured]`.
   - `spec.harnessRef`가 **필수 필드 (`required: [harnessRef, template]`)** 로 추가됨 `[measured]`.
   - `spec.type: Declarative` 및 `spec.declarative` 블록 전체가 제거됨 `[measured]`.
   - 모델 설정은 `spec.template.inline.modelConfig: {name: "default-model-config"}` 객체 형태로 변경 (문자열 리터럴 불가) `[measured]`.
   - `systemMessage`는 `systemPrompt`로 이름 변경 `[measured]`.
   - 도구 바인딩: `tools[].mcpServer` → `tools[].mcp`로 변경, `toolNames` → `tools`로 변경, CEL 유효성 검사 `!has(self.apiGroup)`에 의해 `apiGroup: kagent.dev` 명시 시 거부됨 `[measured]`.
2. **`ModelConfig` 및 로컬 MLX 브리지 (`src-tauri/src/commands/kagent.rs:601,670`)**:
   - `default-model-config` 이름 및 `spec.openAI.baseUrl`, `spec.model` JSON 경로는 1.0.0-alpha17에서도 **완전히 동일하게 유지됨** `[measured]`.
   - `configure_kagent_model`의 values 주입 구조(`providers.openAI.config.baseUrl`, `providers.openAI.model`)가 1.0.0-alpha17 템플릿에서도 동일하게 파싱되어 `spec.openAI.baseUrl`로 렌더링됨 `[measured]`.
3. **UI 서비스 포트 및 진단 파드 라벨 (`src-tauri/src/commands/port_forward.rs:37`, `kagent.rs:168`)**:
   - UI 서비스 이름 `kagent-ui` (템플릿 기준 `<release>-ui`) 및 포트 `8080` 동일 유지 `[measured]`.
   - 컨트롤러 및 UI 파드의 `app.kubernetes.io/name: kagent` 라벨 동일 유지 `[measured]`.
4. **`kagent-values.yaml` 키 소비 분석 `[measured]`**:
   - `providers.openAI.apiKey: dummy-local-key-not-used`: 1.0.0-alpha17의 `modelconfig-secret.yaml`에서도 `{{- if and $model.apiKeySecretRef $model.apiKey }}` 가드가 유지되어 **정상 소비**되며 `Secret/kagent-openai`가 생성됨 `[measured]`.
   - `grafana-mcp.enabled: false`: **정상 소비**되어 grafana-mcp 관련 모든 워크로드가 비활성화됨 `[measured]`.
   - 기본 에이전트 비활성화 키(`kgateway-agent`, `promql-agent`, `observability-agent` 등): 1.0.0-alpha17 차트에는 기본 에이전트 서브차트가 전혀 포함되어 있지 않으므로(기본 렌더 시 에이전트 0개), 이 키들은 에러 없이 무시됨(inert) `[measured]`.
5. **설치 차단 요인 (Architectural Install Blocker) `[measured]`**:
   - 0.9.12에 내장되어 있던 `postgresql` 파드 배포가 1.0.0-alpha17 차트에서 **완전 제거됨** `[measured]`.
   - 1.0.0-alpha17은 외부 PostgreSQL 시크릿(`kagent-postgres-owner`, `kagent-postgres-readwrite`) 연결 정보를 필수로 요구함 (`values.yaml:108`~`124`) `[measured]`. 별도 DB 프로비저닝 없이 1.0.0-alpha17 단독 설치 시 파드가 기동하지 않음.
6. **잔여 미검증 영역 (`[unverified]`)**:
   - 본 프로브는 OCI 아티팩트 및 Helm 템플릿 정적 대조로 한정되었으므로, 실제 K3s 클러스터 내 컨트롤러 기동, Harness 파드 스케줄링, D10 브리지 경유 런타임 추론 및 MCP 도구 호출 성공 여부는 여전히 클러스터 설치 전까지 **`[unverified]`** 로 유지됨.

