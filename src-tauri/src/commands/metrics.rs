use std::sync::{Mutex, Once};

use serde::Serialize;
use sysinfo::System;
use tauri::State;

use crate::services::process::external_command;

#[derive(Serialize)]
pub struct SystemMetrics {
    pub total_memory_gb: f64,
    pub used_memory_gb: f64,
    pub memory_usage_percentage: f32,
    pub cpu_usage_percentage: f32,
    pub gpu_usage_percentage: f32,
    pub gpu_memory_used_gb: f64,
}

/// 1초 주기로 폴링되는 커맨드이므로 파싱/실행 실패를 매 틱 로그하면 스팸이 된다 — 최초
/// 1회만 stderr에 남긴다.
static GPU_METRICS_WARNED: Once = Once::new();

/// bare `Command::new("ioreg")`는 PATH 탐색에 의존하는데, GUI 번들 앱은 로그인 셸 PATH를
/// 상속하지 않아(D5/mistakes-log 2026-07-20) ioreg를 못 찾고 항상 (0.0, 0.0)을 반환했다
/// — 실기기 번들 앱에서 GPU 사용률이 0 고정으로 보이던 원인. `external_command`로
/// `/usr/sbin`(SEARCH_PATHS에 포함, D16) 절대경로 탐색 + 보강 PATH를 적용한다.
async fn get_metal_gpu_metrics() -> (f32, f64) {
    let cmd = external_command("ioreg");
    let Ok(mut cmd) = cmd else {
        GPU_METRICS_WARNED.call_once(|| {
            eprintln!("[metrics] ioreg 실행 파일을 찾을 수 없어 GPU 메트릭을 건너뜁니다.");
        });
        return (0.0, 0.0);
    };

    let output = cmd
        .args(["-l", "-d", "1", "-r", "-c", "IOAccelerator"])
        .output()
        .await;

    let Ok(out) = output else {
        GPU_METRICS_WARNED.call_once(|| {
            eprintln!("[metrics] ioreg 실행 실패로 GPU 메트릭을 건너뜁니다.");
        });
        return (0.0, 0.0);
    };
    if !out.status.success() {
        GPU_METRICS_WARNED.call_once(|| {
            eprintln!(
                "[metrics] ioreg가 비정상 종료({})되어 GPU 메트릭을 건너뜁니다.",
                out.status
            );
        });
        return (0.0, 0.0);
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let mut gpu_pct: f32 = 0.0;
    let mut gpu_mem_bytes: f64 = 0.0;
    let mut matched_any = false;

    for line in text.lines() {
        if line.contains("Device Utilization %") {
            if let Some(val_str) = line.split("Device Utilization %\"=").nth(1) {
                if let Some(num_str) = val_str.split(|c: char| !c.is_numeric()).next() {
                    if let Ok(val) = num_str.parse::<f32>() {
                        gpu_pct = val;
                        matched_any = true;
                    }
                }
            }
        }
        if line.contains("In use system memory") && !line.contains("driver") {
            if let Some(val_str) = line.split("In use system memory\"=").nth(1) {
                if let Some(num_str) = val_str.split(|c: char| !c.is_numeric()).next() {
                    if let Ok(val) = num_str.parse::<f64>() {
                        gpu_mem_bytes = val;
                        matched_any = true;
                    }
                }
            }
        }
    }

    if !matched_any {
        GPU_METRICS_WARNED.call_once(|| {
            eprintln!(
                "[metrics] ioreg 출력에서 IOAccelerator 사용률/메모리 필드를 찾지 못했습니다 \
                 (포맷 변경 또는 비지원 하드웨어 가능성) — GPU 메트릭은 0으로 유지됩니다."
            );
        });
    }

    let gpu_mem_gb = (gpu_mem_bytes / 1024.0 / 1024.0 / 1024.0 * 100.0).round() / 100.0;
    (gpu_pct, gpu_mem_gb)
}

#[tauri::command]
pub async fn get_system_metrics(state: State<'_, Mutex<System>>) -> Result<SystemMetrics, String> {
    let (total, used, cpu_usage_percentage) = {
        let mut sys = state.lock().map_err(|e| e.to_string())?;
        sys.refresh_memory();
        sys.refresh_cpu_usage();
        let total = sys.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
        let used = sys.used_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
        let cpu_usage_percentage = (sys.global_cpu_usage() * 10.0).round() / 10.0;
        (total, used, cpu_usage_percentage)
    };

    let (gpu_usage_percentage, gpu_memory_used_gb) = get_metal_gpu_metrics().await;

    Ok(SystemMetrics {
        total_memory_gb: (total * 100.0).round() / 100.0,
        used_memory_gb: (used * 100.0).round() / 100.0,
        memory_usage_percentage: ((used / total * 100.0) as f32 * 10.0).round() / 10.0,
        cpu_usage_percentage,
        gpu_usage_percentage,
        gpu_memory_used_gb,
    })
}
