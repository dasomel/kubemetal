use std::path::PathBuf;
use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sysinfo::System;
use tauri::{Manager, State};

use crate::services::process::{augmented_path, external_command, resolve_bundled_resource};

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
    /// GPU 지표를 낸 백엔드. 이 저장소는 Apple Silicon 전용이므로 현재는 항상
    /// `"apple_metal"` 고정값이다 — 원격 클러스터의 NVIDIA 텔레메트리가 붙을 때
    /// `"nvidia"` 등 다른 값을 구분할 자리를 미리 만들어 둔 것뿐, 지금은 지어내지 않는다.
    pub gpu_backend: String,
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
    use objc2_foundation::NSProcessInfo;

    thermal_state_name(NSProcessInfo::processInfo().thermalState())
}

/// `NSProcessInfoThermalState` 원시값 → 문자열 매핑을 순수 함수로 분리한 것. objc 호출과
/// 분리해 두면 애플이 정의하지 않은(또는 아직 이 바인딩이 모르는) raw 값이 들어왔을 때도
/// 실제 기기 호출 없이 테스트할 수 있다 — 이름을 지어내지 않고 None으로 남기는 동작(D22)이
/// 이 함수의 계약이다.
fn thermal_state_name(state: objc2_foundation::NSProcessInfoThermalState) -> Option<String> {
    use objc2_foundation::NSProcessInfoThermalState;

    Some(
        match state {
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

/// GPU 텔레메트리 백엔드 공통 인터페이스 (이슈 #1 — GPU Telemetry Interoperability).
///
/// 현재 구현체는 [`AppleMetalBackend`] 하나뿐이다. NVIDIA(원격 클러스터의 GPU, Narwhal이
/// source-of-truth)는 **실기기가 없어 구현을 보류**한다 — 장비를 확보하면 이 trait을
/// 구현하는 `NvidiaBackend`(예: DCGM/nvidia-smi 어댑터)를 추가하면 된다. 없는 백엔드를
/// 지어내지 않는다(D22) — 그래서 지금은 인터페이스만 남기고 구현체를 만들지 않았다.
trait GpuTelemetryBackend {
    /// [`SystemMetrics::gpu_backend`]에 그대로 실리는 식별자.
    fn name(&self) -> &'static str;
    /// (GPU 사용률 %, 사용 중 GPU 메모리 GB).
    async fn read(&self) -> (f32, f64);
}

struct AppleMetalBackend;

impl GpuTelemetryBackend for AppleMetalBackend {
    fn name(&self) -> &'static str {
        "apple_metal"
    }

    async fn read(&self) -> (f32, f64) {
        get_metal_gpu_metrics().await
    }
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

    let gpu_backend = AppleMetalBackend;
    let (gpu_usage_percentage, gpu_memory_used_gb) = gpu_backend.read().await;

    Ok(SystemMetrics {
        total_memory_gb: (total * 100.0).round() / 100.0,
        used_memory_gb: (used * 100.0).round() / 100.0,
        memory_usage_percentage: ((used / total * 100.0) as f32 * 10.0).round() / 10.0,
        cpu_usage_percentage,
        gpu_usage_percentage,
        gpu_memory_used_gb,
        thermal_state: read_thermal_state(),
        gpu_backend: gpu_backend.name().to_string(),
    })
}

/// 실측 MLX matmul 벤치마크 결과(이슈 #11 축소 스코프). `GpuTelemetryBackend`는 순간
/// 사용률/메모리를 폴링하는 인터페이스라 여기 맞지 않는다 — 이건 한 번 돌려 처리량을 재는
/// 별개 동작이라 억지로 그 trait을 구현하지 않는다.
#[derive(Clone, Serialize, Debug)]
pub struct GpuBenchmarkResult {
    pub gflops: f64,
    pub matrix_dim: u32,
    pub iterations: u32,
    /// 파이썬 프로세스 내부에서 `mx.eval()` 강제 동기화 이후 측정한 시간(초).
    pub python_elapsed_seconds: f64,
    /// Rust 쪽에서 스폰~종료까지 `Instant`로 별도 측정한 wall-clock 시간(초). 인터프리터
    /// 기동 비용 등 파이썬 자체 시간에 안 잡히는 오버헤드가 섞이므로, 어느 한쪽 시간만
    /// 믿지 않고 둘 다 기록해 대조 가능하게 한다.
    pub rust_elapsed_seconds: f64,
}

/// `gpu_benchmark.py`가 stdout에 내는 JSON 한 줄의 스키마. Rust 쪽 결과 구조체와 필드가
/// 겹치지만 분리해 둔다 — 파이썬 출력 형식이 바뀌어도 이 구조체만 갱신하면 되고, 파싱
/// 실패가 어떤 필드 때문인지 컴파일러가 알려준다.
#[derive(Deserialize)]
struct GpuBenchmarkPythonOutput {
    gflops: f64,
    elapsed_seconds: f64,
    matrix_dim: u32,
    iterations: u32,
}

const GPU_BENCHMARK_TIMEOUT: Duration = Duration::from_secs(30);

/// 스폰·실행·파싱을 전담하는 AppHandle-비의존 내부 함수 — `check_mlx_env_inner`와 같은
/// 분리 이유다: 유닛 테스트가 `tauri::AppHandle` 없이 실제 venv/스크립트로 이 경로를
/// 직접 검증할 수 있게 한다.
async fn run_gpu_benchmark_inner(
    venv_py: &std::path::Path,
    script: &std::path::Path,
) -> Result<GpuBenchmarkResult, String> {
    if !venv_py.is_file() {
        return Err("MLX venv does not exist. Run setup_mlx_env first.".into());
    }
    if !script.is_file() {
        return Err(format!(
            "Could not find the GPU benchmark script: {}",
            script.display()
        ));
    }

    let mut cmd = tokio::process::Command::new(venv_py);
    cmd.arg(script).env("PATH", augmented_path());

    let rust_start = Instant::now();
    let output = tokio::time::timeout(GPU_BENCHMARK_TIMEOUT, cmd.output())
        .await
        .map_err(|_| {
            format!(
                "GPU benchmark timed out after {}s — the environment may be broken (no fake result returned)",
                GPU_BENCHMARK_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| format!("Failed to launch GPU benchmark script: {e}"))?;
    let rust_elapsed_seconds = rust_start.elapsed().as_secs_f64();

    if !output.status.success() {
        return Err(format!(
            "GPU benchmark script exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .next_back()
        .ok_or_else(|| "GPU benchmark script produced no output".to_string())?;
    let parsed: GpuBenchmarkPythonOutput = serde_json::from_str(line)
        .map_err(|e| format!("Failed to parse GPU benchmark output ({line}): {e}"))?;

    Ok(GpuBenchmarkResult {
        gflops: parsed.gflops,
        matrix_dim: parsed.matrix_dim,
        iterations: parsed.iterations,
        python_elapsed_seconds: parsed.elapsed_seconds,
        rust_elapsed_seconds,
    })
}

fn gpu_benchmark_script_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    Ok(resolve_bundled_resource(&resource_dir, "scripts/mlx/gpu_benchmark.py"))
}

/// 실측 GPU matmul 벤치마크(이슈 #11 축소 스코프). 실행 실패·타임아웃·파싱 불가 시 반드시
/// `Err`를 반환한다 — GFLOPS 0이나 가짜 구조체로 폴백하지 않는다(D22, mistakes-log의
/// 조작된 메트릭 사례들과 같은 실수를 반복하지 않기 위함).
#[tauri::command]
pub async fn run_gpu_benchmark(app: tauri::AppHandle) -> Result<GpuBenchmarkResult, String> {
    let venv_py = crate::commands::mlx::venv_python()?;
    let script = gpu_benchmark_script_path(&app)?;
    run_gpu_benchmark_inner(&venv_py, &script).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 테스트 cwd는 `src-tauri/` — 저장소 루트는 그 상위다(colima.rs의 `repo_k8s_dir`과
    /// 같은 관례).
    fn repo_gpu_benchmark_script() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .join("scripts/mlx/gpu_benchmark.py")
    }

    /// 실제 venv의 실제 파이썬으로 실제 MLX matmul을 돌려 실측 GFLOPS를 얻는다. CI 러너에는
    /// `~/.kubemetal/venv`가 없으므로(이 저장소 관례상 CI가 venv를 갖췄다고 가정하지 않는다)
    /// `#[ignore]`로 표시한다 — 로컬에서 `cargo test -- --ignored`로 실행해 실제 하드웨어
    /// 값을 확인한다.
    #[tokio::test]
    #[ignore = "실제 MLX venv(~/.kubemetal/venv)가 있는 Apple Silicon 기기에서만 실행 가능 — CI 러너엔 venv가 없다"]
    async fn run_gpu_benchmark_inner_produces_real_measurement() {
        let venv_py = crate::commands::mlx::venv_python().expect("HOME must resolve");
        let script = repo_gpu_benchmark_script();

        let result = run_gpu_benchmark_inner(&venv_py, &script)
            .await
            .expect("benchmark should succeed on a machine with a working MLX venv");

        assert_eq!(result.matrix_dim, 2048);
        assert_eq!(result.iterations, 20);
        assert!(result.gflops > 0.0, "GFLOPS must be a real positive measurement");
        assert!(result.python_elapsed_seconds > 0.0);
        assert!(result.rust_elapsed_seconds >= result.python_elapsed_seconds);
    }

    /// 존재하지 않는 스크립트 경로는 스폰조차 시도하지 않고 즉시 Err여야 한다 — 가짜
    /// 결과로 폴백하면 안 된다(D22).
    #[tokio::test]
    async fn run_gpu_benchmark_inner_errors_on_missing_script() {
        let venv_py = crate::commands::mlx::venv_python().expect("HOME must resolve");
        let missing = PathBuf::from("/nonexistent/gpu_benchmark.py");

        let result = run_gpu_benchmark_inner(&venv_py, &missing).await;
        assert!(result.is_err(), "missing script must error, not fabricate a result");
    }

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

    /// 실제 `ioreg`를 다시 읽는다. 픽스처는 "이 형태를 이렇게 읽는다"만 보장하고, 이 기기의
    /// 실제 출력이 여전히 그 형태인지는 별개 사실이다 — 틀어지면 화면이 0을 조용히 띄운다.
    ///
    /// 단정하는 것은 **"출력에 있는 필드는 반드시 읽힌다"** 이지 "두 필드가 다 있다"가 아니다.
    /// 후자로 적었다가 CI가 9커밋 동안 빨갛게 죽어 있었다(2026-08-20~21): GitHub macOS
    /// 러너의 가상화 가속기는 `In use system memory`(약 47MB)는 노출하지만 `Device
    /// Utilization %`는 내보내지 않는다. 하드웨어가 무엇을 노출하는지는 이 파서가 보장할 수
    /// 있는 사실이 아니고, 노출된 것을 놓치지 않는 것이 보장할 수 있는 사실이다.
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

        for (key, parsed) in [
            ("Device Utilization %", pct.is_some()),
            ("In use system memory", mem.is_some()),
        ] {
            if text.contains(&format!("{key}\"=")) {
                assert!(
                    parsed,
                    "ioreg 출력에 `{key}\"=`가 있는데 파서가 읽지 못했다 — 형식이 바뀌었다면 \
                     픽스처와 파서를 함께 갱신해야 한다"
                );
            }
        }

        // 두 필드가 모두 없으면 IOAccelerator를 통째로 못 본 것이다 — 그건 파싱 실패와
        // 구분되어야 하므로 여기서 잡는다(가상화 러너도 메모리 한 필드는 내보낸다).
        assert!(
            pct.is_some() || mem.is_some(),
            "ioreg에서 GPU 필드를 하나도 찾지 못했다 (출력 {}바이트) — IOAccelerator 자체가 \
             안 보이는 환경이거나 형식이 바뀌었다",
            out.stdout.len()
        );
    }

    /// 파서가 아니라 **수집 경로 전체**를 본다 — `external_command("ioreg")` 해석,
    /// 실행, 종료코드, 파싱까지. `live_ioreg_output_is_still_parseable`은 절대경로로
    /// ioreg를 직접 부르므로 이 층을 건너뛴다. GUI 번들 앱이 로그인 셸 PATH를 상속하지
    /// 않아 ioreg를 못 찾고 (0.0, 0.0)을 돌려주던 전례가 있다(D5/mistakes-log 2026-07-20).
    ///
    /// 단정은 **두 경로의 일치**다 — 하드웨어가 얼마를 보고하는지가 아니라. 절대값을 걸면
    /// 기기마다 다른 값에 테스트가 매이고(러너의 가상 가속기는 약 47MB, 이 M4 Pro는 약 1GB),
    /// 정작 검사하려는 것(경로 해석이 되는가)과 무관해진다.
    #[tokio::test]
    async fn metal_gpu_metrics_read_through_external_command() {
        let (pct, mem_gb) = get_metal_gpu_metrics().await;

        // 같은 순간을 두 번 읽을 수는 없으므로(사용률은 매 순간 변한다) 필드의 존재 여부만
        // 맞춘다 — external_command 경로가 깨지면 값이 아니라 **필드가 통째로** 사라진다.
        let direct = std::process::Command::new("/usr/sbin/ioreg")
            .args(["-l", "-d", "1", "-r", "-c", "IOAccelerator"])
            .output();
        let Ok(direct) = direct else { return };
        let text = String::from_utf8_lossy(&direct.stdout);
        let (direct_pct, direct_mem) = parse_ioreg_accelerator(&text);

        if direct_mem.is_some() {
            assert!(
                mem_gb > 0.0,
                "절대경로 ioreg는 메모리를 읽는데 external_command 경로는 0을 돌려줬다 — \
                 경로 해석이나 스폰이 깨졌다(D5/mistakes-log 2026-07-20의 증상)"
            );
        }
        if direct_pct.is_none() {
            // 이 환경은 사용률을 아예 노출하지 않는다(가상화 러너). 수집 경로도 같아야 한다.
            assert_eq!(
                pct, 0.0,
                "이 환경의 ioreg는 사용률을 내보내지 않는데 수집 경로가 값을 만들어냈다"
            );
        }
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

    /// 손으로 구성한 픽스처(`ioreg-ioaccelerator-util-only-hand-built.txt`) — 실측 캡처가
    /// 아니다. 2026-08-20~21에 실제로 관측된 CI 러너 패턴은 정반대였다: `In use system
    /// memory`는 노출하지만 `Device Utilization %`는 없었다(mistakes-log 2026-08-22). 이
    /// 테스트는 그 역방향 — 사용률은 있고 메모리가 없는 가상 환경에서도 파서가 있는 필드만
    /// 정확히 읽고 없는 필드는 None으로 남기는지 확인한다(D22, 지어낸 값 금지).
    #[test]
    fn hand_built_ci_pattern_reversed_utilization_present_memory_absent() {
        const FIXTURE: &str =
            include_str!("../../tests/fixtures/ioreg-ioaccelerator-util-only-hand-built.txt");
        let (pct, mem) = parse_ioreg_accelerator(FIXTURE);
        assert_eq!(pct, Some(12.0), "존재하는 Device Utilization %를 읽지 못했다");
        assert_eq!(
            mem, None,
            "In use system memory가 없는데 값을 만들어냈다 — 0으로 뭉개면 안 된다(D22)"
        );
    }

    /// 손으로 구성한 raw 값 — 실제 기기에서 관측된 값이 아니다. 애플이 아직 정의하지 않았거나
    /// 이 objc2 바인딩이 모르는 `NSProcessInfoThermalState` raw 값이 들어와도 이름을 지어내지
    /// 않고 None으로 남아야 한다(D22). `NSProcessInfoThermalState`는 `pub NSInteger` 튜플이라
    /// objc 호출 없이 임의 값을 직접 구성해 테스트할 수 있다.
    #[test]
    fn thermal_state_name_returns_none_for_unmapped_raw_value() {
        let unmapped = objc2_foundation::NSProcessInfoThermalState(99);
        assert_eq!(
            thermal_state_name(unmapped),
            None,
            "정의되지 않은 thermal raw 값에 이름을 지어냈다"
        );
    }

    /// 손으로 구성한 값 — 두 필드가 모두 존재하고 값이 0인 경우. "필드가 없음"과 "필드 값이
    /// 0"을 구분하는 기존 로직(D22)이 값 0에서도 여전히 Some(0.0)을 내는지 확인한다 —
    /// missing_or_malformed_fields_are_none_not_zero가 "없으면 None"을 보장한다면, 이
    /// 테스트는 그 반대쪽 "있으면 0이어도 Some"을 보장한다.
    #[test]
    fn hand_built_both_present_but_zero_are_not_confused_with_missing() {
        let line = r#""Device Utilization %"=0,"In use system memory"=0"#;
        let (pct, mem) = parse_ioreg_accelerator(line);
        assert_eq!(
            (pct, mem),
            (Some(0.0), Some(0.0)),
            "값이 0인 필드를 필드 부재(None)와 혼동했다"
        );
    }
}
