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

> 현재 리포지토리는 차트 버전 `0.9.12`(`scripts/helm/kagent-version.txt:1`)와 CRD `kagent.dev/v1alpha2` Declarative 스키마(`src-tauri/src/services/kagent_agents.rs:110`)에 고정되어 있다. 외부 조사 덤프에 언급된 `alpha17`(`1.0.0-alpha17`)은 조사 당시 기준의 프리릴리스 관찰값이다. 정식 1.x 및 runtime 하위 호환성은 [unverified]이며, 아래 §3은 커밋된 alpha17 host-probe 아티팩트만 비교한다.

### 1. 현재 리포지토리 결합 지점 (`0.9.12` / `v1alpha2` 기준)

| 영역 | 결합 파일 및 라인 (`file:line`) | 현재 결합 내용 (실측·고정된 규약) |
|------|--------------------------------|-----------------------------------|
| **설치 (Helm·진단·UI 포워드)** | `scripts/helm/kagent-version.txt:1`<br>`src-tauri/src/commands/kagent.rs:21`<br>`src-tauri/src/commands/kagent.rs:30`<br>`src-tauri/src/commands/kagent.rs:338`<br>`src-tauri/src/commands/kagent.rs:413`<br>`src-tauri/src/commands/kagent.rs:425`<br>`src-tauri/src/commands/kagent.rs:427`<br>`src-tauri/src/commands/kagent.rs:168`<br>`Makefile:244`<br>`Makefile:249`<br>`Makefile:251`<br>`Makefile:289`<br>`src-tauri/src/commands/port_forward.rs:37`<br>`scripts/helm/kagent-values.yaml:19` | • 차트 버전 `0.9.12` 단일 출처(`scripts/helm/kagent-version.txt:1`)를 Rust(`include_str!`)와 `Makefile`(`KAGENT_VERSION`)이 공유<br>• `oci://ghcr.io/kagent-dev/kagent/helm` 레지스트리에서 `kagent-crds` 선행 설치 후 `kagent` 본 차트를 `-n kagent`에 설치<br>• `scripts/helm/kagent-values.yaml:19`에서 `providers.openAI.apiKey` 더미 값 주입 및 불필요 기본 에이전트(`scripts/helm/kagent-values.yaml:23`~`42`) 비활성화<br>• `get_kagent_diagnostics`가 파드의 `metadata.labels["app.kubernetes.io/name"]`으로 활성 에이전트 식별(`src-tauri/src/commands/kagent.rs:168`)<br>• UI 포트포워드가 `-n kagent svc/kagent-ui`의 포트 `8080`을 호스트 `8090`으로 연결(`Makefile:289`, `src-tauri/src/commands/port_forward.rs:37`) |
| **ModelConfig (LLM 백엔드)** | `src-tauri/src/commands/kagent.rs:438`<br>`src-tauri/src/commands/kagent.rs:508`<br>`src-tauri/src/commands/kagent.rs:601`<br>`src-tauri/src/commands/kagent.rs:670`<br>`src-tauri/src/commands/kagent.rs:702` | • 고정 리소스명 `default-model-config`(`src-tauri/src/commands/kagent.rs:438`)를 `kubectl -n kagent get modelconfig default-model-config -o json`으로 조회(`src-tauri/src/commands/kagent.rs:508`)<br>• 응답 JSON에서 `spec.openAI.baseUrl` 및 `spec.model` 경로를 직접 파싱해 브리지 URL·포트·모델 ID 일치 판정(`src-tauri/src/commands/kagent.rs:601`)<br>• `configure_kagent_model`이 `providers.default: openAI` 및 `providers.openAI.{apiKey, apiKeySecretKey: OPENAI_API_KEY, apiKeySecretRef: kagent-openai, config.baseUrl, model, provider: OpenAI}` 구조의 임시 values 파일로 `helm upgrade` 실행(`src-tauri/src/commands/kagent.rs:670`)<br>• 단위 테스트가 `scripts/helm/kagent-values.yaml`의 `providers:` 및 `apiKey:` 존재를 강제(`src-tauri/src/commands/kagent.rs:702`) |
| **Agent (CRD 매니페스트·토글)** | `src-tauri/src/services/kagent_agents.rs:9`<br>`src-tauri/src/services/kagent_agents.rs:21`<br>`src-tauri/src/services/kagent_agents.rs:62`<br>`src-tauri/src/services/kagent_agents.rs:83`<br>`src-tauri/src/services/kagent_agents.rs:110`<br>`src-tauri/src/services/kagent_agents.rs:211`<br>`src-tauri/src/commands/kagent.rs:285`<br>`src-tauri/src/commands/kagent.rs:314`<br>`scripts/k8s/security-agent.yaml:1`<br>`scripts/k8s/remote-reader/remote-reader-agent.yaml:23` | • `TOGGLEABLE_AGENTS` 4종(`security-agent`, `promql-agent`, `observability-agent`, `rca-agent`) 매니페스트와 독립 YAML(`scripts/k8s/security-agent.yaml:1`, `scripts/k8s/remote-reader/remote-reader-agent.yaml:23`) 모두 `apiVersion: kagent.dev/v1alpha2`, `kind: Agent` 사용<br>• `spec.type: Declarative` 및 `spec.declarative.{runtime: python, modelConfig: default-model-config, systemMessage, deployment.resources, tools}` 구조에 결합<br>• 단위 테스트 `assert_parses_as_yaml_document`가 `apiVersion: kagent.dev/v1alpha2\n` 접두사를 단언(`src-tauri/src/services/kagent_agents.rs:211`)<br>• 비활성화 시 `kubectl delete agent.kagent.dev <name> -n kagent` 호출(`src-tauri/src/commands/kagent.rs:314`) |
| **RemoteMCPServer (MCP 도구 서버)** | `src-tauri/src/services/kagent_agents.rs:50`<br>`src-tauri/src/services/kagent_agents.rs:128`<br>`scripts/k8s/security-agent.yaml:28`<br>`scripts/k8s/remote-reader/remote-reader-agent.yaml:6`<br>`scripts/k8s/remote-reader/remote-reader-agent.yaml:53`<br>`scripts/k8s/remote-reader/remote-reader-tools.yaml:38`<br>`scripts/k8s/remote-reader/setup-remote-reader.sh:45`<br>`scripts/k8s/remote-reader/setup-remote-reader.sh:162` | • Agent의 `spec.declarative.tools[]`가 `type: McpServer` 및 `mcpServer: { apiGroup: kagent.dev, kind: RemoteMCPServer, name: kagent-tool-server, toolNames: [...] }` 구조로 도구 바인딩(`src-tauri/src/services/kagent_agents.rs:50`, `128`)<br>• 원격 클러스터 리더가 `apiVersion: kagent.dev/v1alpha2`, `kind: RemoteMCPServer`와 `spec.{protocol: STREAMABLE_HTTP, url, sseReadTimeout, terminateOnClose, timeout}` 선언(`scripts/k8s/remote-reader/remote-reader-agent.yaml:6`)<br>• 도구 서버 배포가 `ghcr.io/kagent-dev/kagent/tools:0.2.1` 이미지를 사용하며(`scripts/k8s/remote-reader/remote-reader-tools.yaml:38`), `scripts/k8s/remote-reader/setup-remote-reader.sh:45`가 클러스터의 `deploy/kagent-tools -n kagent` 이미지와 일치 여부를 검사하고 `status.discoveredTools[*].name` 경로를 안내(`scripts/k8s/remote-reader/setup-remote-reader.sh:162`) |
| **Air-Gap (오프라인 번들·버전 추적)** | `scripts/airgap/images-helm.txt:9`<br>`src-tauri/src/commands/colima.rs:265`<br>`src-tauri/src/commands/colima.rs:572`<br>`src-tauri/src/commands/colima.rs:579`<br>`src-tauri/src/commands/colima.rs:701`<br>`scripts/airgap/download_airgap_bundle.sh:127`<br>`scripts/airgap/install_from_airgap.sh:178`<br>`scripts/airgap/test_sbom.sh:42`<br>`scripts/airgap/test_digest_lock.sh:150`<br>`scripts/airgap/test_digest_lock.sh:224`<br>`scripts/airgap/test_digest_lock.sh:357` | • `scripts/airgap/images-helm.txt:9`~`17`에 `cr.kagent.dev/kagent-dev/kagent/{controller,app,ui}:0.9.12`, `ghcr.io/kagent-dev/kagent/tools:0.2.1`, `ghcr.io/kagent-dev/kmcp/controller:0.3.0`, `postgres:18.3-alpine` 고정<br>• `STATIC_AIRGAP_TARGETS`(`src-tauri/src/commands/colima.rs:265`~`311`)가 차트 파일명(`charts/kagent-crds-0.9.12.tgz`, `charts/kagent-0.9.12.tgz`)과 이미지 아카이브 경로를 보유하며 `static_airgap_targets_match_images_helm_txt`(`src-tauri/src/commands/colima.rs:701`)가 드리프트를 차단<br>• `get_airgap_latest_versions`가 `"0.9.12"` 리터럴로 릴리스 비교(`src-tauri/src/commands/colima.rs:572`, `579`)<br>• 에어갭 수집(`scripts/airgap/download_airgap_bundle.sh:127`), 오프라인 설치(`scripts/airgap/install_from_airgap.sh:178`), SBOM·다이제스트 락 테스트 픽스처(`scripts/airgap/test_sbom.sh:42`, `scripts/airgap/test_digest_lock.sh:150`, `224`, `357`)가 `0.9.12` 차트 파일명에 결합 |

### 2. 전환 전 남은 검증

아래 §3은 0.9.12와 1.0.0-alpha17의 커밋된 아티팩트 비교다. 실제 업그레이드·설치·기동·conversion webhooks·migration·API admission/CEL·기본 리소스명·values 적용·Secret 생성·D10 추론/tool calling·UI 포워드·Air-Gap 이미지 및 번들 호환성은 모두 [unverified]이다. 정식 1.x에도 이 결과를 일반화하지 않는다.

### 3. Artifact-only probe / no install (#193)

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
[measured — host-probe/0-kagent-crds-0.9.12-paths.json; host-probe/1-kagent-crds-1.0.0-alpha17-paths.json] 아래 표의 축약 경로는 [개별 필드·file:line 표](../evidence/kagent/round4-chart/code-compatibility-matrix.md)에서 풀어 썼다. Agent 근거: `src-tauri/src/services/kagent_agents.rs:120`, `scripts/k8s/security-agent.yaml:11`; RemoteMCPServer: `scripts/k8s/remote-reader/remote-reader-agent.yaml:15`. ModelConfig 행은 `scripts/helm/kagent-values.yaml:19`, `src-tauri/src/commands/kagent.rs:670`의 설정과 관련된 스키마 경로 비교이며 실제 Helm CR 출력 매핑은 [unverified]이다.

| Kind / 앱 출력·설정 경로 | v1alpha2 존재/타입 | 같은 v1alpha3 경로/타입 | 가장 가까운 literal 후보 (동등성 미검증) |
|---|---|---|---|
| Agent `spec.type`; `spec.declarative.runtime`; `spec.declarative.tools[].type` | string 각각 | 없음 | no equivalent path found in schema |
| Agent `spec.description` | string | 없음 | `spec.template.inline.description` (string) |
| Agent `spec.declarative.modelConfig` | string | 없음 | `spec.template.inline.modelConfig.name` (string; 부모 modelConfig는 object) |
| Agent `spec.declarative.systemMessage` | string | 없음 | `spec.template.inline.systemPrompt` (string) |
| Agent `spec.declarative` 및 그 하위 `deployment`, `deployment.resources`, `deployment.resources.{limits,requests}` 컨테이너 | object 각각 | 없음 | no equivalent path found in schema |
| Agent `spec.declarative.deployment.resources.{limits,requests}.*` (cpu/memory) | `<unspecified>`; anyOf integer/string | 없음 | no equivalent path found in schema |
| Agent `spec.declarative.tools` / `tools[]` / `tools[].mcpServer` | array / object / object | 없음 | `spec.template.inline.tools` / `tools[]` / `tools[].mcp` (같은 타입) |
| Agent `spec.declarative.tools[].mcpServer.{apiGroup,kind,name}` | string 각각 | 없음 | `spec.template.inline.tools[].mcp.server.{apiGroup,kind,name}` (string 각각) |
| Agent `spec.declarative.tools[].mcpServer.toolNames` / `toolNames[]` | array / string | 없음 | `spec.template.inline.tools[].mcp.tools` / `tools[]` (array / string) |
| ModelConfig `spec.{model,provider,apiKeySecret,apiKeySecretKey}`; `spec.openAI.baseUrl` / `spec.openAI` | string 각각 / object | 모두 있음·동일 타입 | — |
| RemoteMCPServer `spec.{description,protocol,url,sseReadTimeout,timeout}` / `spec.terminateOnClose` | string 각각 / boolean | 모두 있음·동일 타입 | — |

- [measured — host-probe/0-kagent-0.9.12-show-chart.stdout.txt; host-probe/1-kagent-1.0.0-alpha17-show-chart.stdout.txt; host-probe/0-kagent-crds-0.9.12-show-chart.stdout.txt; host-probe/1-kagent-crds-1.0.0-alpha17-show-chart.stdout.txt] 두 차트의 version은 각각 0.9.12 / 1.0.0-alpha17이며 네 Chart.yaml 모두 appVersion 필드가 없다.
- [measured — host-probe/0-kagent-0.9.12-sha256.stdout.txt; host-probe/1-kagent-1.0.0-alpha17-sha256.stdout.txt; host-probe/0-kagent-crds-0.9.12-sha256.stdout.txt; host-probe/1-kagent-crds-1.0.0-alpha17-sha256.stdout.txt] 네 tgz의 전체 SHA256은 [아카이브 표](../evidence/kagent/round4-chart/execution_summary.md)에 기록했다. 현재 checkout에 tgz는 없다.
- [measured — host-probe/0-kagent-crds-0.9.12-inventory.json; host-probe/1-kagent-crds-1.0.0-alpha17-inventory.json] 그룹은 kagent.dev → api.kagent.dev. old Agent/ModelConfig는 v1alpha1(served, non-storage)과 v1alpha2(served, storage), RemoteMCPServer는 v1alpha2(served, storage); new 모든 kind는 v1alpha3(served, storage). [전체 kind 목록](../evidence/kagent/round4-chart/crd-inventory.md).
- [measured — host-probe/0-kagent-crds-0.9.12-inventory.json; host-probe/1-kagent-crds-1.0.0-alpha17-inventory.json] old-only kind: AgentHarness, MCPServer, Memory, SandboxAgent, ToolServer. new-only: AgentTemplate, EgressPolicy, Harness, SandboxTemplate. rename 관계는 [unverified].
- [measured — host-probe/schema-diff.json] v1alpha2 → v1alpha3 전체 경로 added/removed/type_changed: Agent 78/1798/0, ModelConfig 29/8/1, RemoteMCPServer 1/4/0. 앱 필드만의 수치가 아니다.
- [measured — host-probe/commands.json] 네 default template 명령 모두 rc=0, stderr 비어 있음, truncation 없음. 필수값을 증명하는 template 오류는 기록되지 않았다. 앱 custom values는 이 명령에 전달되지 않았다.
- [measured — host-probe/1-kagent-1.0.0-alpha17-show-values.stdout.txt] postgres Secret 참조와 `Required.` 주석은 values에 있지만 template 필수값·DB 기동 증거는 아니다.
- [measured — host-probe/result.json; host-probe/template-outputs.sha256.txt] probe exit_code=0, artifact-only 범위. 삭제된 template stdout 내용은 인용하지 않는다. [오프라인 재현 명령](../evidence/kagent/round4-chart/verification.md).

[unverified] 설치, DB 연결, controller/Harness 기동, API admission/CEL·unknown-field pruning, conversion webhooks, migration, 기본값·필수값·enum, D10 추론, MCP 호출, UI 및 runtime 호환성. 후보 경로의 의미 동등성과 ModelConfig values→CR 매핑도 미검증이다. metadata 하위 필드는 flattened paths에 없어 판정하지 않는다.

[measured — host-probe/0-kagent-crds-0.9.12-paths.json; host-probe/1-kagent-crds-1.0.0-alpha17-paths.json] 결론(표에 한정): 앱의 기존 `kagent.dev/v1alpha2` CR 식별자는 new inventory의 `api.kagent.dev/v1alpha3`와 다르며, Agent의 `spec.type`, `spec.description`, `spec.declarative` 및 그 출력 하위 필드는 1.x 스키마 경로에 없어 그대로는 해당 스키마의 필드 구조와 맞지 않는다. ModelConfig 관련 선택 경로와 RemoteMCPServer 출력 spec 경로는 동일 타입으로 존재한다. 실제 API 서버의 거절·가지치기·변환 결과는 [unverified]이다.
