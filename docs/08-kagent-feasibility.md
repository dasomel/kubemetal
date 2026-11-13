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
### 3. Artifact-only probe / no install (#193, 2026-11-13)

[measured — see `evidence/kagent/round4-chart/kagent_0_9_12_show_chart.stdout.txt` and `evidence/kagent/round4-chart/kagent_1_0_0_alpha17_show_chart.stdout.txt`] 저장된 main Chart.yaml 메타데이터의 버전은 각각 `0.9.12`, `1.0.0-alpha17`이다.

[measured — see `evidence/kagent/round4-chart/sha256sums.txt`] 기존 tgz SHA256 기록은 다음과 같다. 현재 tarball은 저장소에 없으므로 재다운로드와 일치 여부는 [pending probe]이다.

| Archive | Recorded SHA256 |
|---|---|
| kagent-0.9.12.tgz | a6aede4fd39aab4ea0292b68a59ed2b0e41afc50f1e8de8a9a2ead2a89bd5075 |
| kagent-1.0.0-alpha17.tgz | 9260dad01ef544c684ca7d749e31b46443100192c318281c4fdffe8e79d86543 |
| kagent-crds-0.9.12.tgz | 977e59c4ad7f751f2badea53d5f698220f37a1af28fb282fd7917103a084e7f3 |
| kagent-crds-1.0.0-alpha17.tgz | 8c236ee9cd3730a2275814d048fccf51a35d197ff5e8f624623bdf4cc415c431 |

[measured — see `evidence/kagent/round4-chart/kagent-crds_1_0_0_alpha17_template_default.stdout.txt`] 렌더된 Agent CRD는 `api.kagent.dev` 그룹, `Agent` kind, `v1alpha3` 버전 이름을 가진다.

[pending probe] 두 버전의 전체 group/kind/served/storage 목록과 Agent·ModelConfig·RemoteMCPServer의 flattened property/type 차이는 `probe.sh`의 `*-inventory.json`, `*-paths.json`, `schema-diff.json`으로 확인해야 한다. 이전의 0.9.12 도식에 근거한 필드 제거·이동·이름 변경 및 호환 판정은 철회한다. 앱 출력의 근거와 1.x 대조 상태는 `evidence/kagent/round4-chart/code-compatibility-matrix.md`에 기록한다.

[measured — see `src-tauri/src/services/kagent_agents.rs:110`] 앱의 rca-agent는 `kagent.dev/v1alpha2`를 출력한다. [measured — see `src-tauri/src/services/kagent_agents.rs:120`] `spec.type: Declarative`와 `spec.declarative`를 출력한다. 이 구조와 1.x의 필드별 비교는 [pending probe]이다.

[measured — see `evidence/kagent/round4-chart/kagent_1_0_0_alpha17_show_values.stdout.txt`] PostgreSQL values 기본값은 `kagent-postgres-owner`, `kagent-postgres-readwrite` Secret을 참조하며 read/write 주석에 `Required.`가 있다. [measured — see `evidence/kagent/round4-chart/kagent_1_0_0_alpha17_template_default.rc.txt` and `evidence/kagent/round4-chart/kagent_1_0_0_alpha17_template_default.stderr.txt`] 저장된 기본 템플릿 rc는 0이고 DB 관련 템플릿 오류는 기록되어 있지 않다.

[unverified] 설치 성공 여부, DB 연결, 컨트롤러·Harness 기동, API admission/CEL, D10 추론, MCP 도구 호출 및 UI 동작은 이 아티팩트로 검증되지 않았다.

[pending probe] 호스트 실행 명령: `bash evidence/kagent/round4-chart/probe.sh evidence/kagent/round4-chart/host-probe 0.9.12 1.0.0-alpha17`. 새 출력 디렉터리가 필요하며 Helm·네트워크·PyYAML은 호스트에 준비되어 있어야 한다. 명령별 argv/rc/stdout/stderr 및 기존 출력의 해시 인덱스는 `evidence/kagent/round4-chart/execution_summary.md`를 참조한다.
