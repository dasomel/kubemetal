//! `toggle_kagent_agent`가 설치하는 Agent CRD 매니페스트의 단일 출처(#2/#26/#27).
//! `commands/kagent.rs`가 ~890줄로 이미 길어(AGENTS.md "파일이 ~300줄을 넘으면
//! 새 순수 로직은 services/ 아래로") CRD 문자열과 그 순수 조회 로직만 여기로
//! 옮긴다 — 매니페스트를 실제로 적용/삭제하는 부수효과(kubectl 호출)는 그대로
//! `commands/kagent.rs`에 남는다.

/// 이 앱이 Agent CRD로 직접 설치/삭제할 수 있는 에이전트 목록. `agent_manifest`의
/// match 분기와 1:1로 대응한다 — k8s-agent·helm-agent는 Helm 차트가 관리하므로 제외.
pub(crate) const TOGGLEABLE_AGENTS: [&str; 4] = [
    "security-agent",
    "promql-agent",
    "observability-agent",
    "rca-agent",
];

/// `agent_name`이 설치 가능한 이름이면 해당 Agent CRD YAML을, 아니면 `None`을 돌려준다.
/// 모르는 이름을 지어내지 않는다(D22) — 호출부가 `TOGGLEABLE_AGENTS`를 인용해 에러를 만든다.
pub(crate) fn agent_manifest(agent_name: &str) -> Option<&'static str> {
    match agent_name {
        "security-agent" => Some(
            r#"apiVersion: kagent.dev/v1alpha2
kind: Agent
metadata:
  name: security-agent
  namespace: kagent
  labels:
    app.kubernetes.io/instance: kagent
    app.kubernetes.io/name: security-agent
    app.kubernetes.io/part-of: kagent
spec:
  type: Declarative
  description: Kubernetes Security, Vulnerability, and RBAC Audit Agent.
  declarative:
    runtime: python
    modelConfig: default-model-config
    systemMessage: |
      You are SecurityAssist, a specialized AI agent for Kubernetes security and vulnerability scanning.
    deployment:
      resources:
        limits:
          cpu: 500m
          memory: 256Mi
        requests:
          cpu: 50m
          memory: 128Mi
    tools:
    - type: McpServer
      mcpServer:
        apiGroup: kagent.dev
        kind: RemoteMCPServer
        name: kagent-tool-server
        toolNames:
        - k8s_get_resources
        - k8s_describe_resource
        - k8s_get_events
"#,
        ),
        "promql-agent" => Some(
            r#"apiVersion: kagent.dev/v1alpha2
kind: Agent
metadata:
  name: promql-agent
  namespace: kagent
  labels:
    app.kubernetes.io/instance: kagent
    app.kubernetes.io/name: promql-agent
spec:
  type: Declarative
  description: Prometheus & PromQL Metrics Diagnostics Agent.
  declarative:
    runtime: python
    modelConfig: default-model-config
    systemMessage: |
      You are PromQLAssist, an AI agent for cluster metrics and Prometheus analysis.
"#,
        ),
        "observability-agent" => Some(
            r#"apiVersion: kagent.dev/v1alpha2
kind: Agent
metadata:
  name: observability-agent
  namespace: kagent
  labels:
    app.kubernetes.io/instance: kagent
    app.kubernetes.io/name: observability-agent
spec:
  type: Declarative
  description: Kubernetes Telemetry & Observability Diagnostics Agent.
  declarative:
    runtime: python
    modelConfig: default-model-config
    systemMessage: |
      You are ObservabilityAssist, an AI agent analyzing OpenTelemetry and trace data.
"#,
        ),
        // #2/#26/#27 승인된 축소 MVP. 실측(2026-08-26, colima): security-agent의
        // 256Mi 한도(도구 미사용 워크로드 기준)를 그대로 복사했다가 exit 137
        // OOMKilled — rca-agent는 k8s_get_events/k8s_get_resources/
        // k8s_describe_resource 3개 도구를 호출해 promql-agent/observability-agent
        // 쪽 워크로드에 가까우므로, 그 둘처럼 resources 블록 자체를 두지 않아
        // 컨트롤러 기본값(관측상 더 넉넉함)을 받는다.
        "rca-agent" => Some(
            r#"apiVersion: kagent.dev/v1alpha2
kind: Agent
metadata:
  name: rca-agent
  namespace: kagent
  labels:
    app.kubernetes.io/instance: kagent
    app.kubernetes.io/name: rca-agent
    app.kubernetes.io/part-of: kagent
spec:
  type: Declarative
  description: Kubernetes root-cause hypothesis agent grounded in Warning events and restarted pods.
  declarative:
    runtime: python
    modelConfig: default-model-config
    systemMessage: |
      You are RCAAssist, a Kubernetes root-cause analysis agent. In the target namespace, first use k8s_get_events to read the most recent Warning-type events, then use k8s_get_resources to find pods that have restarted recently and k8s_describe_resource to inspect the relevant pods. Produce exactly one line: a root-cause hypothesis that explicitly cites the specific Warning event and/or restarted pod evidence used. Only state a root-cause hypothesis when a returned Warning-type event or pod description expressly reports or establishes the cause; do not infer causation from correlation or give vague or hedged non-answers. Otherwise output exactly "insufficient evidence" and do not invent a plausible cause.
    tools:
    - type: McpServer
      mcpServer:
        apiGroup: kagent.dev
        kind: RemoteMCPServer
        name: kagent-tool-server
        toolNames:
        - k8s_get_resources
        - k8s_describe_resource
        - k8s_get_events
"#,
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggleable_agents_and_manifest_match_lists_agree() {
        for name in TOGGLEABLE_AGENTS {
            assert!(
                agent_manifest(name).is_some(),
                "TOGGLEABLE_AGENTS lists [{name}] but agent_manifest has no branch for it"
            );
        }
    }

    #[test]
    fn unknown_agent_name_returns_none() {
        assert!(agent_manifest("k8s-agent").is_none());
        assert!(agent_manifest("not-a-real-agent").is_none());
    }

    /// 실측(2026-08-26, colima): 256Mi 한도에서 exit 137 OOMKilled(352a88d) — 리그레션 가드.
    #[test]
    fn rca_agent_manifest_has_no_resources_block() {
        let manifest = agent_manifest("rca-agent").expect("rca-agent manifest missing");
        assert!(
            !manifest.contains("resources:"),
            "rca-agent must not declare a resources block — see 352a88d OOMKill fix"
        );
    }

    #[test]
    fn rca_agent_manifest_wires_the_three_evidence_tools() {
        let manifest = agent_manifest("rca-agent").expect("rca-agent manifest missing");
        for tool in [
            "k8s_get_events",
            "k8s_get_resources",
            "k8s_describe_resource",
        ] {
            assert!(
                manifest.contains(tool),
                "rca-agent manifest is missing tool [{tool}]"
            );
        }
    }

    #[test]
    fn rca_agent_manifest_follows_existing_namespace_and_label_convention() {
        let manifest = agent_manifest("rca-agent").expect("rca-agent manifest missing");
        assert!(manifest.contains("namespace: kagent"));
        assert!(manifest.contains("app.kubernetes.io/instance: kagent"));
        assert!(manifest.contains("app.kubernetes.io/name: rca-agent"));
        assert!(manifest.contains("modelConfig: default-model-config"));
    }

    /// 근거 없는 단정을 금지하는 원본 취지(D22와 같은 계열) — "모르면 모른다"를
    /// 시스템 프롬프트가 요구하는지 확인한다.
    #[test]
    fn rca_agent_system_message_forbids_unfounded_hypotheses() {
        let manifest = agent_manifest("rca-agent").expect("rca-agent manifest missing");
        assert!(manifest.contains("insufficient evidence"));
        assert!(manifest.contains("do not invent a plausible cause"));
    }
}
