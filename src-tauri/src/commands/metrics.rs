use std::sync::{Mutex, Once};

use serde::Serialize;
use sysinfo::System;
use tauri::State;

use crate::services::process::external_command;

/// 정적 하드웨어 스펙. `gpu_cores`만 `Option`인 이유 — sysctl은 어떤 Mac에서도 CPU/RAM을
/// 돌려주지만 GPU 코어 수는 `system_profiler` 출력 포맷에 의존해 파싱이 실패할 수 있다.
/// 실패 시 스펙을 **추정해 채우지 않는다**(다른 기기에서 허위 스펙이 표시된다).
#[derive(Clone, Serialize, Debug)]
pub struct HardwareSpec {
    pub brand_name: String,
    pub cpu_cores: u32,
    pub total_memory_gb: u32,
    pub gpu_cores: Option<u32>,
}

/// 정적 값이므로 프로세스 수명 동안 1회만 조회한다(`system_profiler`는 수 초가 걸린다).
static HARDWARE_SPEC_CACHE: Mutex<Option<HardwareSpec>> = Mutex::new(None);

async fn sysctl_value(key: &str) -> Result<String, String> {
    let out = external_command("sysctl")?
        .args(["-n", key])
        .output()
        .await
        .map_err(|e| format!("sysctl {key} 실행 실패: {e}"))?;

    if !out.status.success() {
        return Err(format!(
            "sysctl {key} 실패: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// D2 규약: 하드웨어 조회도 sudo 없이 `external_command`(절대경로 + 보강 PATH)로만 스폰한다.
/// 블로킹 `std::process::Command`는 async 커맨드에서 런타임 스레드를 점유하므로 금지.
#[tauri::command]
pub async fn get_hardware_spec() -> Result<HardwareSpec, String> {
    if let Some(cached) = HARDWARE_SPEC_CACHE
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
    {
        return Ok(cached);
    }

    let brand_name = sysctl_value("machdep.cpu.brand_string").await?;
    let cpu_cores = sysctl_value("hw.ncpu")
        .await?
        .parse::<u32>()
        .map_err(|e| format!("hw.ncpu 파싱 실패: {e}"))?;
    let total_memory_gb = (sysctl_value("hw.memsize")
        .await?
        .parse::<u64>()
        .map_err(|e| format!("hw.memsize 파싱 실패: {e}"))?
        / 1024
        / 1024
        / 1024) as u32;

    // GPU 코어 수는 부가 정보 — 실패해도 나머지 스펙은 유효하므로 None으로 둔다.
    let gpu_cores = match external_command("system_profiler") {
        Ok(mut cmd) => cmd
            .arg("SPDisplaysDataType")
            .output()
            .await
            .ok()
            .filter(|out| out.status.success())
            .and_then(|out| {
                String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .find(|l| l.contains("Total Number of Cores"))
                    .and_then(|l| l.split(':').nth(1))
                    .and_then(|s| s.trim().parse::<u32>().ok())
            }),
        Err(_) => None,
    };

    let spec = HardwareSpec {
        brand_name,
        cpu_cores,
        total_memory_gb,
        gpu_cores,
    };

    *HARDWARE_SPEC_CACHE.lock().map_err(|e| e.to_string())? = Some(spec.clone());
    Ok(spec)
}

#[derive(Serialize)]
pub struct SystemMetrics {
    pub total_memory_gb: f64,
    pub used_memory_gb: f64,
    pub memory_usage_percentage: f32,
    pub cpu_usage_percentage: f32,
    pub gpu_usage_percentage: f32,
    pub gpu_memory_used_gb: f64,
    /// `nominal` | `fair` | `serious` | `critical`. 값을 못 읽으면 None —
    /// "정상"으로 폴백하지 않는다(D22, 발열은 가드레일 판정에 쓰인다).
    pub thermal_state: Option<String>,
}

/// macOS의 발열 압력 단계.
///
/// CLI로는 읽을 수 없다 — 이 기기 실측(2026-07-27, M4 Pro / macOS 26):
/// `pmset -g therm`은 "No thermal warning level has been recorded"만 내고,
/// `sysctl -a`에 thermal 키가 없으며, `ioreg -c AppleSMC`에 온도 항목이 0개다.
/// 유일한 sudo-free 경로가 NSProcessInfo.thermalState라 objc 바인딩을 쓴다.
///
/// 발열이 왜 필요한가: 메모리 압력(D16)은 "RAM이 모자란가"를 말할 뿐, 장시간 파인튜닝에서
/// 실제로 스로틀링을 유발하는 신호는 발열이다. Nativ가 tok/s와 함께 이 값을 표면화하는
/// 이유이기도 하다.
pub fn read_thermal_state() -> Option<String> {
    use objc2_foundation::{NSProcessInfo, NSProcessInfoThermalState};

    let info = NSProcessInfo::processInfo();
    Some(
        match info.thermalState() {
            NSProcessInfoThermalState::Nominal => "nominal",
            NSProcessInfoThermalState::Fair => "fair",
            NSProcessInfoThermalState::Serious => "serious",
            NSProcessInfoThermalState::Critical => "critical",
            // 애플이 단계를 추가하면 이름을 지어내지 않고 미상으로 둔다.
            _ => return None,
        }
        .to_string(),
    )
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
        thermal_state: read_thermal_state(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 이 테스트는 발열 값이 **실제로 읽히는지**를 확인한다. CLI 경로가 전부 비어 있는
    /// 것을 실측으로 확인하고 objc로 넘어온 것이므로, 여기서 None이 나오면 그 전제가
    /// 깨졌다는 뜻이고 UI에 "미상"만 뜨게 된다.
    #[test]
    fn thermal_state_is_actually_readable() {
        let state = read_thermal_state();
        assert!(
            state.is_some(),
            "NSProcessInfo.thermalState를 읽지 못했다 — CLI 대체 경로가 없으므로 \
             발열 표시가 통째로 죽는다"
        );
        assert!(
            ["nominal", "fair", "serious", "critical"].contains(&state.as_deref().unwrap()),
            "예상 밖의 발열 단계: {state:?}"
        );
    }
}
