use std::sync::Mutex;
use serde::Serialize;
use sysinfo::System;
use tauri::State;

#[derive(Serialize)]
pub struct SystemMetrics {
    pub total_memory_gb: f64,
    pub used_memory_gb: f64,
    pub memory_usage_percentage: f32,
    pub cpu_usage_percentage: f32,
}

#[tauri::command]
pub fn get_system_metrics(state: State<'_, Mutex<System>>) -> Result<SystemMetrics, String> {
    let mut sys = state.lock().map_err(|e| e.to_string())?;
    sys.refresh_memory();
    sys.refresh_cpu_usage();

    let total = sys.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    let used = sys.used_memory() as f64 / 1024.0 / 1024.0 / 1024.0;

    Ok(SystemMetrics {
        total_memory_gb: (total * 100.0).round() / 100.0,
        used_memory_gb: (used * 100.0).round() / 100.0,
        memory_usage_percentage: ((used / total * 100.0) as f32 * 10.0).round() / 10.0,
        cpu_usage_percentage: (sys.global_cpu_info().cpu_usage() * 10.0).round() / 10.0,
    })
}
