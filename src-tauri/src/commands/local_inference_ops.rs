use std::path::PathBuf;
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tauri::Manager;
use tokio::process::Command;

use crate::commands::guardrails::get_guardrail_status;
use crate::commands::mlx::MlxState;
use crate::services::local_inference::{
    loopback_http_request, LocalInferenceRuntimeKind, RuntimeCapabilities,
};
use crate::services::process::augmented_path;
use crate::services::runtime_adapter::{all_runtime_adapters, RuntimeAdapterDescriptor};

#[derive(Debug, Deserialize)]
pub struct ApiCapabilityProbeRequest {
    pub runtime: LocalInferenceRuntimeKind,
    pub endpoint: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiRouteProbe {
    pub name: String,
    pub path: String,
    pub supported: Option<bool>,
    pub status_code: Option<u16>,
    pub detail: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ApiCapabilityProbeResult {
    pub runtime: LocalInferenceRuntimeKind,
    pub endpoint: String,
    pub routes: Vec<ApiRouteProbe>,
}

#[derive(Debug, Deserialize)]
pub struct ConnectionProfileRequest {
    pub runtime: LocalInferenceRuntimeKind,
    pub endpoint: String,
    pub bridge_endpoint: Option<String>,
    #[serde(default)]
    pub api_key_configured: bool,
}

#[derive(Debug, Serialize)]
pub struct ConnectionProfile {
    pub runtime: LocalInferenceRuntimeKind,
    pub endpoint: String,
    pub openai_base_url: String,
    pub anthropic_base_url: Option<String>,
    pub k3s_base_url: Option<String>,
    pub api_key_placeholder: Option<String>,
    pub environment_lines: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeDiagnosticFinding {
    pub code: String,
    pub severity: DiagnosticSeverity,
    pub summary: String,
    pub evidence: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LocalInferenceDiagnostics {
    pub log_path: String,
    pub log_exists: bool,
    pub metal_wired_limit_mb: Option<u64>,
    pub physical_memory_bytes: Option<u64>,
    pub findings: Vec<RuntimeDiagnosticFinding>,
}

#[derive(Debug, Deserialize)]
pub struct ModelLoadPreflightRequest {
    pub model_id: String,
    pub estimated_memory_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AdmissionDecision {
    Allow,
    Warn,
    Deny,
}

#[derive(Debug, Serialize)]
pub struct ModelLoadPreflight {
    pub model_id: String,
    pub decision: AdmissionDecision,
    pub reasons: Vec<String>,
    pub memory_pressure_level: String,
    pub thermal_state: Option<String>,
    pub training_active: bool,
    pub metal_wired_limit_mb: Option<u64>,
}

#[tauri::command]
pub async fn list_local_inference_adapters() -> Result<Vec<RuntimeAdapterDescriptor>, String> {
    Ok(all_runtime_adapters())
}

fn capability_defaults(runtime: LocalInferenceRuntimeKind) -> RuntimeCapabilities {
    all_runtime_adapters()
        .into_iter()
        .find(|adapter| adapter.runtime == runtime)
        .map(|adapter| adapter.capabilities)
        .unwrap_or(RuntimeCapabilities {
            openai_chat: false,
            openai_responses: false,
            anthropic_messages: false,
            embeddings: false,
            rerank: false,
            multi_model: false,
            model_load_unload: false,
            model_pinning: false,
            model_ttl: false,
            continuous_batching: false,
            tiered_kv_cache: false,
            mcp: false,
        })
}

async fn route_probe(
    endpoint: &str,
    api_key: Option<&str>,
    name: &str,
    path: &str,
    body: &str,
    expected: bool,
) -> ApiRouteProbe {
    match loopback_http_request(endpoint, "POST", path, Some(body), api_key).await {
        Ok(response) => {
            let exists = !matches!(response.status, 404 | 405);
            ApiRouteProbe {
                name: name.into(),
                path: path.into(),
                supported: Some(exists && expected),
                status_code: Some(response.status),
                detail: (!response.body.trim().is_empty())
                    .then(|| response.body.chars().take(300).collect()),
            }
        }
        Err(error) => ApiRouteProbe {
            name: name.into(),
            path: path.into(),
            supported: None,
            status_code: None,
            detail: Some(error),
        },
    }
}

#[tauri::command]
pub async fn probe_local_inference_api_capabilities(
    request: ApiCapabilityProbeRequest,
) -> Result<ApiCapabilityProbeResult, String> {
    let advertised = capability_defaults(request.runtime);
    let token = request.api_key.as_deref();
    let model = "__kubemetal_capability_probe__";

    let (chat, anthropic, embeddings, rerank) = tokio::join!(
        route_probe(
            &request.endpoint,
            token,
            "OpenAI chat",
            "/v1/chat/completions",
            &format!(r#"{{"model":"{model}","messages":[{{"role":"user","content":"probe"}}],"max_tokens":1}}"#),
            advertised.openai_chat,
        ),
        route_probe(
            &request.endpoint,
            token,
            "Anthropic messages",
            "/v1/messages",
            &format!(r#"{{"model":"{model}","messages":[{{"role":"user","content":"probe"}}],"max_tokens":1}}"#),
            advertised.anthropic_messages,
        ),
        route_probe(
            &request.endpoint,
            token,
            "Embeddings",
            "/v1/embeddings",
            &format!(r#"{{"model":"{model}","input":"probe"}}"#),
            advertised.embeddings,
        ),
        route_probe(
            &request.endpoint,
            token,
            "Rerank",
            "/v1/rerank",
            &format!(r#"{{"model":"{model}","query":"probe","documents":["probe"]}}"#),
            advertised.rerank,
        )
    );

    Ok(ApiCapabilityProbeResult {
        runtime: request.runtime,
        endpoint: request.endpoint,
        routes: vec![chat, anthropic, embeddings, rerank],
    })
}

fn normalize_base(endpoint: &str) -> String {
    endpoint.trim_end_matches('/').to_string()
}

#[tauri::command]
pub async fn get_local_inference_connection_profile(
    request: ConnectionProfileRequest,
) -> Result<ConnectionProfile, String> {
    let capabilities = capability_defaults(request.runtime);
    let base = normalize_base(&request.endpoint);
    if !base.starts_with("http://127.0.0.1:") && !base.starts_with("http://localhost:") {
        return Err("Connection profile host endpoint must remain loopback-only".into());
    }
    let api_key_placeholder = request
        .api_key_configured
        .then(|| "<session-or-workload-secret>".to_string());
    let mut environment_lines = vec![format!("OPENAI_BASE_URL={base}/v1")];
    if request.api_key_configured {
        environment_lines.push("OPENAI_API_KEY=<set-at-runtime>".into());
    }
    if capabilities.anthropic_messages {
        environment_lines.push(format!("ANTHROPIC_BASE_URL={base}"));
        if request.api_key_configured {
            environment_lines.push("ANTHROPIC_API_KEY=<set-at-runtime>".into());
        }
    }
    let mut notes = vec![
        "Do not persist API keys in generated profiles or process arguments.".into(),
        "Tool-calling support is inference capability only; tool execution authorization remains outside this profile.".into(),
    ];
    if request.bridge_endpoint.is_some() {
        notes.push("Use the private bridge endpoint only from the intended K3s/Colima network; public/LAN exposure remains denied by default.".into());
    }

    Ok(ConnectionProfile {
        runtime: request.runtime,
        endpoint: base.clone(),
        openai_base_url: format!("{base}/v1"),
        anthropic_base_url: capabilities.anthropic_messages.then_some(base),
        k3s_base_url: request.bridge_endpoint.map(|value| normalize_base(&value)),
        api_key_placeholder,
        environment_lines,
        notes,
    })
}

async fn sysctl_u64(name: &str) -> Option<u64> {
    let output = Command::new("/usr/sbin/sysctl")
        .arg("-n")
        .arg(name)
        .env("PATH", augmented_path())
        .stdin(Stdio::null())
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

#[tauri::command]
pub async fn preflight_local_inference_model_load(
    app: tauri::AppHandle,
    request: ModelLoadPreflightRequest,
) -> Result<ModelLoadPreflight, String> {
    if request.model_id.trim().is_empty() {
        return Err("model_id is required".into());
    }
    let guardrail = get_guardrail_status(app.clone()).await?;
    let training_active = {
        let state = app.state::<MlxState>();
        let guard = state.training.lock().map_err(|e| e.to_string())?;
        guard.as_ref().is_some_and(|training| {
            !matches!(training.status.as_str(), "done" | "error" | "killed")
        })
    };
    let metal_wired_limit_mb = sysctl_u64("iogpu.wired_limit_mb").await;
    let mut decision = AdmissionDecision::Allow;
    let mut reasons = Vec::new();

    if guardrail.memory_pressure_level == "critical" {
        decision = AdmissionDecision::Deny;
        reasons.push("macOS memory pressure is critical".into());
    } else if guardrail.memory_pressure_level == "warn" {
        decision = AdmissionDecision::Warn;
        reasons.push("macOS memory pressure is elevated".into());
    } else if guardrail.memory_pressure_level == "unknown" {
        decision = AdmissionDecision::Warn;
        reasons.push("memory pressure could not be measured".into());
    }

    if matches!(guardrail.thermal_state.as_deref(), Some("critical") | Some("serious")) {
        decision = AdmissionDecision::Deny;
        reasons.push(format!(
            "thermal state is {}",
            guardrail.thermal_state.as_deref().unwrap_or("unknown")
        ));
    } else if guardrail.thermal_state.as_deref() == Some("fair") && decision == AdmissionDecision::Allow {
        decision = AdmissionDecision::Warn;
        reasons.push("thermal state is fair; additional inference load may throttle".into());
    }

    if training_active && decision != AdmissionDecision::Deny {
        decision = AdmissionDecision::Warn;
        reasons.push("MLX fine-tuning is active; model load will compete for unified memory".into());
    }

    if let (Some(estimated), Some(limit_mb)) = (request.estimated_memory_bytes, metal_wired_limit_mb) {
        let limit_bytes = limit_mb.saturating_mul(1024 * 1024);
        let safe_bytes = limit_bytes.saturating_mul(9) / 10;
        if estimated > safe_bytes {
            decision = AdmissionDecision::Deny;
            reasons.push(format!(
                "estimated model memory {} MiB exceeds 90% of Metal wired limit {} MiB",
                estimated / 1024 / 1024,
                limit_mb
            ));
        } else if training_active && estimated > safe_bytes / 2 && decision != AdmissionDecision::Deny {
            decision = AdmissionDecision::Warn;
            reasons.push("estimated model memory consumes more than half of the guarded Metal budget while training is active".into());
        }
    }

    if reasons.is_empty() {
        reasons.push("host guardrails report no blocking condition".into());
    }

    Ok(ModelLoadPreflight {
        model_id: request.model_id,
        decision,
        reasons,
        memory_pressure_level: guardrail.memory_pressure_level,
        thermal_state: guardrail.thermal_state,
        training_active,
        metal_wired_limit_mb,
    })
}

fn tail_file(path: &PathBuf, max_bytes: usize) -> String {
    let Ok(bytes) = std::fs::read(path) else {
        return String::new();
    };
    let start = bytes.len().saturating_sub(max_bytes);
    String::from_utf8_lossy(&bytes[start..]).to_string()
}

fn first_matching_line(text: &str, needles: &[&str]) -> Option<String> {
    text.lines()
        .rev()
        .find(|line| needles.iter().any(|needle| line.contains(needle)))
        .map(|line| line.chars().take(500).collect())
}

#[tauri::command]
pub async fn get_local_inference_diagnostics() -> Result<LocalInferenceDiagnostics, String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is unavailable".to_string())?;
    let log_path = home.join(".kubemetal/logs/omlx.log");
    let log_exists = log_path.is_file();
    let text = if log_exists {
        tail_file(&log_path, 512 * 1024)
    } else {
        String::new()
    };
    let metal_wired_limit_mb = sysctl_u64("iogpu.wired_limit_mb").await;
    let physical_memory_bytes = sysctl_u64("hw.memsize").await;
    let mut findings = Vec::new();

    if let Some(line) = first_matching_line(
        &text,
        &["Insufficient Memory", "kIOGPUCommandBufferCallbackErrorOutOfMemory"],
    ) {
        findings.push(RuntimeDiagnosticFinding {
            code: "metal-oom".into(),
            severity: DiagnosticSeverity::Critical,
            summary: "Metal reported an out-of-memory failure. Reduce model/context/concurrency or increase safety headroom before retrying.".into(),
            evidence: Some(line),
        });
    }
    if let Some(line) = first_matching_line(
        &text,
        &["Prefill context too large", "prefill safety cap", "guard:chunked_step"],
    ) {
        findings.push(RuntimeDiagnosticFinding {
            code: "prefill-memory-guard".into(),
            severity: DiagnosticSeverity::Warning,
            summary: "oMLX prefill memory guard throttled or rejected a long-context request. Treat this as degradation, not a healthy zero-throughput result.".into(),
            evidence: Some(line),
        });
    }
    if let Some(line) = first_matching_line(&text, &["Metal cap", "wired_limit_mb", "static ceiling"]) {
        findings.push(RuntimeDiagnosticFinding {
            code: "metal-cap-conflict".into(),
            severity: DiagnosticSeverity::Warning,
            summary: "Runtime logs indicate a Metal wired-memory cap conflict. KubeMetal does not modify this privileged sysctl automatically.".into(),
            evidence: Some(line),
        });
    }
    if let Some(line) = first_matching_line(
        &text,
        &["Reconstructed cache from tiered cache", "Cache hit for", "paged cache"],
    ) {
        findings.push(RuntimeDiagnosticFinding {
            code: "tiered-cache-evidence".into(),
            severity: DiagnosticSeverity::Info,
            summary: "Tiered/prefix cache reuse evidence was observed in the runtime log.".into(),
            evidence: Some(line),
        });
    }
    if let Some(line) = first_matching_line(&text, &["LRU eviction", "eviction", "unload"]) {
        findings.push(RuntimeDiagnosticFinding {
            code: "model-eviction-evidence".into(),
            severity: DiagnosticSeverity::Info,
            summary: "Model/cache eviction evidence was observed; correlate with request latency and memory pressure before treating it as an error.".into(),
            evidence: Some(line),
        });
    }

    if findings.is_empty() {
        findings.push(RuntimeDiagnosticFinding {
            code: "no-known-runtime-alerts".into(),
            severity: DiagnosticSeverity::Info,
            summary: if log_exists {
                "No known Metal OOM, prefill-guard, cache, or eviction signatures were found in the retained log tail. This does not imply the runtime is healthy; use live probes and benchmark evidence as well.".into()
            } else {
                "No KubeMetal-managed oMLX log exists yet.".into()
            },
            evidence: None,
        });
    }

    Ok(LocalInferenceDiagnostics {
        log_path: log_path.display().to_string(),
        log_exists,
        metal_wired_limit_mb,
        physical_memory_bytes,
        findings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_profile_never_contains_a_real_secret() {
        let request = ConnectionProfileRequest {
            runtime: LocalInferenceRuntimeKind::Omlx,
            endpoint: "http://127.0.0.1:8000".into(),
            bridge_endpoint: Some("http://192.168.5.2:8000".into()),
            api_key_configured: true,
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let profile = runtime.block_on(get_local_inference_connection_profile(request)).unwrap();
        assert!(profile.environment_lines.iter().all(|line| !line.contains("secret")));
        assert!(profile.environment_lines.iter().any(|line| line.contains("<set-at-runtime>")));
    }

    #[test]
    fn log_matching_prefers_latest_evidence() {
        let text = "old Cache hit for x\nnoise\nnew Cache hit for y";
        assert_eq!(
            first_matching_line(text, &["Cache hit"]).as_deref(),
            Some("new Cache hit for y")
        );
    }
}
