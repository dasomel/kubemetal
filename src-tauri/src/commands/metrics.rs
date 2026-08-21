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
        .map_err(|e| format!("Failed to run sysctl {key}: {e}"))?;

    if !out.status.success() {
        return Err(format!(
            "sysctl {key} failed: {}",
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
        .map_err(|e| format!("Failed to parse hw.ncpu: {e}"))?;
    let total_memory_gb = (sysctl_value("hw.memsize")
        .await?
        .parse::<u64>()
        .map_err(|e| format!("Failed to parse hw.memsize: {e}"))?
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
            eprintln!("[metrics] ioreg executable not found, skipping GPU metrics.");
        });
        return (0.0, 0.0);
    };

    let output = cmd
        .args(["-l", "-d", "1", "-r", "-c", "IOAccelerator"])
        .output()
        .await;

    let Ok(out) = output else {
        GPU_METRICS_WARNED.call_once(|| {
            eprintln!("[metrics] ioreg execution failed, skipping GPU metrics.");
        });
        return (0.0, 0.0);
    };
    if !out.status.success() {
        GPU_METRICS_WARNED.call_once(|| {
            eprintln!(
                "[metrics] ioreg exited abnormally ({}), skipping GPU metrics.",
                out.status
            );
        });
        return (0.0, 0.0);
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let (pct, mem_bytes) = parse_ioreg_accelerator(&text);

    // 필드별로 판정한다. 예전에는 "둘 중 하나라도 잡히면 성공"이라 사용률만 읽히고 메모리가
    // 통째로 실패해도 경고가 뜨지 않았다 — 실제로 그 상태였고, 화면은 0 GB를 조용히 띄웠다.
    if pct.is_none() || mem_bytes.is_none() {
        GPU_METRICS_WARNED.call_once(|| {
            eprintln!(
                "[metrics] ioreg IOAccelerator parse incomplete (utilization: {}, memory: {}) \
                 — possible format change or unsupported hardware; missing values report 0.",
                if pct.is_some() { "ok" } else { "MISSING" },
                if mem_bytes.is_some() { "ok" } else { "MISSING" },
            );
        });
    }

    let gpu_mem_gb = (mem_bytes.unwrap_or(0.0) / 1024.0 / 1024.0 / 1024.0 * 100.0).round() / 100.0;
    (pct.unwrap_or(0.0), gpu_mem_gb)
}

/// `ioreg -l -d 1 -r -c IOAccelerator` 출력에서 (사용률 %, 사용 중 시스템 메모리 바이트)를
/// 뽑는다. 읽지 못한 값은 `None`이다 — 0.0으로 뭉개면 "GPU 유휴"와 구분되지 않는다(D22).
///
/// 셸 실행에서 분리한 순수 함수인 이유: 이 파서는 실측 출력 형태를 잘못 가정해 **메모리를
/// 영원히 0으로 보고하고 있었다**. `PerformanceStatistics`는 키가 줄마다 나뉘지 않고 한 줄에
/// 딕셔너리로 들어오며, 그 줄에는 `"In use system memory (driver)"=0`이 실제 값과 **함께**
/// 있다. 그래서 줄 단위로 `driver`를 배제하던 가드가 메모리 분기를 통째로 막았다.
/// 실제 구분은 검색 패턴이 이미 하고 있다 — `(driver)` 키는 `memory` 뒤에 `"=`가 오지 않아
/// `In use system memory"=`와 매치되지 않는다.
fn parse_ioreg_accelerator(text: &str) -> (Option<f32>, Option<f64>) {
    /// `<키>"=<숫자>` 형태에서 첫 숫자를 읽는다. 키가 값 없이 등장하는 자리(IOReportLegend의
    /// 채널 이름 목록)는 `"=`가 뒤따르지 않으므로 자연히 걸러진다.
    fn field_after<'a>(text: &'a str, key: &str) -> Option<&'a str> {
        let needle = format!("{key}\"=");
        let rest = text.split(&needle).nth(1)?;
        let end = rest
            .find(|c: char| !c.is_numeric())
            .unwrap_or(rest.len());
        Some(&rest[..end]).filter(|s| !s.is_empty())
    }

    let pct = field_after(text, "Device Utilization %").and_then(|s| s.parse::<f32>().ok());
    let mem = field_after(text, "In use system memory").and_then(|s| s.parse::<f64>().ok());
    (pct, mem)
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
            "Failed to read NSProcessInfo.thermalState — there is no CLI fallback, so \
             thermal display breaks entirely"
        );
        assert!(
            ["nominal", "fair", "serious", "critical"].contains(&state.as_deref().unwrap()),
            "Unexpected thermal state: {state:?}"
        );
    }

    /// 실측 출력 픽스처. 이 파서는 형태를 잘못 가정해 메모리를 영원히 0으로 보고했고,
    /// 사용률이 정상이라 경고조차 뜨지 않았다 — 실기기에서만 드러나는 종류였다.
    /// 픽스처는 그 형태(한 줄 딕셔너리 + 같은 줄의 `(driver)` 키)를 고정한다.
    const FIXTURE: &str = include_str!("../../tests/fixtures/ioreg-ioaccelerator.txt");

    #[test]
    fn parses_real_ioreg_output() {
        let (pct, mem) = parse_ioreg_accelerator(FIXTURE);
        assert_eq!(pct, Some(30.0), "Device Utilization %를 읽지 못했다");
        assert_eq!(mem, Some(939048960.0), "In use system memory를 읽지 못했다");
    }

    /// 회귀 방지의 핵심. `(driver)` 변종이 실제 값과 **같은 줄**에 있어도 실제 값을 읽어야
    /// 하고, 0인 `(driver)` 값을 집어오면 안 된다.
    #[test]
    fn driver_variant_on_same_line_does_not_shadow_real_memory() {
        let line = r#"  "PerformanceStatistics" = {"In use system memory (driver)"=0,"Device Utilization %"=7,"In use system memory"=1302069248}"#;
        let (pct, mem) = parse_ioreg_accelerator(line);
        assert_eq!(pct, Some(7.0));
        assert_eq!(
            mem,
            Some(1302069248.0),
            "`(driver)`=0을 실제 값으로 착각했거나 메모리 분기가 통째로 막혔다"
        );
    }

    /// 키 이름이 값 없이 등장하는 자리(IOReportLegend의 채널 목록)에 낚이면 안 된다.
    #[test]
    fn key_without_value_is_not_matched() {
        let legend = r#"  "IOReportLegend" = ({"IOReportChannels"=((2,6442450945,"In use system memory"))})"#;
        assert_eq!(parse_ioreg_accelerator(legend), (None, None));
    }

    /// 픽스처는 "이 형태를 이렇게 읽는다"만 보장한다. 이 기기의 **실제** ioreg가 여전히 그
    /// 형태인지는 별개 사실이고, 틀어지면 화면이 0을 조용히 띄운다 — 원래 버그가 그랬다.
    /// `thermal_state_is_actually_readable`과 같은 취지로 실제 값이 읽히는지를 본다.
    ///
    /// 값의 크기가 아니라 **필드를 찾았는지**를 본다. 유휴 GPU에서 사용률 0은 정상이지만,
    /// `None`은 파싱 실패이고 그것이 회귀다.
    #[test]
    fn live_ioreg_output_is_still_parseable() {
        let out = std::process::Command::new("/usr/sbin/ioreg")
            .args(["-l", "-d", "1", "-r", "-c", "IOAccelerator"])
            .output();
        let Ok(out) = out else {
            return; // ioreg가 없는 환경이면 이 테스트가 말할 수 있는 것이 없다
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let (pct, mem) = parse_ioreg_accelerator(&text);
        assert!(
            pct.is_some() && mem.is_some(),
            "실제 ioreg 출력에서 GPU 필드를 못 읽었다 (사용률: {pct:?}, 메모리: {mem:?}) — \
             출력 형식이 바뀌었다면 픽스처와 파서를 함께 갱신해야 한다"
        );
    }

    /// 파서가 아니라 **수집 경로 전체**를 본다 — `external_command("ioreg")` 해석,
    /// 실행, 종료코드, 파싱까지. `live_ioreg_output_is_still_parseable`은 절대경로로
    /// ioreg를 직접 부르므로 이 층을 건너뛴다. GUI 번들 앱이 로그인 셸 PATH를 상속하지
    /// 않아 ioreg를 못 찾고 (0.0, 0.0)을 돌려주던 전례가 있다(D5/mistakes-log 2026-07-20).
    ///
    /// 메모리는 0보다 커야 한다 — WindowServer가 항상 얼마간 잡고 있어 유휴에서도 0이
    /// 아니다(실측: 유휴 시 1.0~1.3GB). 사용률은 유휴에서 0이 정상이라 보지 않는다.
    #[tokio::test]
    async fn metal_gpu_metrics_read_through_external_command() {
        let (_pct, mem_gb) = get_metal_gpu_metrics().await;
        assert!(
            mem_gb > 0.0,
            "GPU 메모리가 0이다 — 파서가 아니라 수집 경로(external_command/실행)가 \
             깨졌을 수 있다. 앱 화면이 0 GB로 보이는 것과 같은 증상이다"
        );
    }

    /// 읽지 못한 값은 0이 아니라 None이다 — 0으로 뭉개면 "GPU 유휴"와 구분되지 않는다(D22).
    #[test]
    fn missing_or_malformed_fields_are_none_not_zero() {
        assert_eq!(parse_ioreg_accelerator(""), (None, None));
        assert_eq!(parse_ioreg_accelerator("전혀 관계없는 출력"), (None, None));
        // 값 자리가 비어 있거나 숫자가 아닌 경우
        assert_eq!(
            parse_ioreg_accelerator(r#""Device Utilization %"=,"In use system memory"=abc"#),
            (None, None)
        );
        // 한쪽만 있는 경우 — 나머지는 None으로 남아 호출부가 경고를 낼 수 있어야 한다
        let (pct, mem) = parse_ioreg_accelerator(r#""Device Utilization %"=42"#);
        assert_eq!((pct, mem), (Some(42.0), None));
    }
}
