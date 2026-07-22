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
    pub gpu_usage_percentage: f32,
    pub gpu_memory_used_gb: f64,
}

fn get_metal_gpu_metrics() -> (f32, f64) {
    let output = std::process::Command::new("ioreg")
        .args(["-l", "-d", "1", "-r", "-c", "IOAccelerator"])
        .output();

    let Ok(out) = output else {
        return (0.0, 0.0);
    };

    let text = String::from_utf8_lossy(&out.stdout);
    let mut gpu_pct: f32 = 0.0;
    let mut gpu_mem_bytes: f64 = 0.0;

    for line in text.lines() {
        if line.contains("Device Utilization %") {
            if let Some(val_str) = line.split("Device Utilization %\"=").nth(1) {
                if let Some(num_str) = val_str.split(|c: char| !c.is_numeric()).next() {
                    if let Ok(val) = num_str.parse::<f32>() {
                        gpu_pct = val;
                    }
                }
            }
        }
        if line.contains("In use system memory") && !line.contains("driver") {
            if let Some(val_str) = line.split("In use system memory\"=").nth(1) {
                if let Some(num_str) = val_str.split(|c: char| !c.is_numeric()).next() {
                    if let Ok(val) = num_str.parse::<f64>() {
                        gpu_mem_bytes = val;
                    }
                }
            }
        }
    }

    let gpu_mem_gb = (gpu_mem_bytes / 1024.0 / 1024.0 / 1024.0 * 100.0).round() / 100.0;
    (gpu_pct, gpu_mem_gb)
}

#[tauri::command]
pub fn get_system_metrics(state: State<'_, Mutex<System>>) -> Result<SystemMetrics, String> {
    let mut sys = state.lock().map_err(|e| e.to_string())?;
    sys.refresh_memory();
    sys.refresh_cpu_usage();

    let total = sys.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    let used = sys.used_memory() as f64 / 1024.0 / 1024.0 / 1024.0;

    let (gpu_usage_percentage, gpu_memory_used_gb) = get_metal_gpu_metrics();

    Ok(SystemMetrics {
        total_memory_gb: (total * 100.0).round() / 100.0,
        used_memory_gb: (used * 100.0).round() / 100.0,
        memory_usage_percentage: ((used / total * 100.0) as f32 * 10.0).round() / 10.0,
        cpu_usage_percentage: (sys.global_cpu_usage() * 10.0).round() / 10.0,
        gpu_usage_percentage,
        gpu_memory_used_gb,
    })
}
