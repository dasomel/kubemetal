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
    /// VLM 런타임(D29). mlx-vlm은 mlx-lm을 의존성으로 끌고 오므로(실측 0.6.7 → mlx-lm
    /// 0.31.3) 같은 venv에 공존한다 — 별도 venv를 만들지 않는다.
    pub mlx_vlm_installed: bool,
    pub mlx_vlm_version: Option<String>,
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
    /// 미지정이면 mlx-lm — 기존 호출의 동작이 바뀌지 않는다(D29).
    pub runtime: Option<MlxRuntime>,
    /// 미지정 false — 기존 호출 동작 불변. mlx-vlm 전용, 비양자화 모델 필요.
    #[serde(default)]
    pub train_vision: bool,
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

/// 서빙 런타임(D29). 둘 다 OpenAI 호환 HTTP 서버라 D10 브리지·kagent·평가(D20) 소비자는
/// 이 선택을 모른다 — 차이는 스폰 인자와 입력 모달리티(vlm은 이미지)뿐이다.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MlxRuntime {
    MlxLm,
    MlxVlm,
}

impl MlxRuntime {
    /// 스폰 인자. 실측(2026-07-27, mlx-vlm 0.6.7): `mlx_vlm.server`의 기본 host는
    /// **0.0.0.0**이다 — 명시하지 않으면 서빙이 LAN에 노출된다. 루프백을 강제한다.
    /// mlx_lm은 기본이 127.0.0.1이지만 같은 이유로 양쪽 다 명시한다.
    fn server_args(&self) -> &'static [&'static str] {
        match self {
            MlxRuntime::MlxLm => &["-m", "mlx_lm", "server", "--host", "127.0.0.1"],
            MlxRuntime::MlxVlm => &["-m", "mlx_vlm.server", "--host", "127.0.0.1"],
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ServingStatus {
    pub pid: u32,
    pub port: u16,
    pub model_path: String,
    pub adapter_path: Option<String>,
    pub runtime: MlxRuntime,
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

pub(crate) fn home_dir() -> Result<PathBuf, String> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| "Could not find HOME environment variable.".to_string())
}

pub(crate) fn venv_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".kubemetal").join("venv"))
}

/// prefect.rs 등 다른 커맨드 모듈도 동일한 앱 전용 venv(~/.kubemetal/venv)를 사용한다(D15) —
/// venv 경로를 분산시키지 않도록 mlx.rs를 단일 출처로 두고 pub(crate)로 재사용한다.
pub(crate) fn venv_python() -> Result<PathBuf, String> {
    Ok(venv_dir()?.join("bin").join("python"))
}

pub(crate) fn venv_pip() -> Result<PathBuf, String> {
    Ok(venv_dir()?.join("bin").join("pip"))
}

/// `model_path`/`data_path`처럼 프론트에서 넘어온 절대경로 문자열을 검증한다.
/// `canonicalize()`가 존재 검증과 `..`/심볼릭 링크 정규화를 동시에 수행하므로,
/// 정규화된 경로가 홈 디렉터리 하위인지만 재확인하면 된다(safe 원칙).
/// 선행 `~`는 셸이 아닌 앱 입력이라 확장되지 않은 채 도달하므로 여기서 HOME으로 치환한다.
pub(crate) fn validate_home_subpath(p: &str) -> Result<PathBuf, String> {
    let home = home_dir()?
        .canonicalize()
        .map_err(|e| format!("Failed to resolve HOME path: {e}"))?;
    let expanded: PathBuf = if p == "~" {
        home.clone()
    } else if let Some(rest) = p.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(p)
    };
    let canonical = expanded
        .canonicalize()
        .map_err(|e| format!("Path not found: {p} ({e})"))?;
    if !canonical.starts_with(&home) {
        return Err(format!("Path not allowed (only paths under the home directory are allowed): {p}"));
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

pub(crate) fn validate_adapter_name(name: &str) -> Result<(), String> {
    let is_valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && name.chars().any(|c| c != '.');
    if is_valid {
        Ok(())
    } else {
        Err(format!("Invalid adapter_name: {name}"))
    }
}

fn wrapper_script_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    Ok(resolve_bundled_resource(&resource_dir, "scripts/mlx/finetune_wrapper.py"))
}

/// venv 패키지 존재/버전 프로브. 블록 들여쓰기를 포함하므로 Rust `\` 줄 연속으로 재작성하면
/// 안 된다 — 연속은 다음 줄 선행 공백을 제거해 IndentationError가 되고, 그 실패는 "두 패키지
/// 모두 미설치"라는 조용한 오판으로 나타난다. 구문 유효성은 단위 테스트가 고정한다.
const ENV_PROBE_SNIPPET: &str = "import importlib.metadata as m\nfor pkg in ('mlx-lm', 'mlx-vlm'):\n    try: print(pkg + '=' + m.version(pkg))\n    except m.PackageNotFoundError: pass";

async fn check_mlx_env_inner() -> MlxEnvStatus {
    let python_ok = resolve_cli_path("python3").is_ok();
    let mut status = MlxEnvStatus {
        python_ok,
        venv_exists: false,
        mlx_lm_installed: false,
        mlx_lm_version: None,
        mlx_vlm_installed: false,
        mlx_vlm_version: None,
    };

    let Ok(venv_py) = venv_python() else {
        return status;
    };
    status.venv_exists = venv_py.is_file();
    if !status.venv_exists {
        return status;
    }

    // 두 패키지를 한 번의 파이썬 기동으로 조회한다(각각 스폰하면 인터프리터 기동 비용 2배).
    // 한쪽이 없어도 다른 쪽 버전은 나와야 하므로 스니펫이 개별 try로 감싼다.
    let output = tokio::process::Command::new(&venv_py)
        .args(["-c", ENV_PROBE_SNIPPET])
        .env("PATH", augmented_path())
        .output()
        .await;

    if let Ok(out) = output {
        if out.status.success() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                match line.trim().split_once('=') {
                    Some(("mlx-lm", v)) => {
                        status.mlx_lm_installed = true;
                        status.mlx_lm_version = Some(v.to_string());
                    }
                    Some(("mlx-vlm", v)) => {
                        status.mlx_vlm_installed = true;
                        status.mlx_vlm_version = Some(v.to_string());
                    }
                    _ => {}
                }
            }
        }
    }
    status
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
            .map_err(|e| format!("Failed to run venv creation: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "venv creation failed: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }

    let pip = venv_pip()?;
    // mlx-vlm[train]은 mlx-lm을 의존성으로 끌고 오지만(실측: 0.6.7 → mlx-lm 0.31.3),
    // 버전 pin 없이 최신 mlx-lm을 함께 올리기 위해 둘 다 명시한다. [train] extra가 없으면
    // `mlx_vlm.lora`가 ImportError로 죽는다(실측 — datasets 미설치).
    let out = tokio::process::Command::new(&pip)
        .args(["install", "-U", "mlx-lm", "mlx-vlm[train]"])
        .env("PATH", augmented_path())
        .output()
        .await
        .map_err(|e| format!("Failed to run pip install: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "Failed to install mlx-lm/mlx-vlm: {}",
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
            return Err("MLX environment setup is already in progress.".into());
        }
        guard.state = "installing".into();
        guard.error = None;
    }

    tokio::spawn(run_setup(app));
    Ok("Started MLX venv installation.".into())
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
                format!("Training process exited abnormally ({status})")
            } else {
                stderr_text.trim().to_string()
            });
        }
        Err(e) => {
            training.status = "error".into();
            training.error = Some(format!("Failed to wait for process: {e}"));
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
                return Err(format!("Training is already in progress (PID {}).", t.pid));
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
            return Err("iters must be at least 1.".into());
        }
        if config.batch_size == 0 {
            return Err("batch_size must be at least 1.".into());
        }
        if !(config.learning_rate.is_finite() && config.learning_rate > 0.0) {
            return Err("learning_rate must be a finite value greater than 0.".into());
        }
        validate_adapter_name(&config.adapter_name)?;

        let model_path = validate_home_subpath(&config.model_path)?;
        let data_path = validate_home_subpath(&config.data_path)?;

        let venv_py = venv_python()?;
        if !venv_py.is_file() {
            return Err("MLX venv does not exist. Run setup_mlx_env first.".into());
        }

        let wrapper = wrapper_script_path(&app)?;
        if !wrapper.is_file() {
            return Err(format!(
                "Could not find the fine-tuning wrapper script: {}",
                wrapper.display()
            ));
        }

        let mut cmd = tokio::process::Command::new(&venv_py);
        cmd.arg(&wrapper)
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
            .arg("--runtime")
            .arg(match config.runtime.unwrap_or(MlxRuntime::MlxLm) {
                MlxRuntime::MlxLm => "mlx-lm",
                MlxRuntime::MlxVlm => "mlx-vlm",
            });

        if config.train_vision {
            cmd.arg("--train-vision");
        }

        let child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("PATH", augmented_path())
            .process_group(0)
            .spawn()
            .map_err(|e| format!("Failed to launch fine-tuning process: {e}"))?;

        let pid = child
            .id()
            .ok_or_else(|| "Could not get PID.".to_string())?;

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
        .map_err(|e| format!("Failed to wait for process termination: {e}"))?;

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
                format!("Serving process exited unexpectedly ({status})")
            } else {
                format!(
                    "Serving process exited unexpectedly ({status}): {}",
                    stderr_text.trim()
                )
            });
        }
        Err(e) => {
            *err_guard = Some(format!("Failed to wait for serving process: {e}"));
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
    runtime: Option<MlxRuntime>,
) -> Result<String, String> {
    // 지정이 없으면 mlx-lm — 기존 사용자·기존 프런트 호출의 동작이 바뀌지 않는다(D29).
    let runtime = runtime.unwrap_or(MlxRuntime::MlxLm);
    {
        let mut guard = state.serving.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            return Err("Model serving is already in progress.".into());
        }
        *guard = Some(ServingStatus {
            pid: 0,
            port,
            model_path: model_path.clone(),
            adapter_path: adapter_path.clone(),
            runtime,
        });
    }

    let res = (|| -> Result<(u32, tokio::process::Child, String, Option<String>), String> {
        // 8080은 개발 환경에서 다른 서비스(예: Tomcat)가 선점하고 있는 경우가 흔하다.
        // bind 성공 시 리스너를 즉시 drop해 포트를 반납하고 그 사이에 실제 서빙 프로세스를 스폰한다.
        let listener = std::net::TcpListener::bind(("127.0.0.1", port)).map_err(|_| {
            format!("Port {port} is in use by another process. Specify a different port (e.g. 8081) in the serving card.")
        })?;
        drop(listener);

        let validated_model_dir = validate_home_subpath(&model_path)?;
        let is_adapter_dir = validated_model_dir.join("adapter_config.json").is_file();

        let (base_model, effective_adapter): (PathBuf, Option<PathBuf>) = if is_adapter_dir {
            let base = read_adapter_base_model(&validated_model_dir).ok_or_else(|| {
                "This is an adapter directory — specify the base model as well.".to_string()
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
            return Err("MLX venv does not exist. Run setup_mlx_env first.".into());
        }

        let mut cmd = tokio::process::Command::new(&venv_py);
        cmd.args(runtime.server_args())
            .arg("--model")
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
            .map_err(|e| format!("Failed to launch serving process: {e}"))?;

        let pid = child
            .id()
            .ok_or_else(|| "Could not get PID.".to_string())?;

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
                    runtime,
                });
            }
            {
                let mut err_guard = state.last_serving_error.lock().map_err(|e| e.to_string())?;
                *err_guard = None;
            }

            tokio::spawn(run_serving_reader(app, child, pid));

            let adapter_note = effective_adapter_str
                .map(|p| format!(" · adapter {p}"))
                .unwrap_or_default();
            Ok(format!(
                "Started model serving on port {port} (PID {pid}){adapter_note}."
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
    Err("Could not find an available port.".into())
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
            None => return Err("No model serving in progress.".into()),
        }
    };

    tokio::task::spawn_blocking(move || terminate_pid(pid, false))
        .await
        .map_err(|e| format!("Failed to wait for process termination: {e}"))?;

    // Child 소유권은 run_serving_reader가 갖고 있으므로 여기서는 상태만 비운다.
    // reaper가 실제 종료를 감지하고 last_serving_error를 남기지 않는다(사용자 의도 종료).
    {
        let mut serving_guard = state.serving.lock().map_err(|e| e.to_string())?;
        if serving_guard.as_ref().map(|s| s.pid) == Some(pid) {
            *serving_guard = None;
        }
    }

    Ok("Stopped model serving.".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn validate_home_subpath_expands_tilde() {
        // "~"와 "~/..."가 HOME 기준으로 확장되어 검증을 통과해야 한다.
        let home = super::validate_home_subpath("~").expect("~ expansion failed");
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
        let occupied = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("failed to occupy port");
        let occupied_port = occupied.local_addr().expect("failed to get address").port();

        let result = find_available_port(occupied_port..=occupied_port.saturating_add(5))
            .expect("failed to find available port");

        assert_ne!(result, occupied_port);
        drop(occupied);
    }

    /// ENV_PROBE_SNIPPET이 유효한 파이썬인지 고정한다. 이 스니펫이 깨지는 실패 모드는
    /// 예외가 아니라 "두 패키지 모두 미설치"라는 조용한 오판이다 — Rust `\` 줄 연속으로
    /// 재작성하면 들여쓰기가 사라져 정확히 그렇게 된다(작성 시점 셸 재현으로 확인).
    /// 시스템 python3에는 mlx가 없으므로 기대 출력은 빈 stdout + exit 0이다.
    #[test]
    fn env_probe_snippet_is_valid_python() {
        let out = std::process::Command::new("python3")
            .args(["-c", ENV_PROBE_SNIPPET])
            .output()
            .expect("failed to run python3");
        assert!(
            out.status.success(),
            "probe snippet died with a python syntax error: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn finetune_config_defaults_train_vision_to_false() {
        let json = r#"{
            "model_path": "/path/to/model",
            "data_path": "/path/to/data",
            "iters": 100,
            "batch_size": 4,
            "learning_rate": 0.0001,
            "adapter_name": "test-adapter"
        }"#;
        let config: FineTuneConfig = serde_json::from_str(json).unwrap();
        assert!(!config.train_vision);

        let json_with_vision = r#"{
            "model_path": "/path/to/model",
            "data_path": "/path/to/data",
            "iters": 100,
            "batch_size": 4,
            "learning_rate": 0.0001,
            "adapter_name": "test-adapter",
            "train_vision": true
        }"#;
        let config_vision: FineTuneConfig = serde_json::from_str(json_with_vision).unwrap();
        assert!(config_vision.train_vision);
    }
}
