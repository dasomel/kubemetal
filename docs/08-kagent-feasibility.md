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
