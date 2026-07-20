use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::Manager;

use crate::commands::mlx::MlxState;
use crate::services::process::resolve_cli_path;

#[derive(Debug, Clone, Serialize)]
pub struct GuardrailStatus {
    pub memory_pressure_level: String, // "normal" | "warn" | "critical" | "unknown"
    pub on_battery: bool,
    pub battery_pause_enabled: bool,
    pub training_paused: bool,
    pub caffeinate_active: bool,
}

#[derive(Default)]
pub struct GuardrailState {
    pub battery_pause_enabled: Mutex<bool>,
    pub caffeinate_active: Mutex<bool>,
}

/// `sysctl -n kern.memorystatus_vm_pressure_level` 원시 출력을 정규화한다(D16).
/// 실기기 확인(2026-07-21, 일반 사용자 권한, sudo 불필요): 1=normal, 2=warn, 4=critical.
fn map_pressure_level(raw: &str) -> &'static str {
    match raw.trim() {
        "1" => "normal",
        "2" => "warn",
        "4" => "critical",
        _ => "unknown",
    }
}

/// `pmset -g batt` 출력에서 배터리 구동 여부를 판정한다(D16).
/// 실기기 확인(2026-07-21, sudo 불필요): AC 전원은 "Now drawing from 'AC Power'",
/// 배터리 구동은 "Now drawing from 'Battery Power'" 문자열을 포함한다.
fn parse_on_battery(text: &str) -> bool {
    text.contains("Battery Power")
}

async fn measure_memory_pressure_level() -> String {
    let sysctl = match resolve_cli_path("sysctl") {
        Ok(p) => p,
        Err(_) => return "unknown".into(),
    };
    let output = tokio::process::Command::new(&sysctl)
        .args(["-n", "kern.memorystatus_vm_pressure_level"])
        .output()
        .await;
    match output {
        Ok(out) if out.status.success() => {
            map_pressure_level(&String::from_utf8_lossy(&out.stdout)).to_string()
        }
        _ => "unknown".into(),
    }
}

async fn measure_on_battery() -> bool {
    let pmset = match resolve_cli_path("pmset") {
        Ok(p) => p,
        Err(_) => return false,
    };
    let output = tokio::process::Command::new(&pmset)
        .args(["-g", "batt"])
        .output()
        .await;
    match output {
        Ok(out) if out.status.success() => {
            parse_on_battery(&String::from_utf8_lossy(&out.stdout))
        }
        _ => false,
    }
}

#[tauri::command]
pub async fn get_guardrail_status(app: tauri::AppHandle) -> Result<GuardrailStatus, String> {
    let memory_pressure_level = measure_memory_pressure_level().await;
    let on_battery = measure_on_battery().await;

    let guardrail_state = app.state::<GuardrailState>();
    let battery_pause_enabled = *guardrail_state
        .battery_pause_enabled
        .lock()
        .map_err(|e| e.to_string())?;
    let caffeinate_active = *guardrail_state
        .caffeinate_active
        .lock()
        .map_err(|e| e.to_string())?;

    let mlx_state = app.state::<MlxState>();
    let training_paused = {
        let guard = mlx_state.training.lock().map_err(|e| e.to_string())?;
        guard
            .as_ref()
            .map(|t| t.status.starts_with("paused"))
            .unwrap_or(false)
    };

    Ok(GuardrailStatus {
        memory_pressure_level,
        on_battery,
        battery_pause_enabled,
        training_paused,
        caffeinate_active,
    })
}

#[tauri::command]
pub async fn set_guardrail_config(
    app: tauri::AppHandle,
    battery_pause: bool,
) -> Result<(), String> {
    let guardrail_state = app.state::<GuardrailState>();
    let mut guard = guardrail_state
        .battery_pause_enabled
        .lock()
        .map_err(|e| e.to_string())?;
    *guard = battery_pause;
    Ok(())
}

/// 학습 pid에 시그널을 보낸다. `run_mlx_finetune`이 래퍼를 `.process_group(0)`으로 기동해
/// 자신이 속한 새 프로세스 그룹의 리더로 만들므로, 여기서도 그룹 전체(-pid)로 보내야
/// 래퍼가 내부에서 띄운 `mlx_lm` 학습 자식까지 함께 멈춘다/재개된다(D17). 단일 pid로만
/// 보내면 래퍼만 멈추고 실제 GPU 연산을 하는 자식은 학습을 계속 진행해버려 가드레일이
/// 무력화됨을 실기기에서 확인했다(2026-07-21).
async fn signal_pid(pid: u32, sig: i32) -> Result<(), String> {
    tokio::task::spawn_blocking(move || unsafe {
        libc::kill(-(pid as i32), sig);
    })
    .await
    .map_err(|e| format!("시그널 전송 실패: {e}"))
}

async fn pause_pid(app: &tauri::AppHandle, pid: u32, status: &str) -> Result<(), String> {
    signal_pid(pid, libc::SIGSTOP).await?;
    let mlx_state = app.state::<MlxState>();
    let mut guard = mlx_state.training.lock().map_err(|e| e.to_string())?;
    if let Some(t) = guard.as_mut() {
        if t.pid == pid {
            t.status = status.to_string();
        }
    }
    Ok(())
}

async fn resume_pid(app: &tauri::AppHandle, pid: u32) -> Result<(), String> {
    signal_pid(pid, libc::SIGCONT).await?;
    let mlx_state = app.state::<MlxState>();
    let mut guard = mlx_state.training.lock().map_err(|e| e.to_string())?;
    if let Some(t) = guard.as_mut() {
        if t.pid == pid {
            t.status = "running".to_string();
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn pause_mlx_training(app: tauri::AppHandle) -> Result<bool, String> {
    let pid = {
        let mlx_state = app.state::<MlxState>();
        let guard = mlx_state.training.lock().map_err(|e| e.to_string())?;
        match guard.as_ref() {
            Some(t) if t.status == "running" => t.pid,
            _ => return Err("일시정지할 수 있는 진행 중인 학습이 없습니다.".into()),
        }
    };
    pause_pid(&app, pid, "paused").await?;
    Ok(true)
}

#[tauri::command]
pub async fn resume_mlx_training(app: tauri::AppHandle) -> Result<bool, String> {
    let pid = {
        let mlx_state = app.state::<MlxState>();
        let guard = mlx_state.training.lock().map_err(|e| e.to_string())?;
        match guard.as_ref() {
            Some(t) if t.status.starts_with("paused") => t.pid,
            _ => return Err("재개할 수 있는 일시정지된 학습이 없습니다.".into()),
        }
    };
    resume_pid(&app, pid).await?;
    Ok(true)
}

/// 학습 시작 시 `caffeinate -dims -w <pid>`를 기동해 슬립 진입을 방지한다(FR-05.3).
/// `-w <pid>`는 대상 프로세스가 종료되면 caffeinate 자신도 함께 종료되므로 별도 kill이 불필요하다.
pub fn start_caffeinate(app: &tauri::AppHandle, pid: u32) {
    let caffeinate = match resolve_cli_path("caffeinate") {
        Ok(p) => p,
        Err(_) => return, // caffeinate 없음 — 가드레일은 best-effort이므로 조용히 skip
    };
    let mut child = match tokio::process::Command::new(&caffeinate)
        .args(["-dims", "-w", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return,
    };

    if let Ok(mut guard) = app.state::<GuardrailState>().caffeinate_active.lock() {
        *guard = true;
    }

    let app = app.clone();
    tokio::spawn(async move {
        let _ = child.wait().await;
        if let Ok(mut guard) = app.state::<GuardrailState>().caffeinate_active.lock() {
            *guard = false;
        }
    });
}

/// 학습 시작 시 훅되는 5초 주기 가드레일 루프(FR-05.2/05.3).
/// memory pressure가 warn/critical이면 자동 SIGSTOP(paused_memory_pressure), 배터리 일시정지가
/// 켜져 있고 배터리 구동 중이면 자동 SIGSTOP(paused_battery)한다. 재개는 사용자 조작
/// (`resume_mlx_training`)에 맡기며, 학습이 done/error/killed로 종료되거나 학습 항목이
/// 교체되면 루프가 스스로 종료된다.
pub fn spawn_guardrail_loop(app: tauri::AppHandle, pid: u32) {
    tokio::spawn(guardrail_loop(app, pid));
}

async fn guardrail_loop(app: tauri::AppHandle, pid: u32) {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    interval.tick().await; // 첫 tick은 즉시 발생 — 소비만 하고 그 다음 tick부터 5초 간격 검사

    loop {
        interval.tick().await;

        let mlx_state = app.state::<MlxState>();
        let status = {
            let guard = match mlx_state.training.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            match guard.as_ref() {
                Some(t) if t.pid == pid => t.status.clone(),
                _ => return, // 학습 항목이 교체/삭제됨 -> 루프 종료
            }
        };

        if status == "done" || status == "error" || status == "killed" {
            return;
        }
        if status != "running" {
            continue; // 이미 일시정지 상태(수동 포함) — 재개는 사용자 조작에 맡긴다
        }

        let level = measure_memory_pressure_level().await;
        if level == "warn" || level == "critical" {
            let _ = pause_pid(&app, pid, "paused_memory_pressure").await;
            continue;
        }

        let battery_pause_enabled = {
            match app.state::<GuardrailState>().battery_pause_enabled.lock() {
                Ok(g) => *g,
                Err(_) => false,
            }
        };
        if battery_pause_enabled && measure_on_battery().await {
            let _ = pause_pid(&app, pid, "paused_battery").await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_pressure_level_translates_known_codes() {
        assert_eq!(map_pressure_level("1"), "normal");
        assert_eq!(map_pressure_level("2\n"), "warn");
        assert_eq!(map_pressure_level("4"), "critical");
        assert_eq!(map_pressure_level("99"), "unknown");
        assert_eq!(map_pressure_level(""), "unknown");
    }

    #[test]
    fn parse_on_battery_detects_power_source() {
        assert!(!parse_on_battery("Now drawing from 'AC Power'\n"));
        assert!(parse_on_battery(
            "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=...)\t87%; discharging"
        ));
    }
}
