use std::process::Stdio;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::services::local_inference::{
    build_omlx_command, endpoint_for, omlx_model_action, pid_is_running, probe_all_runtimes,
    probe_live_runtime, probe_runtime, terminate_pid, LocalInferenceRuntimeKind,
    RuntimeLaunchConfig, RuntimeLiveStatus, RuntimeProbe,
};

#[derive(Debug, Serialize)]
pub struct LocalInferenceStatus {
    pub preferred_runtime: Option<LocalInferenceRuntimeKind>,
    pub runtimes: Vec<RuntimeProbe>,
    pub managed_process: Option<ManagedRuntimeProcess>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManagedRuntimeProcess {
    pub runtime: LocalInferenceRuntimeKind,
    pub pid: u32,
    pub endpoint: String,
    pub running: bool,
}

#[derive(Default)]
pub struct LocalInferenceState {
    managed: Mutex<Option<ManagedRuntimeProcess>>,
}

#[derive(Debug, Deserialize)]
pub struct RuntimeLiveProbeRequest {
    pub runtime: LocalInferenceRuntimeKind,
    pub endpoint: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OmlxModelActionRequest {
    pub endpoint: String,
    pub model_id: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RuntimeActionResult {
    pub ok: bool,
    pub status_code: Option<u16>,
    pub detail: Option<String>,
}

fn managed_process(state: &LocalInferenceState) -> Result<Option<ManagedRuntimeProcess>, String> {
    let mut guard = state
        .managed
        .lock()
        .map_err(|_| "Local inference state lock poisoned".to_string())?;
    if let Some(process) = guard.as_mut() {
        process.running = pid_is_running(process.pid);
        if !process.running {
            *guard = None;
        }
    }
    Ok(guard.clone())
}

#[tauri::command]
pub async fn get_local_inference_status(
    state: tauri::State<'_, LocalInferenceState>,
) -> Result<LocalInferenceStatus, String> {
    let runtimes = probe_all_runtimes().await;
    let preferred_runtime = runtimes
        .iter()
        .find(|r| r.runtime == LocalInferenceRuntimeKind::Omlx && r.installed)
        .or_else(|| {
            runtimes
                .iter()
                .find(|r| r.runtime == LocalInferenceRuntimeKind::MlxLm && r.installed)
        })
        .map(|r| r.runtime);

    Ok(LocalInferenceStatus {
        preferred_runtime,
        runtimes,
        managed_process: managed_process(&state)?,
    })
}

#[tauri::command]
pub async fn probe_local_inference_runtime(
    runtime: LocalInferenceRuntimeKind,
) -> Result<RuntimeProbe, String> {
    Ok(probe_runtime(runtime).await)
}

#[tauri::command]
pub async fn probe_local_inference_live(
    request: RuntimeLiveProbeRequest,
) -> Result<RuntimeLiveStatus, String> {
    Ok(probe_live_runtime(
        request.runtime,
        &request.endpoint,
        request.api_key.as_deref(),
    )
    .await)
}

#[tauri::command]
pub async fn start_local_inference_runtime(
    config: RuntimeLaunchConfig,
    state: tauri::State<'_, LocalInferenceState>,
) -> Result<ManagedRuntimeProcess, String> {
    if let Some(existing) = managed_process(&state)? {
        return Err(format!(
            "A KubeMetal-managed local inference runtime is already running (pid {}, {:?})",
            existing.pid, existing.runtime
        ));
    }

    let endpoint = endpoint_for(config.port);
    let live = probe_live_runtime(config.runtime, &endpoint, None).await;
    if live.reachable {
        return Err(format!(
            "Port {} already exposes a local inference server. KubeMetal will not take ownership of an existing process.",
            config.port
        ));
    }

    let mut command = build_omlx_command(&config).await?;
    let log_dir = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| "HOME is unavailable".to_string())?
        .join(".kubemetal")
        .join("logs");
    std::fs::create_dir_all(&log_dir)
        .map_err(|e| format!("Failed to create local inference log directory: {e}"))?;
    let log_path = log_dir.join("omlx.log");
    let stdout = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| format!("Failed to open {}: {e}", log_path.display()))?;
    let stderr = stdout
        .try_clone()
        .map_err(|e| format!("Failed to clone oMLX log handle: {e}"))?;
    command
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .kill_on_drop(false);

    let mut child = command
        .spawn()
        .map_err(|e| format!("Failed to start oMLX: {e}"))?;
    let pid = child
        .id()
        .ok_or_else(|| "oMLX started without a process id".to_string())?;
    let process = ManagedRuntimeProcess {
        runtime: config.runtime,
        pid,
        endpoint,
        running: true,
    };
    {
        let mut guard = state
            .managed
            .lock()
            .map_err(|_| "Local inference state lock poisoned".to_string())?;
        *guard = Some(process.clone());
    }

    // Reap the process asynchronously. The status command reconciles stale state via pid probing.
    tokio::spawn(async move {
        let _ = child.wait().await;
    });

    Ok(process)
}

#[tauri::command]
pub async fn stop_local_inference_runtime(
    state: tauri::State<'_, LocalInferenceState>,
) -> Result<RuntimeActionResult, String> {
    let process = managed_process(&state)?
        .ok_or_else(|| "No KubeMetal-managed local inference runtime is running".to_string())?;
    terminate_pid(process.pid)?;
    let mut guard = state
        .managed
        .lock()
        .map_err(|_| "Local inference state lock poisoned".to_string())?;
    *guard = None;
    Ok(RuntimeActionResult {
        ok: true,
        status_code: None,
        detail: Some(format!("SIGTERM sent to pid {}", process.pid)),
    })
}

#[tauri::command]
pub async fn load_omlx_model(
    request: OmlxModelActionRequest,
) -> Result<RuntimeActionResult, String> {
    let response = omlx_model_action(
        &request.endpoint,
        &request.model_id,
        "load",
        request.api_key.as_deref(),
    )
    .await?;
    Ok(RuntimeActionResult {
        ok: (200..300).contains(&response.status),
        status_code: Some(response.status),
        detail: (!response.body.trim().is_empty()).then_some(response.body),
    })
}

#[tauri::command]
pub async fn unload_omlx_model(
    request: OmlxModelActionRequest,
) -> Result<RuntimeActionResult, String> {
    let response = omlx_model_action(
        &request.endpoint,
        &request.model_id,
        "unload",
        request.api_key.as_deref(),
    )
    .await?;
    Ok(RuntimeActionResult {
        ok: (200..300).contains(&response.status),
        status_code: Some(response.status),
        detail: (!response.body.trim().is_empty()).then_some(response.body),
    })
}
