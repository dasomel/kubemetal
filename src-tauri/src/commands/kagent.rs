//! kagent 진단/설치/에이전트 토글 IPC. `colima.rs`에서 이관됨(D33 작업) — colima 자체의
//! 수명주기(start/stop/status)와는 무관하고, kagent 배포 대상은 활성 `DeployTarget`(D26)이다.

use serde::Serialize;
use tauri::{Manager, State};

use crate::commands::deploy_target::{get_deploy_target, kubectl_json};
use crate::commands::mlx::MlxState;
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
    kubectl_json(context, &["get", "pods", "-n", namespace, "-o", "json"]).await
}

/// 진단 결과는 전부 kubectl 실측에서만 파생한다. 조회에 실패하면 에러로 올린다 —
/// "정상"으로 폴백하면 장애를 정상으로 위장하게 된다.
#[tauri::command]
pub async fn get_kagent_diagnostics(
    context: Option<String>,
) -> Result<KagentDiagnosticReport, String> {
    let target_ctx = context.unwrap_or_else(|| "colima".into());

    // 두 네임스페이스 조회는 서로 무관하므로 동시에 실행한다.
    let (kagent_pods, default_pods) = tokio::join!(
        get_pods_json(&target_ctx, "kagent"),
        get_pods_json(&target_ctx, "default"),
    );

    // kagent 네임스페이스 — 파드가 하나도 없으면 미설치, 있으면 전부 Ready일 때만 ready.
    let kagent_pods = kagent_pods?;
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
    let default_pods = default_pods?;
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

        let result = external_command("kubectl")?
            .args(["--context", &target_ctx, "apply", "-f"])
            .arg(&file_path)
            .output()
            .await
            .map_err(|e| format!("kubectl apply execution failed: {e}"));

        let _ = std::fs::remove_file(&file_path);
        let output = result?;

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

/// kagent OCI 차트 저장소. 릴리스명과 차트명이 같아(`kagent`, `kagent-crds`) 인자 하나로 쓴다.
const KAGENT_CHART_BASE: &str = "oci://ghcr.io/kagent-dev/kagent/helm";

/// kagent 차트(`kagent-crds`/`kagent`) helm upgrade 공통 경로 — 버전·네임스페이스·컨텍스트
/// 조립이 같으므로 한 곳에서만 만든다. `install`은 신규 설치(`--install`)와 기존 릴리스
/// 갱신(모델 연계)을 가른다.
async fn helm_upgrade_kagent(
    context: &str,
    chart: &str,
    values_path: Option<&std::path::Path>,
    install: bool,
) -> Result<String, String> {
    let chart_ref = format!("{KAGENT_CHART_BASE}/{chart}");

    let mut cmd = external_command("helm")?;
    cmd.arg("upgrade");
    if install {
        cmd.arg("--install");
    }
    cmd.args([chart, &chart_ref])
        .args(["--version", kagent_version(), "-n", "kagent"]);
    if let Some(path) = values_path {
        cmd.arg("-f").arg(path);
    }
    cmd.args(["--kube-context", context, "--reuse-values"]);

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("helm upgrade {chart} execution failed: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "helm upgrade {chart} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// 지정한 컨텍스트에 kagent를 Helm 설치한다. Agent CRD와 달리 클러스터 쪽에 Mac 의존을
/// 만들지 않으므로(에이전트 정의만 생김, 모델 연계는 별도 IPC — D32) L1/L2 게이트를 걸지
/// 않는다(D30 호환).
///
/// `context`는 `get_kagent_diagnostics`/`toggle_kagent_agent`와 같은 축 — KagentOpsView의
/// kubeconfig 선택기 값이다(D33 개정). 같은 패널의 진단은 드롭다운을, 설치만 저장된
/// DeployTarget을 보던 탓에 "narwhal을 보며 설치했는데 colima에 설치되는" 불일치가 있었다.
/// 인자가 없으면 저장된 배포 대상(D26)으로 폴백해 기존 호출자 동작을 유지한다.
///
/// 네임스페이스는 `helm --create-namespace`가 아니라 `kubectl create ns --dry-run=client
/// -o yaml | apply`로 먼저 만든다 — 리포의 `Makefile kagent-up`이 실제로 쓰는 방식이 이것이고,
/// `--create-namespace` 플래그의 동작을 실기기로 확인한 근거가 없어 그 위에 새로 얹지 않는다.
#[tauri::command]
pub async fn install_kagent(
    app: tauri::AppHandle,
    context: Option<String>,
) -> Result<String, String> {
    let target_ctx = match context {
        Some(ctx) => ctx,
        None => get_deploy_target(app.clone()).await?.context,
    };

    ensure_namespace(&target_ctx, "kagent").await?;

    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let values_path = resolve_bundled_resource(&resource_dir, "scripts/helm/kagent-values.yaml");
    if !values_path.is_file() {
        return Err(format!(
            "kagent values file not found: {}",
            values_path.display()
        ));
    }

    // CRD 차트가 먼저다(D33 개정 2). 본 차트의 템플릿에는 Agent/ModelConfig/RemoteMCPServer
    // 같은 kagent.dev 리소스가 들어 있어, CRD가 없는 클러스터에서는 helm이 "no matches for
    // kind ... ensure CRDs are installed first"로 렌더 단계에서 실패한다. colima는 2026-07-23
    // 수동 설치분 CRD가 남아 있어 이 누락이 가려져 있었다(narwhal 실측으로 드러남).
    helm_upgrade_kagent(&target_ctx, "kagent-crds", None, true).await?;

    let stdout = helm_upgrade_kagent(&target_ctx, "kagent", Some(&values_path), true).await?;

    // helm이 실제로 출력한 내용만 인용한다 — 파드가 떴는지는 여기서 알 수 없으므로
    // "설치 완료"를 단정하지 않는다(D22). 실제 상태는 get_kagent_diagnostics 재조회로 확인.
    Ok(format!(
        "helm upgrade --install kagent [{target_ctx}] returned: {stdout}"
    ))
}

/// D32(c): Secret/ModelConfig 둘 다 chart가 helm values의 `providers.openAI`에서 만들어낸다
/// (Secret 이름 `kagent-openai`는 우리가 짓지 않는다) — 여기서는 그 결과를 읽기만 한다.
const MODEL_CONFIG_NAME: &str = "default-model-config";

#[derive(Debug, Serialize)]
pub struct KagentServingSummary {
    pub port: u16,
    /// serving.model_path 전체 경로 — basename이 아니다. 서버가 `/v1/models`에 보고하는
    /// 정확한 id와 맞춰야 한다(mistakes-log 2026-07-24: `model: 'mlx'`로 HF repo id로
    /// 오인돼 phone-home한 사고).
    pub model_id: String,
}

#[derive(Debug, Serialize)]
pub struct KagentModelConfigSummary {
    pub base_url: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct KagentModelStatus {
    pub target_context: String,
    pub target_namespace: String,
    pub gate_ok: bool,
    /// 영문 기술 사유(D31 계약의 우발적 오류 상세 계열) — `full_stack_gate()`가 이미 영문.
    pub gate_reason: Option<String>,
    pub serving: Option<KagentServingSummary>,
    pub model_config: Option<KagentModelConfigSummary>,
    /// 프런트가 `kagent.modelStatus.stale.${code}`로 매핑하는 안정 코드(D31). 모두 일치하면
    /// None. 알려진 코드: `not_configured`/`port_mismatch`/`model_mismatch`/
    /// `bridge_port_not_proxied`/`bridge_state_unknown`(브리지 조회 자체가 실패 — 필드는
    /// 일치하지만 실제 유효성은 확인 못함. `not_proxied`와 달리 중립 톤으로 안내).
    pub stale_code: Option<String>,
}

fn expected_base_url(namespace: &str, port: u16) -> String {
    format!("http://mac-gpu-service.{namespace}.svc.cluster.local:{port}/v1")
}

/// ModelConfig의 baseUrl/model이 현재 서빙과 어긋난 지점을 구분한다 — 셋 다 안내 문구가
/// 다르므로(D32 e) 한 뭉치의 "안 맞음"으로 뭉개지 않는다.
fn classify_model_stale(
    base_url: &str,
    model: &str,
    namespace: &str,
    serving_port: u16,
    serving_model: &str,
) -> Option<&'static str> {
    let prefix = format!("http://mac-gpu-service.{namespace}.svc.cluster.local:");
    let Some(after_prefix) = base_url.strip_prefix(&prefix) else {
        // chart 기본값(gpt-* 등 우리 브리지가 아닌 baseUrl)도 여기로 떨어진다.
        return Some("not_configured");
    };
    let observed_port = after_prefix
        .split('/')
        .next()
        .and_then(|p| p.parse::<u16>().ok());
    if observed_port != Some(serving_port) {
        return Some("port_mismatch");
    }
    if model != serving_model {
        return Some("model_mismatch");
    }
    None
}

/// `kubectl get modelconfig` — NotFound는 "미구성" 정보이지 오류가 아니므로 None으로
/// 흡수한다. 그 외 실패(권한/네트워크 등)는 그대로 올린다(D22).
async fn get_model_config_json(
    context: &str,
    namespace: &str,
) -> Result<Option<serde_json::Value>, String> {
    match kubectl_json(
        context,
        &["-n", namespace, "get", "modelconfig", MODEL_CONFIG_NAME, "-o", "json"],
    )
    .await
    {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.contains("NotFound") => Ok(None),
        Err(e) => Err(e),
    }
}

/// `mac-gpu-service`의 실제 배포 형태를 읽어 서빙 포트가 프록시되는지 확인한 결과.
/// `NotApplicable`(ExternalName — 포트 미선언이 설계상 정상)과 `Unknown`(조회 자체가
/// 실패해 판정할 수 없음)을 구분한다 — 둘 다 "문제 없음"으로 뭉개면 조회 실패가
/// "정상"으로 위장된다(D22, 검수 반영).
enum BridgePortCheck {
    NotApplicable,
    Proxied,
    NotProxied,
    Unknown,
}

/// best-effort 조회 — kubectl 실패/파싱 실패는 `Unknown`으로 흡수해 전체 상태 조회
/// 자체는 막지 않지만, 그 실패를 `stale_code=None`(정상)으로 위장하지도 않는다.
async fn check_bridge_port(context: &str, namespace: &str, serving_port: u16) -> BridgePortCheck {
    let Ok(svc) = kubectl_json(context, &["-n", namespace, "get", "svc", "mac-gpu-service", "-o", "json"]).await
    else {
        return BridgePortCheck::Unknown;
    };
    if svc["spec"]["type"].as_str() == Some("ExternalName") {
        return BridgePortCheck::NotApplicable;
    }
    let Some(ports) = svc["spec"]["ports"].as_array() else {
        return BridgePortCheck::Unknown;
    };
    if ports.iter().any(|p| p["port"].as_u64() == Some(serving_port as u64)) {
        BridgePortCheck::Proxied
    } else {
        BridgePortCheck::NotProxied
    }
}

/// 저장된 배포 대상(D26)의 kagent 모델 연계 상태를 실측한다. KagentOpsView의 로컬
/// kubeconfig 선택기와는 무관하다(D32 a) — 대상은 항상 저장된 DeployTarget.
#[tauri::command]
pub async fn get_kagent_model_status(
    app: tauri::AppHandle,
    mlx_state: State<'_, MlxState>,
) -> Result<KagentModelStatus, String> {
    let target = get_deploy_target(app).await?;

    let gate_result = target.full_stack_gate();
    let gate_ok = gate_result.is_ok();
    let gate_reason = gate_result.err();

    let serving = mlx_state
        .serving
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .map(|s| KagentServingSummary {
            port: s.port,
            model_id: s.model_path,
        });

    let model_config_json = get_model_config_json(&target.context, "kagent").await?;
    let model_config = model_config_json.as_ref().map(|v| KagentModelConfigSummary {
        base_url: v["spec"]["openAI"]["baseUrl"].as_str().map(str::to_string),
        model: v["spec"]["model"].as_str().map(str::to_string),
    });

    let stale_code = match (&serving, &model_config) {
        (Some(srv), Some(mc)) => {
            let base_url = mc.base_url.clone().unwrap_or_default();
            let model = mc.model.clone().unwrap_or_default();
            match classify_model_stale(&base_url, &model, &target.namespace, srv.port, &srv.model_id) {
                Some(code) => Some(code.to_string()),
                None => match check_bridge_port(&target.context, &target.namespace, srv.port).await {
                    BridgePortCheck::NotProxied => Some("bridge_port_not_proxied".to_string()),
                    BridgePortCheck::Unknown => Some("bridge_state_unknown".to_string()),
                    BridgePortCheck::Proxied | BridgePortCheck::NotApplicable => None,
                },
            }
        }
        // 서빙이 없으면 비교 기준이 없다 — 프런트는 serving=None을 별도 안내로 다룬다.
        (None, _) => None,
        (Some(_), None) => Some("not_configured".to_string()),
    };

    Ok(KagentModelStatus {
        target_context: target.context,
        target_namespace: target.namespace,
        gate_ok,
        gate_reason,
        serving,
        model_config,
        stale_code,
    })
}

/// YAML 이중따옴표 문자열 안에 안전하게 넣기 위한 최소 이스케이프. `model_id`는
/// 로컬 파일시스템 절대경로라 특수문자가 드물지만, 값을 그대로 리터럴에 꽂지 않는다.
fn yaml_dquote(raw: &str) -> String {
    format!("\"{}\"", raw.replace('\\', "\\\\").replace('"', "\\\""))
}

/// 현재 서빙 중인 모델로 kagent LLM 백엔드를 연결한다. ModelConfig는 helm 소유이므로
/// (D32 보정 1) `kubectl apply`가 아니라 `install_kagent`와 같은 helm upgrade 경로를 쓴다.
/// `--set`으로 baseUrl/model을 넘기면 이스케이프 지뢰가 있어(URL의 `:`, 경로의 `/`) 임시
/// values 파일(-f)로 넘긴다.
#[tauri::command]
pub async fn configure_kagent_model(
    app: tauri::AppHandle,
    mlx_state: State<'_, MlxState>,
) -> Result<String, String> {
    let target = get_deploy_target(app).await?;
    target.full_stack_gate()?;

    let serving = mlx_state
        .serving
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or("MLX serving is not running — start serving first")?;

    // D32(c): 모델 id는 serving.model_path 전체 경로 그대로 — basename 금지.
    let model_id = serving.model_path;
    let base_url = expected_base_url(&target.namespace, serving.port);

    let values = format!(
        "providers:\n  default: openAI\n  openAI:\n    apiKey: dummy-local-key-not-used\n    apiKeySecretKey: OPENAI_API_KEY\n    apiKeySecretRef: kagent-openai\n    config:\n      baseUrl: {}\n    model: {}\n    provider: OpenAI\n",
        yaml_dquote(&base_url),
        yaml_dquote(&model_id),
    );

    let values_path = std::env::temp_dir().join("kubemetal-kagent-model-values.yaml");
    std::fs::write(&values_path, &values)
        .map_err(|e| format!("Failed to write kagent model values temp file: {e}"))?;

    let result = helm_upgrade_kagent(&target.context, "kagent", Some(&values_path), false).await;
    let _ = std::fs::remove_file(&values_path);
    let stdout = result?;

    // helm이 출력한 내용만 인용한다 — 도달성은 여기서 검증되지 않는다(D22). "연결됨"을
    // 단정하지 않고, 실제 반영 여부는 get_kagent_model_status 재조회로 확인한다.
    Ok(format!(
        "helm upgrade kagent (model config) [{}] returned: {}. Reachability not verified — re-check status.",
        target.context, stdout
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

    #[test]
    fn expected_base_url_matches_d10_bridge_pattern() {
        assert_eq!(
            expected_base_url("kagent", 8081),
            "http://mac-gpu-service.kagent.svc.cluster.local:8081/v1"
        );
    }

    #[test]
    fn classify_model_stale_flags_non_bridge_baseurl_as_not_configured() {
        // 차트 기본값(gpt-4 등) — 우리 브리지 패턴이 아니다.
        let code = classify_model_stale("https://api.openai.com/v1", "gpt-4", "default", 8081, "/models/foo");
        assert_eq!(code, Some("not_configured"));
    }

    #[test]
    fn classify_model_stale_flags_port_mismatch() {
        let code = classify_model_stale(
            "http://mac-gpu-service.default.svc.cluster.local:8080/v1",
            "/models/foo",
            "default",
            8081,
            "/models/foo",
        );
        assert_eq!(code, Some("port_mismatch"));
    }

    #[test]
    fn classify_model_stale_flags_model_mismatch() {
        let code = classify_model_stale(
            "http://mac-gpu-service.default.svc.cluster.local:8081/v1",
            "/models/old",
            "default",
            8081,
            "/models/new",
        );
        assert_eq!(code, Some("model_mismatch"));
    }

    #[test]
    fn classify_model_stale_none_when_fully_matched() {
        let code = classify_model_stale(
            "http://mac-gpu-service.default.svc.cluster.local:8081/v1",
            "/models/foo",
            "default",
            8081,
            "/models/foo",
        );
        assert_eq!(code, None);
    }

    #[test]
    fn yaml_dquote_escapes_backslash_and_quote() {
        assert_eq!(yaml_dquote(r#"C:\path\"weird""#), r#""C:\\path\\\"weird\"""#);
    }
}
