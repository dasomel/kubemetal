use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::services::process::{
    augmented_path, external_command, resolve_bundled_resource, resolve_cli_path,
};

#[derive(Debug, Clone, Serialize, Default)]
pub struct MlxEnvStatus {
    pub python_ok: bool,
    pub venv_exists: bool,
    pub mlx_lm_installed: bool,
    pub mlx_lm_version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnvSetupStatus {
    pub state: String, // "idle" | "installing" | "done" | "error"
    pub error: Option<String>,
}

impl Default for EnvSetupStatus {
    fn default() -> Self {
        Self {
            state: "idle".into(),
            error: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FineTuneConfig {
    pub model_path: String,
    pub data_path: String,
    pub iters: u32,
    pub batch_size: u32,
    pub learning_rate: f64,
    pub adapter_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrainingStatus {
    pub pid: u32,
    pub status: String, // "running" | "done" | "error" | "killed"
    pub current_iter: u32,
    pub total_iters: u32,
    pub last_loss: Option<f64>,
    pub adapter_path: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServingStatus {
    pub pid: u32,
    pub port: u16,
    pub model_path: String,
    pub adapter_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MlxStatus {
    pub env: MlxEnvStatus,
    pub env_setup: EnvSetupStatus,
    pub training: Option<TrainingStatus>,
    pub serving: Option<ServingStatus>,
    pub last_serving_error: Option<String>,
}

#[derive(Default)]
pub struct MlxState {
    pub env_setup: Mutex<EnvSetupStatus>,
    pub training: Mutex<Option<TrainingStatus>>,
    pub serving: Mutex<Option<ServingStatus>>,
    pub last_serving_error: Mutex<Option<String>>,
}

fn home_dir() -> Result<PathBuf, String> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| "HOME 환경변수를 찾을 수 없습니다.".to_string())
}

fn venv_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".kubemetal").join("venv"))
}

fn venv_python() -> Result<PathBuf, String> {
    Ok(venv_dir()?.join("bin").join("python"))
}

fn venv_pip() -> Result<PathBuf, String> {
    Ok(venv_dir()?.join("bin").join("pip"))
}

/// `model_path`/`data_path`처럼 프론트에서 넘어온 절대경로 문자열을 검증한다.
/// `canonicalize()`가 존재 검증과 `..`/심볼릭 링크 정규화를 동시에 수행하므로,
/// 정규화된 경로가 홈 디렉터리 하위인지만 재확인하면 된다(safe 원칙).
/// 선행 `~`는 셸이 아닌 앱 입력이라 확장되지 않은 채 도달하므로 여기서 HOME으로 치환한다.
fn validate_home_subpath(p: &str) -> Result<PathBuf, String> {
    let home = home_dir()?
        .canonicalize()
        .map_err(|e| format!("HOME 경로 확인 실패: {e}"))?;
    let expanded: PathBuf = if p == "~" {
        home.clone()
    } else if let Some(rest) = p.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(p)
    };
    let canonical = expanded
        .canonicalize()
        .map_err(|e| format!("경로를 찾을 수 없습니다: {p} ({e})"))?;
    if !canonical.starts_with(&home) {
        return Err(format!("허용되지 않은 경로입니다(홈 디렉터리 하위만 허용): {p}"));
    }
    Ok(canonical)
}

#[derive(Debug, Deserialize)]
struct AdapterConfigFile {
    model: Option<String>,
}

/// `adapter_dir`이 `adapter_config.json`을 담은 어댑터 디렉터리인지 판정하고,
/// 있다면 학습 시 사용된 베이스 모델 경로(`model` 필드)를 읽어 반환한다.
/// 실물 확인(2026-07-21): `mlx_lm.lora` 학습이 남기는 `adapter_config.json`에
/// 베이스 모델 절대경로가 최상위 `model` 필드로 그대로 저장되어 있다.
fn read_adapter_base_model(adapter_dir: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(adapter_dir.join("adapter_config.json")).ok()?;
    let parsed: AdapterConfigFile = serde_json::from_str(&content).ok()?;
    parsed.model
}

fn validate_adapter_name(name: &str) -> Result<(), String> {
    let is_valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && name.chars().any(|c| c != '.');
    if is_valid {
        Ok(())
    } else {
        Err(format!("잘못된 adapter_name입니다: {name}"))
    }
}

fn wrapper_script_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    Ok(resolve_bundled_resource(&resource_dir, "scripts/mlx/finetune_wrapper.py"))
}

async fn check_mlx_env_inner() -> MlxEnvStatus {
    let python_ok = resolve_cli_path("python3").is_ok();
    let venv_py = match venv_python() {
        Ok(p) => p,
        Err(_) => {
            return MlxEnvStatus {
                python_ok,
                venv_exists: false,
                mlx_lm_installed: false,
                mlx_lm_version: None,
            }
        }
    };
    let venv_exists = venv_py.is_file();
    if !venv_exists {
        return MlxEnvStatus {
            python_ok,
            venv_exists,
            mlx_lm_installed: false,
            mlx_lm_version: None,
        };
    }

    let output = tokio::process::Command::new(&venv_py)
        .args([
            "-c",
            "import mlx_lm, importlib.metadata; print(importlib.metadata.version('mlx-lm'))",
        ])
        .env("PATH", augmented_path())
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => {
            let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
            MlxEnvStatus {
                python_ok,
                venv_exists,
                mlx_lm_installed: true,
                mlx_lm_version: Some(version),
            }
        }
        _ => MlxEnvStatus {
            python_ok,
            venv_exists,
            mlx_lm_installed: false,
            mlx_lm_version: None,
        },
    }
}

#[tauri::command]
pub async fn check_mlx_env() -> Result<MlxEnvStatus, String> {
    Ok(check_mlx_env_inner().await)
}

async fn run_setup_inner() -> Result<(), String> {
    let venv = venv_dir()?;
    if !venv.join("bin").join("python").is_file() {
        let out = external_command("python3")?
            .arg("-m")
            .arg("venv")
            .arg(&venv)
            .output()
            .await
            .map_err(|e| format!("venv 생성 실행 실패: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "venv 생성 실패: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }

    let pip = venv_pip()?;
    let out = tokio::process::Command::new(&pip)
        .args(["install", "-U", "mlx-lm"])
        .env("PATH", augmented_path())
        .output()
        .await
        .map_err(|e| format!("pip install 실행 실패: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "mlx-lm 설치 실패: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

async fn run_setup(app: tauri::AppHandle) {
    let result = run_setup_inner().await;
    let state = app.state::<MlxState>();
    let mut guard = match state.env_setup.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    match result {
        Ok(()) => {
            guard.state = "done".into();
            guard.error = None;
        }
        Err(e) => {
            guard.state = "error".into();
            guard.error = Some(e);
        }
    }
}

#[tauri::command]
pub async fn setup_mlx_env(
    app: tauri::AppHandle,
    state: State<'_, MlxState>,
) -> Result<String, String> {
    {
        let mut guard = state.env_setup.lock().map_err(|e| e.to_string())?;
        if guard.state == "installing" {
            return Err("MLX 환경 설치가 이미 진행 중입니다.".into());
        }
        guard.state = "installing".into();
        guard.error = None;
    }

    tokio::spawn(run_setup(app));
    Ok("MLX venv 설치를 시작했습니다.".into())
}

fn apply_training_event(app: &tauri::AppHandle, value: &serde_json::Value) {
    let state = app.state::<MlxState>();
    let mut guard = match state.training.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let training = match guard.as_mut() {
        Some(t) => t,
        None => return,
    };
    match value.get("type").and_then(|v| v.as_str()) {
        Some("progress") => {
            if let Some(i) = value.get("iter").and_then(|v| v.as_u64()) {
                training.current_iter = i as u32;
            }
            if let Some(l) = value.get("train_loss").and_then(|v| v.as_f64()) {
                training.last_loss = Some(l);
            }
        }
        Some("done") => {
            training.status = "done".into();
            if let Some(p) = value.get("adapter_path").and_then(|v| v.as_str()) {
                training.adapter_path = Some(p.to_string());
            }
            if let Some(l) = value.get("last_loss").and_then(|v| v.as_f64()) {
                training.last_loss = Some(l);
            }
        }
        Some("error") => {
            training.status = "error".into();
            if let Some(m) = value.get("message").and_then(|v| v.as_str()) {
                training.error = Some(m.to_string());
            }
        }
        Some("warning") => {
            // 경고는 상태를 바꾸지 않는다(예: MLflow 접근 실패) — 향후 로그 노출용으로만 무시하지 않고 수신.
        }
        _ => {}
    }
}

async fn read_stdout_lines(app: tauri::AppHandle, stdout: tokio::process::ChildStdout) {
    let mut lines = BufReader::new(stdout).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    apply_training_event(&app, &value);
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
}

async fn collect_stderr(stderr: tokio::process::ChildStderr) -> String {
    let mut lines = BufReader::new(stderr).lines();
    let mut buf = String::new();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                if buf.len() < 4000 {
                    buf.push_str(&line);
                    buf.push('\n');
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    buf
}

fn finalize_training(
    app: &tauri::AppHandle,
    exit: std::io::Result<std::process::ExitStatus>,
    stderr_text: String,
) {
    let state = app.state::<MlxState>();
    let mut guard = match state.training.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let training = match guard.as_mut() {
        Some(t) => t,
        None => return,
    };
    if training.status != "running" {
        return;
    }
    match exit {
        Ok(status) if status.success() => training.status = "done".into(),
        Ok(status) => {
            training.status = "error".into();
            training.error = Some(if stderr_text.trim().is_empty() {
                format!("학습 프로세스가 비정상 종료되었습니다({status})")
            } else {
                stderr_text.trim().to_string()
            });
        }
        Err(e) => {
            training.status = "error".into();
            training.error = Some(format!("프로세스 대기 실패: {e}"));
        }
    }
}

async fn run_training_reader(app: tauri::AppHandle, mut child: tokio::process::Child) {
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let stdout_task = stdout.map(|out| tokio::spawn(read_stdout_lines(app.clone(), out)));
    let stderr_task = stderr.map(|err| tokio::spawn(collect_stderr(err)));

    if let Some(t) = stdout_task {
        let _ = t.await;
    }
    let stderr_text = if let Some(t) = stderr_task {
        t.await.unwrap_or_default()
    } else {
        String::new()
    };

    let exit = child.wait().await;
    finalize_training(&app, exit, stderr_text);
}

#[tauri::command]
pub async fn run_mlx_finetune(
    app: tauri::AppHandle,
    state: State<'_, MlxState>,
    config: FineTuneConfig,
) -> Result<u32, String> {
    let prev_training = {
        let mut guard = state.training.lock().map_err(|e| e.to_string())?;
        if let Some(t) = guard.as_ref() {
            if t.status == "running" {
                return Err(format!("이미 학습이 진행 중입니다(PID {}).", t.pid));
            }
        }
        let prev = guard.clone();
        *guard = Some(TrainingStatus {
            pid: 0,
            status: "running".into(),
            current_iter: 0,
            total_iters: config.iters,
            last_loss: None,
            adapter_path: None,
            error: None,
        });
        prev
    };

    let res = (|| -> Result<(u32, tokio::process::Child), String> {
        if config.iters == 0 {
            return Err("iters는 1 이상이어야 합니다.".into());
        }
        if config.batch_size == 0 {
            return Err("batch_size는 1 이상이어야 합니다.".into());
        }
        if !(config.learning_rate.is_finite() && config.learning_rate > 0.0) {
            return Err("learning_rate는 0보다 큰 유한한 값이어야 합니다.".into());
        }
        validate_adapter_name(&config.adapter_name)?;

        let model_path = validate_home_subpath(&config.model_path)?;
        let data_path = validate_home_subpath(&config.data_path)?;

        let venv_py = venv_python()?;
        if !venv_py.is_file() {
            return Err("MLX venv가 없습니다. setup_mlx_env를 먼저 실행하세요.".into());
        }

        let wrapper = wrapper_script_path(&app)?;
        if !wrapper.is_file() {
            return Err(format!(
                "파인튜닝 래퍼 스크립트를 찾을 수 없습니다: {}",
                wrapper.display()
            ));
        }

        let child = tokio::process::Command::new(&venv_py)
            .arg(&wrapper)
            .arg("--model")
            .arg(&model_path)
            .arg("--data")
            .arg(&data_path)
            .arg("--iters")
            .arg(config.iters.to_string())
            .arg("--batch-size")
            .arg(config.batch_size.to_string())
            .arg("--learning-rate")
            .arg(config.learning_rate.to_string())
            .arg("--adapter-name")
            .arg(&config.adapter_name)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("PATH", augmented_path())
            .process_group(0)
            .spawn()
            .map_err(|e| format!("파인튜닝 프로세스 실행 실패: {e}"))?;

        let pid = child
            .id()
            .ok_or_else(|| "PID를 가져올 수 없습니다.".to_string())?;

        Ok((pid, child))
    })();

    match res {
        Ok((pid, child)) => {
            {
                let mut guard = state.training.lock().map_err(|e| e.to_string())?;
                *guard = Some(TrainingStatus {
                    pid,
                    status: "running".into(),
                    current_iter: 0,
                    total_iters: config.iters,
                    last_loss: None,
                    adapter_path: None,
                    error: None,
                });
            }

            crate::commands::guardrails::start_caffeinate(&app, pid);
            crate::commands::guardrails::spawn_guardrail_loop(app.clone(), pid);

            tokio::spawn(run_training_reader(app, child));

            Ok(pid)
        }
        Err(e) => {
            if let Ok(mut guard) = state.training.lock() {
                *guard = prev_training;
            }
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn get_mlx_status(state: State<'_, MlxState>) -> Result<MlxStatus, String> {
    let env = check_mlx_env_inner().await;
    let env_setup = state.env_setup.lock().map_err(|e| e.to_string())?.clone();
    let training = state.training.lock().map_err(|e| e.to_string())?.clone();
    let serving = state.serving.lock().map_err(|e| e.to_string())?.clone();
    let last_serving_error = state
        .last_serving_error
        .lock()
        .map_err(|e| e.to_string())?
        .clone();

    Ok(MlxStatus {
        env,
        env_setup,
        training,
        serving,
        last_serving_error,
    })
}

/// SIGTERM 전송 후 1초 대기, 여전히 살아있으면 SIGKILL. `libc::kill(pid, 0)`으로 생존 여부를 확인한다.
/// `use_process_group`이면 시그널을 `-pid`(프로세스 그룹)로 보낸다 — 학습 래퍼는
/// `.process_group(0)`으로 기동되어 자신이 그룹 리더이므로, 그룹으로 보내야 내부에서
/// `subprocess.Popen`으로 띄운 `mlx_lm` 학습 자식까지 함께 종료된다(D17). 서빙 프로세스는
/// 새 그룹 없이 앱과 그룹을 공유하므로 단일 pid로 보낸다.
fn terminate_pid(pid: u32, use_process_group: bool) {
    let target: i32 = if use_process_group {
        -(pid as i32)
    } else {
        pid as i32
    };
    unsafe {
        libc::kill(target, libc::SIGTERM);
    }
    std::thread::sleep(std::time::Duration::from_secs(1));
    let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
    if alive {
        unsafe {
            libc::kill(target, libc::SIGKILL);
        }
    }
}

#[tauri::command]
pub async fn kill_mlx_process(state: State<'_, MlxState>, pid: u32) -> Result<bool, String> {
    let is_training = {
        let guard = state.training.lock().map_err(|e| e.to_string())?;
        guard.as_ref().map(|t| t.pid == pid).unwrap_or(false)
    };

    tokio::task::spawn_blocking(move || terminate_pid(pid, is_training))
        .await
        .map_err(|e| format!("프로세스 종료 대기 실패: {e}"))?;

    {
        let mut guard = state.training.lock().map_err(|e| e.to_string())?;
        if let Some(t) = guard.as_mut() {
            // running/paused* 등 아직 종료되지 않은 상태였다면 killed로 전이한다(가드레일이
            // 일시정지시킨 상태에서 사용자가 중지를 눌러도 상태가 갱신되어야 한다).
            if t.pid == pid && t.status != "done" && t.status != "error" && t.status != "killed" {
                t.status = "killed".into();
            }
        }
    }

    {
        // 서빙 프로세스는 spawn 직후 run_serving_reader가 Child 소유권을 가져가 wait()한다.
        // 여기서는 상태만 즉시 비우면 되고, reaper는 pid가 더 이상 state.serving과 일치하지
        // 않는 것을 보고 사용자 의도 종료로 판단해 last_serving_error를 기록하지 않는다.
        let mut serving_guard = state.serving.lock().map_err(|e| e.to_string())?;
        if serving_guard.as_ref().map(|s| s.pid) == Some(pid) {
            *serving_guard = None;
        }
    }

    Ok(true)
}

/// 서빙 Child의 종료를 백그라운드에서 대기하고 상태를 정리한다.
/// spawn 직후 Child 소유권 전체가 이 태스크로 넘어오므로(다른 코드는 더 이상 wait()하지
/// 않는다), stop_model_serving/kill_mlx_process는 시그널만 보내고 state.serving을 즉시
/// 비운다. 그래서 여기서 pid가 더 이상 state.serving과 일치하지 않으면 "사용자 의도 종료"로
/// 판단해 조용히 반환하고, 일치하면(=아무도 멈추라고 하지 않았는데 죽었다) exit code와
/// 최근 stderr 요약을 last_serving_error에 남긴다.
async fn run_serving_reader(app: tauri::AppHandle, mut child: tokio::process::Child, pid: u32) {
    let stderr = child.stderr.take();
    let stderr_task = stderr.map(|err| tokio::spawn(collect_stderr(err)));

    let exit = child.wait().await;
    let stderr_text = if let Some(t) = stderr_task {
        t.await.unwrap_or_default()
    } else {
        String::new()
    };

    let state = app.state::<MlxState>();
    let still_current = {
        let serving_guard = match state.serving.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        serving_guard.as_ref().map(|s| s.pid) == Some(pid)
    };
    if !still_current {
        return;
    }
    if let Ok(mut serving_guard) = state.serving.lock() {
        *serving_guard = None;
    }

    let mut err_guard = match state.last_serving_error.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    match exit {
        Ok(status) if status.success() => {
            *err_guard = None;
        }
        Ok(status) => {
            *err_guard = Some(if stderr_text.trim().is_empty() {
                format!("서빙 프로세스가 예기치 않게 종료되었습니다({status})")
            } else {
                format!(
                    "서빙 프로세스가 예기치 않게 종료되었습니다({status}): {}",
                    stderr_text.trim()
                )
            });
        }
        Err(e) => {
            *err_guard = Some(format!("서빙 프로세스 대기 실패: {e}"));
        }
    }
}

#[tauri::command]
pub async fn start_model_serving(
    app: tauri::AppHandle,
    state: State<'_, MlxState>,
    model_path: String,
    adapter_path: Option<String>,
    port: u16,
) -> Result<String, String> {
    {
        let mut guard = state.serving.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            return Err("이미 모델 서빙이 진행 중입니다.".into());
        }
        *guard = Some(ServingStatus {
            pid: 0,
            port,
            model_path: model_path.clone(),
            adapter_path: adapter_path.clone(),
        });
    }

    let res = (|| -> Result<(u32, tokio::process::Child, String, Option<String>), String> {
        // 8080은 개발 환경에서 다른 서비스(예: Tomcat)가 선점하고 있는 경우가 흔하다.
        // bind 성공 시 리스너를 즉시 drop해 포트를 반납하고 그 사이에 실제 서빙 프로세스를 스폰한다.
        let listener = std::net::TcpListener::bind(("127.0.0.1", port)).map_err(|_| {
            format!("포트 {port}는 다른 프로세스가 사용 중입니다. 서빙 카드에서 다른 포트(예: 8081)를 지정하세요.")
        })?;
        drop(listener);

        let validated_model_dir = validate_home_subpath(&model_path)?;
        let is_adapter_dir = validated_model_dir.join("adapter_config.json").is_file();

        let (base_model, effective_adapter): (PathBuf, Option<PathBuf>) = if is_adapter_dir {
            let base = read_adapter_base_model(&validated_model_dir).ok_or_else(|| {
                "어댑터 디렉터리입니다 — 베이스 모델을 함께 지정하세요.".to_string()
            })?;
            let validated_base = validate_home_subpath(&base)?;
            (validated_base, Some(validated_model_dir.clone()))
        } else {
            let explicit_adapter = match adapter_path.as_deref() {
                Some(p) if !p.is_empty() => Some(validate_home_subpath(p)?),
                _ => None,
            };
            (validated_model_dir.clone(), explicit_adapter)
        };

        let venv_py = venv_python()?;
        if !venv_py.is_file() {
            return Err("MLX venv가 없습니다. setup_mlx_env를 먼저 실행하세요.".into());
        }

        let mut cmd = tokio::process::Command::new(&venv_py);
        cmd.args(["-m", "mlx_lm", "server", "--model"])
            .arg(&base_model)
            .args(["--port", &port.to_string()]);
        if let Some(ref adapter) = effective_adapter {
            cmd.arg("--adapter-path").arg(adapter);
        }

        let child = cmd
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .env("PATH", augmented_path())
            .spawn()
            .map_err(|e| format!("서빙 프로세스 실행 실패: {e}"))?;

        let pid = child
            .id()
            .ok_or_else(|| "PID를 가져올 수 없습니다.".to_string())?;

        Ok((
            pid,
            child,
            base_model.to_string_lossy().to_string(),
            effective_adapter.map(|p| p.to_string_lossy().to_string()),
        ))
    })();

    match res {
        Ok((pid, child, base_model_str, effective_adapter_str)) => {
            {
                let mut serving_guard = state.serving.lock().map_err(|e| e.to_string())?;
                *serving_guard = Some(ServingStatus {
                    pid,
                    port,
                    model_path: base_model_str,
                    adapter_path: effective_adapter_str.clone(),
                });
            }
            {
                let mut err_guard = state.last_serving_error.lock().map_err(|e| e.to_string())?;
                *err_guard = None;
            }

            tokio::spawn(run_serving_reader(app, child, pid));

            let adapter_note = effective_adapter_str
                .map(|p| format!(" · 어댑터 {p}"))
                .unwrap_or_default();
            Ok(format!(
                "{port} 포트에서 모델 서빙을 시작했습니다(PID {pid}){adapter_note}."
            ))
        }
        Err(e) => {
            if let Ok(mut guard) = state.serving.lock() {
                *guard = None;
            }
            Err(e)
        }
    }
}

/// `range` 내에서 로컬 바인드가 성공하는 첫 포트를 찾는다. bind 성공 시 리스너를 즉시
/// drop해 포트를 반납한다 — 실제 서빙 프로세스가 그 포트를 다시 bind할 때까지 사용 여부만
/// 확인하는 용도이므로 TOCTOU 경합 가능성은 있으나(D: 다른 프로세스가 그 사이 선점),
/// start_model_serving이 실제 spawn 직전에 다시 bind를 확인하므로 여기서는 "제안값"으로만 쓰인다.
fn find_available_port(range: std::ops::RangeInclusive<u16>) -> Result<u16, String> {
    for port in range {
        if let Ok(listener) = std::net::TcpListener::bind(("127.0.0.1", port)) {
            drop(listener);
            return Ok(port);
        }
    }
    Err("사용 가능한 포트를 찾지 못했습니다.".into())
}

#[tauri::command]
pub async fn suggest_serving_port() -> Result<u16, String> {
    find_available_port(8080..=8099)
}

#[tauri::command]
pub async fn stop_model_serving(state: State<'_, MlxState>) -> Result<String, String> {
    let pid = {
        let guard = state.serving.lock().map_err(|e| e.to_string())?;
        match guard.as_ref() {
            Some(s) => s.pid,
            None => return Err("진행 중인 모델 서빙이 없습니다.".into()),
        }
    };

    tokio::task::spawn_blocking(move || terminate_pid(pid, false))
        .await
        .map_err(|e| format!("프로세스 종료 대기 실패: {e}"))?;

    // Child 소유권은 run_serving_reader가 갖고 있으므로 여기서는 상태만 비운다.
    // reaper가 실제 종료를 감지하고 last_serving_error를 남기지 않는다(사용자 의도 종료).
    {
        let mut serving_guard = state.serving.lock().map_err(|e| e.to_string())?;
        if serving_guard.as_ref().map(|s| s.pid) == Some(pid) {
            *serving_guard = None;
        }
    }

    Ok("모델 서빙을 정지했습니다.".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn validate_home_subpath_expands_tilde() {
        // "~"와 "~/..."가 HOME 기준으로 확장되어 검증을 통과해야 한다.
        let home = super::validate_home_subpath("~").expect("~ 확장 실패");
        assert!(home.ends_with(std::env::var("HOME").unwrap().trim_start_matches('/')));
        // 존재가 보장되는 홈 하위 경로로 확장 검증 (~/. == 홈 자신)
        assert!(super::validate_home_subpath("~/.").is_ok());
    }

    use super::*;

    #[test]
    fn validate_adapter_name_accepts_simple_names() {
        assert!(validate_adapter_name("smoke-test").is_ok());
        assert!(validate_adapter_name("adapter_v1.2").is_ok());
    }

    #[test]
    fn validate_adapter_name_rejects_traversal_and_empty() {
        assert!(validate_adapter_name("").is_err());
        assert!(validate_adapter_name("..").is_err());
        assert!(validate_adapter_name("../evil").is_err());
        assert!(validate_adapter_name("a/b").is_err());
    }

    #[test]
    fn validate_home_subpath_rejects_outside_home() {
        assert!(validate_home_subpath("/etc/passwd").is_err());
    }

    #[test]
    fn validate_home_subpath_rejects_nonexistent() {
        assert!(validate_home_subpath("/definitely/not/here/xyz").is_err());
    }

    #[test]
    fn find_available_port_skips_occupied_port() {
        // 포트 0으로 바인드하면 OS가 빈 포트를 배정한다 — 실제 개발 환경에서 점유 중인
        // 8080/8081과 충돌하지 않으면서도 "점유된 포트를 건너뛰는지"를 검증할 수 있다.
        let occupied = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("포트 점유 실패");
        let occupied_port = occupied.local_addr().expect("주소 조회 실패").port();

        let result = find_available_port(occupied_port..=occupied_port.saturating_add(5))
            .expect("가용 포트를 찾지 못함");

        assert_ne!(result, occupied_port);
        drop(occupied);
    }
}
