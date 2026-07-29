use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::Manager;

use crate::commands::mlx::MlxState;
use crate::services::process::external_command;

#[derive(Debug, Clone, Serialize)]
pub struct GuardrailStatus {
    pub memory_pressure_level: String, // "normal" | "warn" | "critical" | "unknown"
    pub on_battery: bool,
    pub battery_pause_enabled: bool,
    pub training_paused: bool,
    pub caffeinate_active: bool,
    /// "nominal" | "fair" | "serious" | "critical", 읽기 실패 시 None(D22).
    /// 메모리 압력이 "RAM이 모자란가"를 말한다면 이쪽은 "지금 스로틀링되고 있는가"다 —
    /// 장시간 파인튜닝에서 실제로 처리량을 떨어뜨리는 신호다.
    pub thermal_state: Option<String>,
    pub thermal_pause_enabled: bool,
    pub resume_overrides: Vec<String>,
}

#[derive(Default)]
pub struct GuardrailState {
    pub battery_pause_enabled: Mutex<bool>,
    pub caffeinate_active: Mutex<bool>,
    /// 기본 off — 배터리 일시정지와 같은 취급이다. 발열은 정상적인 학습 부하에서도
    /// fair까지 흔히 올라가므로, 켤지 말지는 사용자가 정한다.
    pub thermal_pause_enabled: Mutex<bool>,
    /// D16 개정: 수동 재개 오버라이드 저장소. (학습 pid, 억제된 원인 집합).
    /// 수동 재개는 사용자 의사 표명이므로 같은 원인의 advisory 신호로 다시 멈추지 않는다, critical은 예외.
    pub resume_overrides: Mutex<Option<(u32, HashSet<String>)>>,
}

/// 학습을 멈춰야 할 발열 단계. `serious`부터다 — `fair`는 부하가 걸린 정상 상태에서도
/// 흔히 나타나서 여기서 멈추면 학습이 사실상 불가능해진다.
fn thermal_should_pause(state: Option<&str>) -> bool {
    matches!(state, Some("serious") | Some("critical"))
}

/// D16 개정: 수동 재개는 사용자 의사 표명이므로 같은 원인의 advisory 신호(warn)로 다시 멈추지 않는다, critical은 예외.
fn memory_should_auto_pause(level: &str, overridden: bool) -> bool {
    level == "critical" || (level == "warn" && !overridden)
}

fn battery_should_auto_pause(
    on_battery: bool,
    battery_pause_enabled: bool,
    overridden: bool,
) -> bool {
    battery_pause_enabled && on_battery && !overridden
}

fn thermal_should_auto_pause(
    thermal_state: Option<&str>,
    thermal_pause_enabled: bool,
    overridden: bool,
) -> bool {
    thermal_pause_enabled && thermal_should_pause(thermal_state) && !overridden
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
    let mut cmd = match external_command("sysctl") {
        Ok(c) => c,
        Err(_) => return "unknown".into(),
    };
    let output = cmd
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
    let mut cmd = match external_command("pmset") {
        Ok(c) => c,
        Err(_) => return false,
    };
    let output = cmd
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
    let thermal_pause_enabled = *guardrail_state
        .thermal_pause_enabled
        .lock()
        .map_err(|e| e.to_string())?;
    let thermal_state = crate::commands::metrics::read_thermal_state();

    let mlx_state = app.state::<MlxState>();
    let (training_paused, active_pid) = {
        let guard = mlx_state.training.lock().map_err(|e| e.to_string())?;
        let paused = guard
            .as_ref()
            .map(|t| t.status.starts_with("paused"))
            .unwrap_or(false);
        let pid = guard.as_ref().map(|t| t.pid);
        (paused, pid)
    };

    let resume_overrides = {
        let guard = guardrail_state
            .resume_overrides
            .lock()
            .map_err(|e| e.to_string())?;
        match (*guard).as_ref() {
            Some((pid, set)) if active_pid == Some(*pid) => {
                let mut list: Vec<String> = set.iter().cloned().collect();
                list.sort();
                list
            }
            _ => vec![],
        }
    };

    Ok(GuardrailStatus {
        memory_pressure_level,
        on_battery,
        battery_pause_enabled,
        training_paused,
        caffeinate_active,
        thermal_state,
        thermal_pause_enabled,
        resume_overrides,
    })
}

#[tauri::command]
pub async fn set_guardrail_config(
    app: tauri::AppHandle,
    battery_pause: bool,
    thermal_pause: Option<bool>,
) -> Result<(), String> {
    let guardrail_state = app.state::<GuardrailState>();
    {
        let mut guard = guardrail_state
            .battery_pause_enabled
            .lock()
            .map_err(|e| e.to_string())?;
        *guard = battery_pause;
    }
    // 호출자가 발열 항목을 안 보내면 기존 설정을 건드리지 않는다 — 프런트의 다른
    // 토글이 이 값을 실수로 꺼버리지 않게 하기 위해서다.
    if let Some(thermal_pause) = thermal_pause {
        let mut guard = guardrail_state
            .thermal_pause_enabled
            .lock()
            .map_err(|e| e.to_string())?;
        *guard = thermal_pause;
    }
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
    let (pid, status) = {
        let mlx_state = app.state::<MlxState>();
        let guard = mlx_state.training.lock().map_err(|e| e.to_string())?;
        match guard.as_ref() {
            Some(t) if t.status.starts_with("paused") => (t.pid, t.status.clone()),
            _ => return Err("재개할 수 있는 일시정지된 학습이 없습니다.".into()),
        }
    };

    // D16 개정: paused_<cause> 상태에서 수동 재개 시 해당 cause를 오버라이드 집합에 추가
    if let Some(cause) = status.strip_prefix("paused_") {
        if !cause.is_empty() {
            let guardrail_state = app.state::<GuardrailState>();
            let mut overrides_guard = guardrail_state
                .resume_overrides
                .lock()
                .map_err(|e| e.to_string())?;

            match overrides_guard.as_mut() {
                Some((stored_pid, set)) if *stored_pid == pid => {
                    set.insert(cause.to_string());
                }
                _ => {
                    let mut set = HashSet::new();
                    set.insert(cause.to_string());
                    *overrides_guard = Some((pid, set));
                }
            }
        }
    }

    resume_pid(&app, pid).await?;
    Ok(true)
}

/// 학습 시작 시 `caffeinate -dims -w <pid>`를 기동해 슬립 진입을 방지한다(FR-05.3).
/// `-w <pid>`는 대상 프로세스가 종료되면 caffeinate 자신도 함께 종료되므로 별도 kill이 불필요하다.
pub fn start_caffeinate(app: &tauri::AppHandle, pid: u32) {
    let mut cmd = match external_command("caffeinate") {
        Ok(c) => c,
        Err(_) => return, // caffeinate 없음 — 가드레일은 best-effort이므로 조용히 skip
    };
    let mut child = match cmd
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

/// 이 루프(pid)의 오버라이드만 지운다. 무조건 비우면 학습 교체 직후 구 루프의 마지막
/// 틱이 신규 학습의 오버라이드를 지울 수 있다(경합 — 재정지 1회로 자가 치유되지만
/// 사용자 의사를 한 번 무시하게 된다).
fn clear_resume_overrides_for_pid(app: &tauri::AppHandle, pid: u32) {
    if let Ok(mut guard) = app.state::<GuardrailState>().resume_overrides.lock() {
        if let Some((stored_pid, _)) = *guard {
            if stored_pid == pid {
                *guard = None;
            }
        }
    }
}

fn clear_resume_overrides_if_not_pid(app: &tauri::AppHandle, pid: u32) {
    if let Ok(mut guard) = app.state::<GuardrailState>().resume_overrides.lock() {
        if let Some((stored_pid, _)) = *guard {
            if stored_pid != pid {
                *guard = None;
            }
        }
    }
}

fn get_active_overrides(app: &tauri::AppHandle, pid: u32) -> HashSet<String> {
    if let Ok(guard) = app.state::<GuardrailState>().resume_overrides.lock() {
        if let Some((stored_pid, set)) = guard.as_ref() {
            if *stored_pid == pid {
                return set.clone();
            }
        }
    }
    HashSet::new()
}

async fn guardrail_loop(app: tauri::AppHandle, pid: u32) {
    clear_resume_overrides_if_not_pid(&app, pid);

    let mut interval = tokio::time::interval(Duration::from_secs(5));
    interval.tick().await; // 첫 tick은 즉시 발생 — 소비만 하고 그 다음 tick부터 5초 간격 검사

    loop {
        interval.tick().await;

        let mlx_state = app.state::<MlxState>();
        let status = {
            let guard = match mlx_state.training.lock() {
                Ok(g) => g,
                Err(_) => {
                    clear_resume_overrides_for_pid(&app, pid);
                    return;
                }
            };
            match guard.as_ref() {
                Some(t) if t.pid == pid => t.status.clone(),
                _ => {
                    clear_resume_overrides_for_pid(&app, pid);
                    return; // 학습 항목이 교체/삭제됨 -> 루프 종료
                }
            }
        };

        if status == "done" || status == "error" || status == "killed" {
            clear_resume_overrides_for_pid(&app, pid);
            return;
        }
        if status != "running" {
            continue; // 이미 일시정지 상태(수동 포함) — 재개는 사용자 조작에 맡긴다
        }

        let overrides = get_active_overrides(&app, pid);

        let level = measure_memory_pressure_level().await;
        let mem_override = overrides.contains("memory_pressure");
        if memory_should_auto_pause(&level, mem_override) {
            let _ = pause_pid(&app, pid, "paused_memory_pressure").await;
            continue;
        }

        let battery_pause_enabled = {
            match app.state::<GuardrailState>().battery_pause_enabled.lock() {
                Ok(g) => *g,
                Err(_) => false,
            }
        };
        let bat_override = overrides.contains("battery");
        if battery_should_auto_pause(
            measure_on_battery().await,
            battery_pause_enabled,
            bat_override,
        ) {
            let _ = pause_pid(&app, pid, "paused_battery").await;
            continue;
        }

        // 발열은 옵트인이다. 켜져 있어도 serious 이상에서만 멈춘다 — fair는 부하가
        // 걸린 정상 상태에서도 흔해서, 거기서 멈추면 학습이 사실상 불가능해진다.
        let thermal_pause_enabled = {
            match app.state::<GuardrailState>().thermal_pause_enabled.lock() {
                Ok(g) => *g,
                Err(_) => false,
            }
        };
        let th_override = overrides.contains("thermal");
        let thermal_st = crate::commands::metrics::read_thermal_state();
        if thermal_should_auto_pause(thermal_st.as_deref(), thermal_pause_enabled, th_override) {
            let _ = pause_pid(&app, pid, "paused_thermal").await;
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

    /// 임계선이 serious인 게 핵심이다 — fair에서 멈추면 정상 학습 부하에서도 계속
    /// 일시정지가 걸린다. 값을 못 읽었을 때(None) 멈추지 않는 것도 의도다: 알 수 없음을
    /// 위험으로 단정해 학습을 세우지 않는다.
    #[test]
    fn thermal_pauses_only_from_serious_up() {
        assert!(!thermal_should_pause(Some("nominal")));
        assert!(!thermal_should_pause(Some("fair")));
        assert!(thermal_should_pause(Some("serious")));
        assert!(thermal_should_pause(Some("critical")));
        assert!(!thermal_should_pause(None));
    }

    #[test]
    fn parse_on_battery_detects_power_source() {
        assert!(!parse_on_battery("Now drawing from 'AC Power'\n"));
        assert!(parse_on_battery(
            "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=...)\t87%; discharging"
        ));
    }

    #[test]
    fn memory_should_auto_pause_handles_critical_and_warn_overrides() {
        // critical은 오버라이드 여부와 무관하게 항상 pause
        assert!(memory_should_auto_pause("critical", false));
        assert!(memory_should_auto_pause("critical", true));

        // warn은 오버라이드가 없을 때만 pause
        assert!(memory_should_auto_pause("warn", false));
        assert!(!memory_should_auto_pause("warn", true));

        // normal/unknown은 항상 pause 안 함
        assert!(!memory_should_auto_pause("normal", false));
        assert!(!memory_should_auto_pause("normal", true));
        assert!(!memory_should_auto_pause("unknown", false));
    }

    #[test]
    fn battery_should_auto_pause_respects_override() {
        assert!(battery_should_auto_pause(true, true, false));
        assert!(!battery_should_auto_pause(true, true, true));
        assert!(!battery_should_auto_pause(false, true, false));
        assert!(!battery_should_auto_pause(true, false, false));
    }

    #[test]
    fn thermal_should_auto_pause_respects_override() {
        assert!(thermal_should_auto_pause(Some("serious"), true, false));
        assert!(thermal_should_auto_pause(Some("critical"), true, false));
        assert!(!thermal_should_auto_pause(Some("serious"), true, true));
        assert!(!thermal_should_auto_pause(Some("fair"), true, false));
        assert!(!thermal_should_auto_pause(Some("serious"), false, false));
    }
}

