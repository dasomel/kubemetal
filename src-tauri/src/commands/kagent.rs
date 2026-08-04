//! kagent 진단/설치/에이전트 토글 IPC. `colima.rs`에서 이관됨(D33 작업) — colima 자체의
//! 수명주기(start/stop/status)와는 무관하고, kagent 배포 대상은 활성 `DeployTarget`(D26)이다.

use serde::Serialize;
use tauri::Manager;

use crate::commands::deploy_target::get_deploy_target;
use crate::commands::provision::ensure_namespace;
use crate::services::process::{external_command, resolve_bundled_resource};

/// kagent Helm 릴리스 버전의 단일 출처(D33). `Makefile`의 `kagent-up`도 같은 파일을
/// `$(shell cat ...)`으로 읽는다 — 여기와 Makefile에 버전 리터럴을 각각 박아두면
/// 한쪽만 올라갔을 때 조용히 어긋난다(CLAUDE.md "같은 사실 두 곳" 금지).
const KAGENT_VERSION_RAW: &str = include_str!("../../../scripts/helm/kagent-version.txt");

fn kagent_version() -> &'static str {
    KAGENT_VERSION_RAW.trim()
}

#[derive(Debug, Serialize)]
pub struct KagentDiagnosticReport {
    pub target_context: String,
    pub kagent_ready: bool,
    /// kagent 네임스페이스에 파드가 하나라도 있는지 — Ready 여부와 무관한 "설치됐는가" 신호.
    /// 설치 버튼의 프롬프트 표시 조건(`!kagent_installed`)이 이 필드를 쓴다.
    pub kagent_installed: bool,
    pub pod_issues_count: usize,
    pub recent_diagnosis: String,
    pub recommended_action: String,
    pub active_agents: Vec<String>,
    pub available_agents: Vec<String>,
}

/// 이 앱이 Agent CRD로 직접 설치/삭제할 수 있는 에이전트 목록(`toggle_kagent_agent`의
/// 매니페스트 분기와 1:1로 대응한다). k8s-agent·helm-agent는 Helm 차트가 관리하므로 제외.
const TOGGLEABLE_AGENTS: [&str; 3] = ["security-agent", "promql-agent", "observability-agent"];

/// 파드가 실제로 Ready인지 — phase만으로는 CrashLoopBackOff 파드도 Running으로 보인다.
fn pod_is_ready(pod: &serde_json::Value) -> bool {
    pod["status"]["conditions"]
        .as_array()
        .map(|conds| {
            conds.iter().any(|c| {
                c["type"].as_str() == Some("Ready") && c["status"].as_str() == Some("True")
            })
        })
        .unwrap_or(false)
}

async fn get_pods_json(context: &str, namespace: &str) -> Result<serde_json::Value, String> {
    let out = external_command("kubectl")?
        .args(["--context", context, "get", "pods", "-n", namespace, "-o", "json"])
        .output()
        .await
        .map_err(|e| format!("kubectl get pods -n {namespace} execution failed: {e}"))?;

    if !out.status.success() {
        return Err(format!(
            "kubectl get pods -n {namespace} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }

    serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("kubectl get pods -n {namespace} response parse failed: {e}"))
}

/// 진단 결과는 전부 kubectl 실측에서만 파생한다. 조회에 실패하면 에러로 올린다 —
/// "정상"으로 폴백하면 장애를 정상으로 위장하게 된다.
#[tauri::command]
pub async fn get_kagent_diagnostics(
    context: Option<String>,
) -> Result<KagentDiagnosticReport, String> {
    let target_ctx = context.unwrap_or_else(|| "colima".into());

    // kagent 네임스페이스 — 파드가 하나도 없으면 미설치, 있으면 전부 Ready일 때만 ready.
    let kagent_pods = get_pods_json(&target_ctx, "kagent").await?;
    let kagent_items = kagent_pods["items"].as_array().cloned().unwrap_or_default();

    let mut active_agents: Vec<String> = Vec::new();
    for pod in &kagent_items {
        if !pod_is_ready(pod) {
            continue;
        }
        // Helm 차트/Agent CRD 모두 app.kubernetes.io/name 라벨을 붙인다. 라벨이 없으면
        // 파드 이름을 그대로 쓴다(추측해서 채워 넣지 않는다).
        let name = pod["metadata"]["labels"]["app.kubernetes.io/name"]
            .as_str()
            .or_else(|| pod["metadata"]["name"].as_str())
            .unwrap_or("unknown")
            .to_string();
        if !active_agents.contains(&name) {
            active_agents.push(name);
        }
    }

    let kagent_installed = !kagent_items.is_empty();
    let kagent_ready = kagent_installed && kagent_items.iter().all(pod_is_ready);

    // default 네임스페이스 파드 장애 탐지 — 원인 문자열은 클러스터가 준 reason/message만 쓴다.
    let default_pods = get_pods_json(&target_ctx, "default").await?;
    let default_items = default_pods["items"].as_array().cloned().unwrap_or_default();

    let mut pod_issues_count = 0usize;
    let mut issue_details: Vec<String> = Vec::new();

    for pod in &default_items {
        let pod_name = pod["metadata"]["name"].as_str().unwrap_or("unknown");
        let phase = pod["status"]["phase"].as_str().unwrap_or("");
        let mut has_issue = phase != "Running" && phase != "Succeeded";

        if let Some(cs_list) = pod["status"]["containerStatuses"].as_array() {
            for cs in cs_list {
                if let Some(waiting) = cs["state"]["waiting"].as_object() {
                    let reason = waiting.get("reason").and_then(|r| r.as_str()).unwrap_or("");
                    if reason.is_empty() || reason == "ContainerCreating" || reason == "PodInitializing" {
                        continue;
                    }
                    has_issue = true;
                    let message = waiting
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("(no message)");
                    issue_details.push(format!("Pod [{pod_name}] {reason}: {message}"));
                }
            }
        }

        if has_issue {
            pod_issues_count += 1;
            if issue_details.is_empty() || !issue_details.iter().any(|d| d.contains(pod_name)) {
                issue_details.push(format!("Pod [{pod_name}] phase={phase}"));
            }
        }
    }

    let recent_diagnosis = if pod_issues_count == 0 {
        format!("kubectl measured: context [{target_ctx}] default namespace — no issues among {} pod(s).", default_items.len())
    } else {
        format!(
            "kubectl measured: {pod_issues_count} pod(s) with issues — {}",
            issue_details.join(" / ")
        )
    };

    // 권고는 "다음에 무엇을 실행하면 원인을 볼 수 있는가"까지만 제시한다.
    // 실제 복구안 생성은 kagent 에이전트의 몫이며, 여기서 결론을 지어내지 않는다.
    let recommended_action = if !kagent_ready {
        format!("kagent namespace is not Ready ({} pod(s)). Run `make kagent-up`, then check with `kubectl --context {target_ctx} get pods -n kagent`.", kagent_items.len())
    } else if pod_issues_count > 0 {
        format!("Check events via kagent UI or `kubectl --context {target_ctx} describe pod <name>`, and request a diagnosis from k8s-agent.")
    } else {
        format!("No further action needed. {} active agent(s) Ready.", active_agents.len())
    };

    Ok(KagentDiagnosticReport {
        target_context: target_ctx,
        kagent_ready,
        kagent_installed,
        pod_issues_count,
        recent_diagnosis,
        recommended_action,
        active_agents,
        available_agents: TOGGLEABLE_AGENTS.iter().map(|s| s.to_string()).collect(),
    })
}

#[tauri::command]
pub async fn toggle_kagent_agent(
    agent_name: String,
    enable: bool,
    context: Option<String>,
) -> Result<String, String> {
    let target_ctx = context.unwrap_or_else(|| "colima".into());

    if enable {
        let manifest = match agent_name.as_str() {
            "security-agent" => r#"apiVersion: kagent.dev/v1alpha2
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
            "promql-agent" => r#"apiVersion: kagent.dev/v1alpha2
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
            "observability-agent" => r#"apiVersion: kagent.dev/v1alpha2
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
            other => {
                return Err(format!(
                    "Agent [{other}] cannot be installed by this app. Installable: {}",
                    TOGGLEABLE_AGENTS.join(", ")
                ));
            }
        };

        let file_path = std::env::temp_dir().join(format!("kubemetal-{agent_name}.yaml"));
        std::fs::write(&file_path, manifest)
            .map_err(|e| format!("Failed to create manifest temp file: {e}"))?;

        let output = external_command("kubectl")?
            .args(["--context", &target_ctx, "apply", "-f"])
            .arg(&file_path)
            .output()
            .await
            .map_err(|e| format!("kubectl apply execution failed: {e}"))?;

        let _ = std::fs::remove_file(&file_path);

        if !output.status.success() {
            return Err(format!(
                "Agent CRD [{agent_name}] apply failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        // kubectl이 돌려준 문장만 전달한다. 파드 기동 여부는 여기서 알 수 없으므로
        // "1/1 Running" 같은 상태를 주장하지 않는다 — 진단 재조회가 실제 상태를 채운다.
        Ok(format!(
            "Agent CRD [{agent_name}] applied: {}. Check pod startup status via diagnostics re-query.",
            String::from_utf8_lossy(&output.stdout).trim()
        ))
    } else {
        let output = external_command("kubectl")?
            .args([
                "--context", &target_ctx, "delete", "agent.kagent.dev", &agent_name, "-n", "kagent",
            ])
            .output()
            .await
            .map_err(|e| format!("kubectl delete execution failed: {e}"))?;

        if !output.status.success() {
            return Err(format!(
                "Agent CRD [{agent_name}] delete failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        Ok(format!(
            "Agent CRD [{agent_name}] deleted: {}",
            String::from_utf8_lossy(&output.stdout).trim()
        ))
    }
}

/// 저장된 배포 대상(D26)의 컨텍스트로 kagent를 Helm 설치한다. Agent CRD와 달리 클러스터
/// 쪽에 Mac 의존을 만들지 않으므로(에이전트 정의만 생김, 모델 연계는 별도 IPC — D32) L1/L2
/// 게이트를 걸지 않는다(D30 호환).
///
/// 네임스페이스는 `helm --create-namespace`가 아니라 `kubectl create ns --dry-run=client
/// -o yaml | apply`로 먼저 만든다 — 리포의 `Makefile kagent-up`이 실제로 쓰는 방식이 이것이고,
/// `--create-namespace` 플래그의 동작을 실기기로 확인한 근거가 없어 그 위에 새로 얹지 않는다.
#[tauri::command]
pub async fn install_kagent(app: tauri::AppHandle) -> Result<String, String> {
    let target = get_deploy_target(app.clone()).await?;

    ensure_namespace(&target.context, "kagent").await?;

    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let values_path = resolve_bundled_resource(&resource_dir, "scripts/helm/kagent-values.yaml");
    if !values_path.is_file() {
        return Err(format!(
            "kagent values file not found: {}",
            values_path.display()
        ));
    }

    let output = external_command("helm")?
        .args([
            "upgrade",
            "--install",
            "kagent",
            "oci://ghcr.io/kagent-dev/kagent/helm/kagent",
            "--version",
            kagent_version(),
            "-n",
            "kagent",
            "-f",
        ])
        .arg(&values_path)
        .args(["--kube-context", &target.context, "--reuse-values"])
        .output()
        .await
        .map_err(|e| format!("helm upgrade --install kagent execution failed: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "helm upgrade --install kagent failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    // helm이 실제로 출력한 내용만 인용한다 — 파드가 떴는지는 여기서 알 수 없으므로
    // "설치 완료"를 단정하지 않는다(D22). 실제 상태는 get_kagent_diagnostics 재조회로 확인.
    Ok(format!(
        "helm upgrade --install kagent [{}] returned: {}",
        target.context,
        String::from_utf8_lossy(&output.stdout).trim()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kagent_version_is_trimmed_and_non_empty() {
        let version = kagent_version();
        assert!(!version.is_empty(), "kagent-version.txt is empty");
        assert_eq!(
            version,
            version.trim(),
            "kagent_version() must strip surrounding whitespace/newline"
        );
    }
}
