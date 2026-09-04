use serde::Serialize;

use crate::services::local_inference::{probe_all_runtimes, probe_runtime, LocalInferenceRuntimeKind, RuntimeProbe};

#[derive(Debug, Serialize)]
pub struct LocalInferenceStatus {
    pub preferred_runtime: Option<LocalInferenceRuntimeKind>,
    pub runtimes: Vec<RuntimeProbe>,
}

#[tauri::command]
pub async fn get_local_inference_status() -> Result<LocalInferenceStatus, String> {
    let runtimes = probe_all_runtimes().await;
    let preferred_runtime = runtimes
        .iter()
        .find(|r| r.runtime == LocalInferenceRuntimeKind::Omlx && r.installed)
        .or_else(|| runtimes.iter().find(|r| r.runtime == LocalInferenceRuntimeKind::MlxLm && r.installed))
        .map(|r| r.runtime);

    Ok(LocalInferenceStatus {
        preferred_runtime,
        runtimes,
    })
}

#[tauri::command]
pub async fn probe_local_inference_runtime(
    runtime: LocalInferenceRuntimeKind,
) -> Result<RuntimeProbe, String> {
    Ok(probe_runtime(runtime).await)
}
