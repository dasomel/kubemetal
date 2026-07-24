use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::services::process::{external_command, resolve_bundled_resource};

/// colima 0.10.x `status --json` 실측 스키마: 기동 중일 때만 exit 0 + stdout에
/// 평면 JSON({"kubernetes":true,...})을 출력하고, 미기동이면 exit 1 + stdout 없음.
/// "status" 필드는 존재하지 않는다 — 기동 여부는 파싱 성공 자체로 판별한다.
#[derive(Debug, Deserialize)]
struct ColimaStatusRaw {
    #[serde(default)]
    kubernetes: bool,
}

#[derive(Debug, Serialize)]
pub struct ClusterStatus {
    pub is_running: bool,
    pub kubernetes_active: bool,
    pub mlflow_ready: bool,
    pub seaweedfs_ready: bool,
    pub artifact_store_wired: bool,
}

#[tauri::command]
pub async fn get_cluster_status() -> Result<ClusterStatus, String> {
    let output = external_command("colima")?
        .args(["status", "--json"])
        .output()
        .await
        .map_err(|e| format!("colima 실행 실패: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let raw: Option<ColimaStatusRaw> = if output.status.success() {
        serde_json::from_str(&stdout).ok()
    } else {
        None
    };

    let Some(raw) = raw else {
        return Ok(ClusterStatus {
            is_running: false,
            kubernetes_active: false,
            mlflow_ready: false,
            seaweedfs_ready: false,
            artifact_store_wired: false,
        });
    };

    let is_running = true; // exit 0 + JSON 출력 = 기동 중 (미기동은 위에서 조기 반환)
    let kubernetes_active = raw.kubernetes;

    let (mlflow_ready, seaweedfs_ready, artifact_store_wired) = if kubernetes_active {
        let deploy_out = external_command("kubectl")?
            .args(["--context", "colima", "get", "deploy", "-n", "default", "-o", "json"])
            .output()
            .await
            .map_err(|e| format!("kubectl get deploy 실패: {e}"))?;
        let json: serde_json::Value =
            serde_json::from_slice(&deploy_out.stdout).unwrap_or(serde_json::json!({"items": []}));
        let items = json["items"].as_array().cloned().unwrap_or_default();
        let is_ready = |name: &str| {
            items.iter().any(|d| {
                d["metadata"]["name"].as_str() == Some(name)
                    && d["status"]["availableReplicas"].as_u64().unwrap_or(0) > 0
            })
        };
        // mlflow Deployment 컨테이너 env에 MLFLOW_S3_ENDPOINT_URL이 있으면
        // SeaweedFS(S3) 아티팩트 스토어가 연동된 것으로 판정 — 추가 kubectl 호출 없이
        // 위에서 이미 가져온 deploy JSON을 재사용한다.
        let artifact_store_wired = items.iter().any(|d| {
            d["metadata"]["name"].as_str() == Some("mlflow")
                && d["spec"]["template"]["spec"]["containers"]
                    .as_array()
                    .map(|containers| {
                        containers.iter().any(|c| {
                            c["env"]
                                .as_array()
                                .map(|envs| {
                                    envs.iter().any(|e| {
                                        e["name"].as_str() == Some("MLFLOW_S3_ENDPOINT_URL")
                                    })
                                })
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
        });
        (is_ready("mlflow"), is_ready("seaweedfs"), artifact_store_wired)
    } else {
        (false, false, false)
    };

    Ok(ClusterStatus {
        is_running,
        kubernetes_active,
        mlflow_ready,
        seaweedfs_ready,
        artifact_store_wired,
    })
}

#[tauri::command]
pub async fn start_cluster(cpu: u32, memory: u32) -> Result<String, String> {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_memory();
    sys.refresh_cpu_usage();
    let host_ram_gb = sys.total_memory() / 1024 / 1024 / 1024;
    let host_cores = sys.cpus().len().max(1) as u32;
    let max_memory_gb: u64 = match host_ram_gb {
        0..=23 => 4,
        24..=55 => 8,
        _ => 12,
    };
    let memory = memory.min(max_memory_gb as u32).max(1);
    let cpu = cpu.clamp(1, host_cores);

    let output = external_command("colima")?
        .args([
            "start",
            "--cpu", &cpu.to_string(),
            "--memory", &memory.to_string(),
            "--vm-type=vz",
            "--mount-type=virtiofs",
            "--kubernetes",
        ])
        .output()
        .await
        .map_err(|e| format!("colima start 실행 실패: {e}"))?;

    if output.status.success() {
        Ok("Colima K8s 클러스터가 시작되었습니다.".into())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

#[tauri::command]
pub async fn stop_cluster() -> Result<String, String> {
    let output = external_command("colima")?
        .arg("stop")
        .output()
        .await
        .map_err(|e| format!("colima stop 실행 실패: {e}"))?;

    if output.status.success() {
        Ok("Colima 클러스터가 정지되었습니다.".into())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

#[derive(Debug, Serialize)]
pub struct KagentDiagnosticReport {
    pub target_context: String,
    pub kagent_ready: bool,
    pub pod_issues_count: usize,
    pub recent_diagnosis: String,
    pub recommended_action: String,
    pub active_agents: Vec<String>,
    pub available_agents: Vec<String>,
}

/// kubeconfig에 실제로 등록된 컨텍스트만 반환한다. 조회 실패는 에러로 올린다 —
/// 존재하지 않는 컨텍스트를 폴백으로 지어내면 UI가 없는 클러스터를 있는 것처럼 표시한다.
#[tauri::command]
pub async fn list_kubeconfig_contexts() -> Result<Vec<String>, String> {
    let output = external_command("kubectl")?
        .args(["config", "get-contexts", "-o", "name"])
        .output()
        .await
        .map_err(|e| format!("kubectl config get-contexts 실패: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "kubectl config get-contexts 실패: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
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
        .map_err(|e| format!("kubectl get pods -n {namespace} 실행 실패: {e}"))?;

    if !out.status.success() {
        return Err(format!(
            "kubectl get pods -n {namespace} 실패: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }

    serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("kubectl get pods -n {namespace} 응답 파싱 실패: {e}"))
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

    let kagent_ready = !kagent_items.is_empty() && kagent_items.iter().all(pod_is_ready);

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
                        .unwrap_or("(메시지 없음)");
                    issue_details.push(format!("파드 [{pod_name}] {reason}: {message}"));
                }
            }
        }

        if has_issue {
            pod_issues_count += 1;
            if issue_details.is_empty() || !issue_details.iter().any(|d| d.contains(pod_name)) {
                issue_details.push(format!("파드 [{pod_name}] phase={phase}"));
            }
        }
    }

    let recent_diagnosis = if pod_issues_count == 0 {
        format!("kubectl 실측: 컨텍스트 [{target_ctx}] default 네임스페이스 파드 {}개 중 이상 없음.", default_items.len())
    } else {
        format!(
            "kubectl 실측: {pod_issues_count}개 파드 이상 — {}",
            issue_details.join(" / ")
        )
    };

    // 권고는 "다음에 무엇을 실행하면 원인을 볼 수 있는가"까지만 제시한다.
    // 실제 복구안 생성은 kagent 에이전트의 몫이며, 여기서 결론을 지어내지 않는다.
    let recommended_action = if !kagent_ready {
        format!("kagent 네임스페이스가 Ready 상태가 아닙니다({}개 파드). `make kagent-up` 후 `kubectl --context {target_ctx} get pods -n kagent`로 확인하세요.", kagent_items.len())
    } else if pod_issues_count > 0 {
        format!("kagent UI 또는 `kubectl --context {target_ctx} describe pod <이름>`으로 이벤트를 확인하고, k8s-agent에 진단을 요청하세요.")
    } else {
        format!("추가 조치 불필요. 활성 에이전트 {}개 Ready.", active_agents.len())
    };

    Ok(KagentDiagnosticReport {
        target_context: target_ctx,
        kagent_ready,
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
                    "에이전트 [{other}]는 이 앱에서 설치할 수 없습니다. 설치 가능: {}",
                    TOGGLEABLE_AGENTS.join(", ")
                ));
            }
        };

        let file_path = std::env::temp_dir().join(format!("kubemetal-{agent_name}.yaml"));
        std::fs::write(&file_path, manifest)
            .map_err(|e| format!("매니페스트 임시 파일 생성 실패: {e}"))?;

        let output = external_command("kubectl")?
            .args(["--context", &target_ctx, "apply", "-f"])
            .arg(&file_path)
            .output()
            .await
            .map_err(|e| format!("kubectl apply 실행 실패: {e}"))?;

        let _ = std::fs::remove_file(&file_path);

        if !output.status.success() {
            return Err(format!(
                "Agent CRD [{agent_name}] 적용 실패: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        // kubectl이 돌려준 문장만 전달한다. 파드 기동 여부는 여기서 알 수 없으므로
        // "1/1 Running" 같은 상태를 주장하지 않는다 — 진단 재조회가 실제 상태를 채운다.
        Ok(format!(
            "Agent CRD [{agent_name}] 적용됨: {}. 파드 기동 상태는 진단 재조회로 확인하세요.",
            String::from_utf8_lossy(&output.stdout).trim()
        ))
    } else {
        let output = external_command("kubectl")?
            .args([
                "--context", &target_ctx, "delete", "agent.kagent.dev", &agent_name, "-n", "kagent",
            ])
            .output()
            .await
            .map_err(|e| format!("kubectl delete 실행 실패: {e}"))?;

        if !output.status.success() {
            return Err(format!(
                "Agent CRD [{agent_name}] 삭제 실패: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        Ok(format!(
            "Agent CRD [{agent_name}] 삭제됨: {}",
            String::from_utf8_lossy(&output.stdout).trim()
        ))
    }
}

#[derive(Debug, Serialize)]
pub struct AirgapAssetItem {
    pub category: String,
    pub name: String,
    pub version: String,
    pub file_name: String,
    pub exists: bool,
    pub size_mb: f64,
    /// 손상 파일은 MB로 반올림하면 실제 크기가 0으로 뭉개진다 — 정확한 바이트를 함께 준다.
    pub size_bytes: u64,
    /// 파일은 있는데 유효한 산출물로 보기엔 너무 작은 경우. 실기기에서 발견(2026-07-25):
    /// 구버전 다운로더의 `curl ... || true`가 남긴 9바이트 `Not Found` 본문이
    /// `binaries/kubescape`로 저장돼 UI가 "보유 (0 MB)"로 보고하고 있었다.
    pub corrupt: bool,
}

/// 이보다 작은 파일은 바이너리·차트(.tgz)·이미지 아카이브(.tar.gz) 어느 쪽으로도 유효할 수
/// 없다. 자산별 실제 크기를 추정하는 대신, 어떤 산출물에도 적용되는 하한만 둔다.
const MIN_VALID_ASSET_BYTES: u64 = 1024;

#[derive(Debug, Serialize)]
pub struct AirgapStatusReport {
    pub airgap_dir: String,
    pub total_assets_count: usize,
    pub downloaded_count: usize,
    pub total_size_mb: f64,
    pub assets: Vec<AirgapAssetItem>,
}

#[tauri::command]
pub async fn get_airgap_status() -> Result<AirgapStatusReport, String> {
    let home_str = std::env::var("HOME").map_err(|_| "HOME 환경변수를 찾을 수 없습니다.".to_string())?;
    let airgap_dir = std::path::PathBuf::from(home_str).join(".kubemetal").join("airgap");

    let targets = vec![
        ("Binary", "K3s Kubernetes Engine", "v1.28.2", "binaries/k3s"),
        ("Binary", "Kubescape Security CLI", "v3.0.0", "binaries/kubescape"),
        ("Helm Chart", "kagent Helm Chart", "0.9.12", "charts/kagent-0.9.12.tgz"),
        ("Container Image", "kagent Controller Image", "0.9.12", "images/cr.kagent.dev_kagent-dev_kagent_controller_0.9.12.tar.gz"),
        ("Container Image", "kagent Declarative App Image", "0.9.12", "images/cr.kagent.dev_kagent-dev_kagent_app_0.9.12.tar.gz"),
        ("Container Image", "kagent UI Dashboard Image", "0.9.12", "images/cr.kagent.dev_kagent-dev_kagent_ui_0.9.12.tar.gz"),
        ("Container Image", "kagent Tools Server Image", "0.2.1", "images/ghcr.io_kagent-dev_kagent_tools_0.2.1.tar.gz"),
        ("Container Image", "kmcp Controller Image", "0.3.0", "images/ghcr.io_kagent-dev_kmcp_controller_0.3.0.tar.gz"),
        ("Container Image", "MLflow Server Image", "v2.10.0", "images/ghcr.io_mlflow_mlflow_v2.10.0.tar.gz"),
        ("Container Image", "SeaweedFS Storage Image", "3.60", "images/chrislusf_seaweedfs_3.60.tar.gz"),
        ("Container Image", "PostgreSQL Database Image", "16-alpine", "images/postgres_16-alpine.tar.gz"),
        ("Container Image", "Trivy Vulnerability Scanner", "latest", "images/aquasec_trivy_latest.tar.gz"),
        ("Container Image", "Nginx Test Image", "alpine", "images/nginx_alpine.tar.gz"),
    ];

    let mut assets = Vec::new();
    let mut downloaded_count = 0;
    let mut total_size_mb = 0.0;

    for (cat, name, ver, rel_path) in targets {
        let full_path = airgap_dir.join(rel_path);
        // .tar.gz 우선 확인, 없으면 .tar 가념 확인
        let target_file = if full_path.exists() {
            Some(full_path)
        } else {
            let fallback_path = airgap_dir.join(rel_path.trim_end_matches(".gz"));
            if fallback_path.exists() {
                Some(fallback_path)
            } else {
                None
            }
        };

        // 파일 존재만으로 "보유"로 세지 않는다 — 크기 하한을 못 넘기면 손상으로 분류하고
        // 보유 수·총 용량에서도 제외한다(부분 수신 파일을 완료로 보고하지 않기 위함).
        let (exists, corrupt, size_mb, size_bytes) = match target_file {
            Some(p) => {
                let bytes = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                let mb = (bytes as f64) / 1024.0 / 1024.0;
                if bytes >= MIN_VALID_ASSET_BYTES {
                    downloaded_count += 1;
                    total_size_mb += mb;
                    (true, false, (mb * 100.0).round() / 100.0, bytes)
                } else {
                    (false, true, (mb * 100.0).round() / 100.0, bytes)
                }
            }
            None => (false, false, 0.0, 0),
        };

        assets.push(AirgapAssetItem {
            category: cat.into(),
            name: name.into(),
            version: ver.into(),
            file_name: rel_path.into(),
            exists,
            size_mb,
            size_bytes,
            corrupt,
        });
    }

    let total_assets_count = assets.len();

    Ok(AirgapStatusReport {
        airgap_dir: airgap_dir.to_string_lossy().to_string(),
        total_assets_count,
        downloaded_count,
        total_size_mb: (total_size_mb * 100.0).round() / 100.0,
        assets,
    })
}

/// Air-Gap 스크립트는 `bundle.resources`로 동봉된다. 상대경로(`scripts/airgap/...`)는
/// `.app` 실행 시 CWD가 프로젝트 루트가 아니므로 항상 실패한다 — resource_dir 기준으로
/// 해석해야 dev/번들 양쪽에서 동작한다(D5·`resolve_bundled_resource`와 동일 규약).
async fn run_airgap_script(
    app: &tauri::AppHandle,
    relative: &str,
    label: &str,
) -> Result<String, String> {
    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let script_path = resolve_bundled_resource(&resource_dir, relative);
    if !script_path.is_file() {
        return Err(format!(
            "{label} 스크립트를 찾을 수 없습니다: {}",
            script_path.display()
        ));
    }

    let output = external_command("bash")?
        .arg(&script_path)
        .output()
        .await
        .map_err(|e| format!("{label} 스크립트 실행 실패: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "{label} 실패: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    // 스크립트가 실제로 출력한 마지막 줄을 그대로 돌려준다(성공 문구를 지어내지 않는다).
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("(출력 없음)")
        .trim()
        .to_string())
}

#[tauri::command]
pub async fn trigger_airgap_download(app: tauri::AppHandle) -> Result<String, String> {
    run_airgap_script(
        &app,
        "scripts/airgap/download_airgap_bundle.sh",
        "Air-Gap 번들 다운로드",
    )
    .await
}

#[tauri::command]
pub async fn trigger_airgap_install(app: tauri::AppHandle) -> Result<String, String> {
    run_airgap_script(
        &app,
        "scripts/airgap/install_from_airgap.sh",
        "Air-Gap 오프라인 설치",
    )
    .await
}

#[derive(Debug, Serialize)]
pub struct AirgapLatestVersionReport {
    pub name: String,
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
}

#[tauri::command]
pub async fn check_latest_airgap_versions() -> Result<Vec<AirgapLatestVersionReport>, String> {
    /// `v1.28.2+k3s1` → `1.28.2`. 업스트림 태그의 접두 v와 빌드 메타데이터를 걷어내
    /// 보유 버전과 같은 축으로 맞춘다(이 정규화 없이는 k3s가 상시 "업데이트 있음"이 된다).
    fn normalize(tag: &str) -> String {
        tag.trim()
            .trim_start_matches('v')
            .split('+')
            .next()
            .unwrap_or("")
            .to_string()
    }

    // 조회에 실패하면 "최신"이라고 단정하지 않고 실패 사실을 그대로 표시한다.
    const UNKNOWN: &str = "조회 실패";

    let check_repo = |owner: &'static str, repo: &'static str, cur_ver: &'static str| async move {
        let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
        if let Ok(mut cmd) = external_command("curl") {
            cmd.args(["-sS", "-m", "6", "-H", "User-Agent: KubeMetal", &url]);
            if let Ok(out) = cmd.output().await {
                if out.status.success() {
                    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                        if let Some(tag) = json["tag_name"].as_str() {
                            let latest = normalize(tag);
                            let has_update = !latest.is_empty() && latest != normalize(cur_ver);
                            return (latest, has_update);
                        }
                    }
                }
            }
        }
        (UNKNOWN.to_string(), false)
    };

    let with_v = |ver: String| {
        if ver == UNKNOWN {
            ver
        } else {
            format!("v{ver}")
        }
    };

    let (kagent_latest, kagent_up) = check_repo("kagent-dev", "kagent", "0.9.12").await;
    let (k3s_latest, k3s_up) = check_repo("k3s-io", "k3s", "1.28.2").await;
    let (kubescape_latest, ks_up) = check_repo("kubescape", "kubescape", "3.0.0").await;

    Ok(vec![
        AirgapLatestVersionReport {
            name: "kagent Helm & Controller".into(),
            current_version: "0.9.12".into(),
            latest_version: kagent_latest,
            has_update: kagent_up,
        },
        AirgapLatestVersionReport {
            name: "K3s Kubernetes Engine".into(),
            current_version: "v1.28.2".into(),
            latest_version: with_v(k3s_latest),
            has_update: k3s_up,
        },
        AirgapLatestVersionReport {
            name: "Kubescape Security CLI".into(),
            current_version: "v3.0.0".into(),
            latest_version: with_v(kubescape_latest),
            has_update: ks_up,
        },
    ])
}
