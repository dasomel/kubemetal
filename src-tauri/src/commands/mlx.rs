use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{Manager, State};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::services::ports;
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
    /// 비종료: "running" | "paused" | "paused_memory_pressure" | "paused_battery"
    ///         | "paused_thermal"  (paused*는 guardrails.rs가 설정한다)
    /// 종료:   "done" | "error" | "killed"
    /// 이 목록이 `paused*`를 빠뜨리고 있던 것이 finalize_training의 종료 판정을
    /// `status == "running"`으로 좁히게 만든 배경이다 — 목록과 판정을 함께 둔다.
    pub status: String,
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
    /// 헬스체크를 통과한 마지막 서빙 구성(이슈 #12) — `revert_to_last_serving`이 되돌릴
    /// 대상. 스폰 성공만으로는 채우지 않는다: 모델 로드 실패로 죽는 프로세스를 "성공"으로
    /// 남기면 되돌리기가 똑같이 죽는 구성으로 되돌아간다(D22).
    pub last_known_good_serving: Mutex<Option<ServingStatus>>,
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

#[derive(Debug, Serialize)]
struct ManifestFileEntry {
    file: String,
    sha256: String,
    size_bytes: u64,
}

#[derive(Debug, Serialize)]
struct TrainingManifest {
    adapter_dir: String,
    generated_at_unix: u64,
    files: Vec<ManifestFileEntry>,
}

/// `adapter_dir` 안의 파일들(어댑터 가중치, adapter_config.json 등)의 sha256을 계산해
/// 같은 디렉터리에 `manifest.json`으로 쓴다(이슈 #22 — 산출물 무결성 증빙). 호출부
/// (`apply_training_event`의 "done" 분기)가 학습 성공 시에만 부르므로 실패 케이스에는
/// 쓰이지 않는다. 서브디렉터리·심볼릭 링크는 건너뛰고, 실패는 에러로 전파해 조용히
/// 삼키지 않는다(D22) — 호출부가 로그로 남긴다.
fn write_training_manifest(adapter_dir: &std::path::Path) -> Result<(), String> {
    let entries = std::fs::read_dir(adapter_dir)
        .map_err(|e| format!("Failed to read adapter directory: {e}"))?;

    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {e}"))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        if file_name == "manifest.json" {
            continue; // 매니페스트 자신은 대상에서 제외한다.
        }
        let bytes =
            std::fs::read(&path).map_err(|e| format!("Failed to read {file_name}: {e}"))?;
        let digest = Sha256::digest(&bytes);
        files.push(ManifestFileEntry {
            file: file_name,
            sha256: hex::encode(digest),
            size_bytes: bytes.len() as u64,
        });
    }
    // read_dir은 순서를 보장하지 않는다 — 매니페스트 내용을 재현 가능하게 정렬한다.
    files.sort_by(|a, b| a.file.cmp(&b.file));

    let generated_at_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let manifest = TrainingManifest {
        adapter_dir: adapter_dir.to_string_lossy().to_string(),
        generated_at_unix,
        files,
    };
    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("Failed to serialize manifest: {e}"))?;
    std::fs::write(adapter_dir.join("manifest.json"), json)
        .map_err(|e| format!("Failed to write manifest.json: {e}"))
}

#[derive(Debug, Deserialize)]
struct ModelConfigFile {
    quantization: Option<serde_json::Value>,
}

/// 모델이 quantized인지 판별한다. mlx 커뮤니티 모델은 quantize 시 `config.json`에
/// 최상위 `quantization` 필드(group_size/bits)가 추가된다 — 그 필드의 유무만 본다.
/// `config.json`이 없거나 파싱에 실패하면 판별 불가로 `None`을 반환한다: 이슈 #23은
/// "모르면 통과시켜라"(D22)를 요구한다 — false positive로 정상 학습을 막는 것이
/// 크래시를 막는 것보다 나쁘다.
fn is_quantized_model(model_dir: &std::path::Path) -> Option<bool> {
    let content = std::fs::read_to_string(model_dir.join("config.json")).ok()?;
    let parsed: ModelConfigFile = serde_json::from_str(&content).ok()?;
    Some(parsed.quantization.is_some())
}

/// D29 실측 비호환 조합을 spawn 전에 거부한다: 4bit quantized 모델에 `--train-vision`을
/// 얹으면 양자화된 가중치에 대한 gradient를 요구해 `QuantizedMatmul::vjp`에서 죽는다
/// (LoRA-only는 문제없다 — frozen quantized layer는 forward-only). 판별 불가(`None`)면
/// 통과시킨다 — 알 수 없는 것을 지어내 정상 학습을 막지 않는다(D22).
fn reject_incompatible_runtime_combo(
    model_dir: &std::path::Path,
    train_vision: bool,
) -> Result<(), String> {
    if !train_vision {
        return Ok(());
    }
    if is_quantized_model(model_dir) == Some(true) {
        return Err(
            "4bit quantized 모델은 --train-vision을 지원하지 않습니다 — non-quantized(bf16) 모델을 사용하세요."
                .to_string(),
        );
    }
    Ok(())
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
                // 성공 케이스에서만 매니페스트를 쓴다(이슈 #22) — "error"/"warning" 분기는
                // 이 코드를 부르지 않는다. 실패는 학습 완료 처리 자체를 막지 않고 로그로만
                // 남긴다(D22 — 조용히 삼키지 않는다).
                if let Err(e) = write_training_manifest(std::path::Path::new(p)) {
                    eprintln!("[mlx] Failed to write training manifest for {p}: {e}");
                }
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

/// 프로세스 종료를 상태에 기록해야 하는가.
///
/// 기준은 "이미 종착 상태인가"다. 예전 기준은 `status == "running"`이었는데, 그러면
/// 일시정지된 학습이 밖에서 죽었을 때(OOM killer 등) 화면이 영원히 "일시정지 중"에
/// 머문다 — 존재하지 않는 프로세스를 멈춰 있는 것으로 표시하는 상태 날조다(D22).
/// `paused_*`는 아직 결말이 나지 않은 상태이므로 종료를 기록해야 한다.
///
/// 반대로 `killed`는 보호해야 한다. `kill_mlx_process`가 시그널을 보내기 **전에**
/// 의도를 기록하므로, 그 뒤 도착하는 비정상 종료 코드가 사용자의 의도적 중지를
/// "오류"로 덮어쓰면 안 된다.
fn should_record_exit(status: &str) -> bool {
    !matches!(status, "done" | "error" | "killed")
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
    if !should_record_exit(&training.status) {
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
        reject_incompatible_runtime_combo(&model_path, config.train_vision)?;

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
            })
            // MLflow 주소를 명시로 넘긴다. 넘기지 않으면 래퍼가 자기 기본값(5001 고정)을
            // 쓰는데, 포트는 런타임 값이라(D1 개정) 5001이 점유되면 학습 기록이 통째로
            // 엉뚱한 곳으로 간다.
            .arg("--mlflow-uri")
            .arg(ports::local_url("mlflow"));

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

    // **시그널을 보내기 전에** 의도를 기록한다. 이 블록이 terminate_pid 뒤에 있었을 때는
    // 사용자가 중지를 눌러도 화면에 "Training process exited abnormally" 오류가 떴다:
    // terminate_pid는 SIGTERM 후 1초를 자는데, 그 사이 run_training_reader가 자식의 종료를
    // 관측해 finalize_training을 부르고, 그때 status는 아직 "running"이라 exit code
    // 비정상을 이유로 "error"가 먼저 쓰인다. 그다음 여기 도달해도 `!= "error"` 가드에
    // 걸려 "killed" 갱신이 통째로 스킵됐다.
    //
    // 순서를 뒤집으면 finalize_training의 기존 가드(`status != "running"`이면 반환)가
    // 그대로 보호막이 된다 — 새 가드가 필요한 게 아니라 쓰는 시점이 늦었던 것이다.
    // 의도적 중지의 종료 코드는 오류가 아니므로 기록하지 않는 것이 맞다.
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

    tokio::task::spawn_blocking(move || terminate_pid(pid, is_training))
        .await
        .map_err(|e| format!("Failed to wait for process termination: {e}"))?;

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

    let res = (|| -> Result<(u32, tokio::process::Child, String, Option<String>, u16), String> {
        // 요청한 포트가 막혀 있으면 실패시키지 않고 비어 있는 포트로 비켜간다.
        // 8080은 개발 환경에서 다른 서비스(Docker 컨테이너·Tomcat 등)가 선점하는 일이 흔하고,
        // 그 프로세스가 와일드카드로 바인딩하면 우리 서버가 뜨기 전 창에서 남의 응답이
        // 돌아온다(실측 2026-08-06: `404 page not found`). 바뀐 포트는 반환값에 명시한다.
        let (_, range_end) = serving_port_spec();
        let port = ports::find_free_port(port, range_end.max(port))?;

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
            port,
        ))
    })();

    match res {
        Ok((pid, child, base_model_str, effective_adapter_str, actual_port)) => {
            {
                let mut serving_guard = state.serving.lock().map_err(|e| e.to_string())?;
                *serving_guard = Some(ServingStatus {
                    pid,
                    port: actual_port,
                    model_path: base_model_str.clone(),
                    adapter_path: effective_adapter_str.clone(),
                    runtime,
                });
            }
            // 서빙 포트도 레지스트리에 기록해 다른 소비자가 같은 값을 본다.
            ports::set_assigned("serving", actual_port);
            {
                let mut err_guard = state.last_serving_error.lock().map_err(|e| e.to_string())?;
                *err_guard = None;
            }

            // 이슈 #12: 헬스체크가 통과하는 순간의 구성을 last_known_good으로 남긴다.
            // 스폰 성공 자체가 아니라 헬스체크 통과가 기준이다 — 모델 로드 실패로 죽는
            // 프로세스를 "성공"으로 기록하면 되돌리기가 똑같이 죽는 구성으로 돌아간다(D22).
            let healthcheck_config = ServingStatus {
                pid,
                port: actual_port,
                model_path: base_model_str.clone(),
                adapter_path: effective_adapter_str.clone(),
                runtime,
            };
            let healthcheck_base_url = format!("http://127.0.0.1:{actual_port}/v1");
            tokio::spawn(record_last_known_good_after_healthcheck(
                app.clone(),
                pid,
                healthcheck_base_url,
                healthcheck_config,
            ));

            tokio::spawn(run_serving_reader(app, child, pid));

            let adapter_note = effective_adapter_str
                .map(|p| format!(" · adapter {p}"))
                .unwrap_or_default();
            // 포트가 바뀌었으면 조용히 넘어가지 않는다 — 사용자가 지정한 값과 다르다.
            let moved_note = if actual_port != port {
                format!(" (requested port {port} was in use)")
            } else {
                String::new()
            };
            Ok(format!(
                "Started model serving on port {actual_port} (PID {pid}){adapter_note}.{moved_note}"
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

/// 서빙 포트 규격(우선 8080, 범위 상한 8099)의 단일 출처는 `services::ports::SPECS`다.
/// 예전에는 이 파일이 `127.0.0.1`만 바인딩해 보는 자체 탐지기를 갖고 있었는데, 그 방식은
/// 와일드카드(`*:8080`)로 잡힌 포트를 "비어 있음"으로 오판한다(실측 2026-08-06).
fn serving_port_spec() -> (u16, u16) {
    let spec = ports::SPECS
        .iter()
        .find(|s| s.key == "serving")
        .expect("serving spec must exist in ports::SPECS");
    (spec.preferred, spec.range_end)
}

#[tauri::command]
pub async fn suggest_serving_port() -> Result<u16, String> {
    let (preferred, range_end) = serving_port_spec();
    ports::find_free_port(preferred, range_end)
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

/// 서빙 헬스체크. `access.rs::check_serving_health`와 판정 기준(OpenAI 호환 `/v1/models`가
/// HTTP 200일 때만 ok — TCP 응답만으로는 무관한 프로세스의 404를 정상으로 오판한다,
/// 실측 2026-08-06)이 같다. 이 lane은 `mlx.rs` 단일 파일로 스코프가 고정돼 있어(harness.md)
/// access.rs를 건드리지 않고 최소 구현으로 둔다 — 기준이 바뀌면 두 곳을 함께 고쳐야 한다.
async fn is_serving_healthy(base_url: &str) -> bool {
    let mut cmd = match external_command("curl") {
        Ok(c) => c,
        Err(_) => return false,
    };
    let url = format!("{base_url}/models");
    let output = cmd
        .args(["-s", "-o", "/dev/null", "-m", "2", "-w", "%{http_code}", &url])
        .output()
        .await;
    matches!(output, Ok(out) if String::from_utf8_lossy(&out.stdout) == "200")
}

/// pid가 여전히 현재 서빙과 일치할 때만 `config`를 last_known_good으로 기록한다. 기록
/// 시점 사이에 프로세스가 죽거나 다른 서빙으로 교체됐으면 쓰지 않는다 — 죽은 구성을
/// "마지막 성공"으로 남기면 되돌리기가 똑같이 죽는 구성으로 돌아간다(D22). AppHandle이
/// 필요 없는 순수 판정이라 `MlxState`만으로 직접 테스트한다.
fn record_serving_success(state: &MlxState, pid: u32, config: ServingStatus) -> bool {
    let still_current = state
        .serving
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.pid))
        == Some(pid);
    if !still_current {
        return false;
    }
    match state.last_known_good_serving.lock() {
        Ok(mut guard) => {
            *guard = Some(config);
            true
        }
        Err(_) => false,
    }
}

const SERVING_HEALTHCHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
// 모델 로딩 시간 여유 — 최대 약 60초.
const SERVING_HEALTHCHECK_MAX_ATTEMPTS: u32 = 30;

/// 스폰 직후 헬스체크를 폴링하고, 통과하는 순간의 구성을 last_known_good으로 기록한다
/// (이슈 #12). 판정 자체는 `record_serving_success`가 담당하고 여기서는 폴링 루프만
/// 담당한다 — AppHandle 의존 글루라 이 저장소 관례상(다른 AppHandle 기반 함수들과 동일)
/// 실제 앱에서 라이브 검증하고 별도 유닛 테스트는 두지 않는다.
async fn record_last_known_good_after_healthcheck(
    app: tauri::AppHandle,
    pid: u32,
    base_url: String,
    config: ServingStatus,
) {
    for _ in 0..SERVING_HEALTHCHECK_MAX_ATTEMPTS {
        let state = app.state::<MlxState>();
        let still_current = state
            .serving
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|s| s.pid))
            == Some(pid);
        if !still_current {
            return;
        }
        if is_serving_healthy(&base_url).await {
            record_serving_success(&state, pid, config);
            return;
        }
        tokio::time::sleep(SERVING_HEALTHCHECK_INTERVAL).await;
    }
}

/// state.last_known_good_serving에서 되돌릴 구성을 꺼낸다. 없으면 지어내지 않고
/// 명확한 에러를 반환한다(D22).
fn revert_config_or_error(state: &MlxState) -> Result<ServingStatus, String> {
    state
        .last_known_good_serving
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "되돌릴 이전 구성이 없습니다.".to_string())
}

/// 저장된 last_known_good 구성으로 현재 서빙을 중지 후 재시작한다(이슈 #12 축소 스코프).
/// 프런트 소비자가 아직 없어 IPC로는 노출하지 않는다 — 필요해지면 generate_handler!에
/// 등록하고 scripts/ci/check_ipc_types.py를 통과시킨다(harness.md 스코프 밖: 프런트 UI).
/// `commands` 모듈이 `lib.rs`에서 `pub`이 아니라 이 함수는 어차피 크레이트 외부에
/// 도달 불가능하다 — dead_code는 "미등록 상태에서는 호출부가 없다"는 사실 그대로이므로
/// 지어내지 않고 `allow`로 명시한다(IPC 등록 시 이 allow를 제거한다).
#[allow(dead_code)]
pub(crate) async fn revert_to_last_serving(app: tauri::AppHandle) -> Result<String, String> {
    let target = {
        let state = app.state::<MlxState>();
        revert_config_or_error(&state)?
    };

    let is_serving = {
        let state = app.state::<MlxState>();
        let guard = state.serving.lock().map_err(|e| e.to_string())?;
        guard.is_some()
    };
    if is_serving {
        let state = app.state::<MlxState>();
        stop_model_serving(state).await?;
    }

    let state = app.state::<MlxState>();
    start_model_serving(
        app.clone(),
        state,
        target.model_path,
        target.adapter_path,
        target.port,
        Some(target.runtime),
    )
    .await
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

    /// 학습 상태 집합은 Rust와 TS **두 언어에 각각** 적혀 있다 — 파생시킬 수가 없다.
    /// 그래서 CLAUDE.md 규칙대로 "어긋나면 실패하는 테스트"를 둔다.
    ///
    /// 이 드리프트가 실제로 낸 피해: 프런트 네 곳이 비종료 판정을 `!== 'done' && !== 'error'`로
    /// 복사해 갖고 있었고, 백엔드에 `killed`가 추가된 뒤 그 네 곳이 함께 조용히 틀렸다.
    /// 중지한 학습이 배지·파이프라인에 계속 "진행 중"으로 남고 3초 폴링이 멈추지 않았다.
    #[test]
    fn training_status_sets_match_frontend() {
        const TS: &str = include_str!("../../../src/lib/trainingStatus.ts");

        // 프런트가 비종료로 아는 값 전부가 Rust에서도 비종료여야 한다.
        for status in [
            "running",
            "paused",
            "paused_memory_pressure",
            "paused_battery",
            "paused_thermal",
        ] {
            assert!(
                TS.contains(&format!("'{status}'")),
                "trainingStatus.ts에 비종료 상태 {status}가 없다"
            );
            assert!(
                should_record_exit(status),
                "{status}는 비종료인데 Rust가 종료로 취급한다"
            );
        }
        // 종료 상태도 마찬가지다 — 한쪽만 늘어나면 여기서 깨진다.
        for status in ["done", "error", "killed"] {
            assert!(
                TS.contains(&format!("'{status}'")),
                "trainingStatus.ts에 종료 상태 {status}가 없다"
            );
            assert!(
                !should_record_exit(status),
                "{status}는 종료 상태인데 Rust가 종료 기록을 허용한다"
            );
        }
    }

    /// 종료 기록 여부의 두 방향을 함께 고정한다 — 한쪽만 고치면 다른 쪽이 깨지는 관계다.
    ///
    /// 1. `paused_*`에서 밖으로 죽은 학습은 기록돼야 한다. 예전 기준(`status == "running"`)은
    ///    이걸 막아 화면이 영원히 "일시정지 중"에 머물렀다.
    /// 2. `killed`는 보호돼야 한다. `kill_mlx_process`가 시그널 전에 의도를 기록하는데,
    ///    terminate_pid가 SIGTERM 후 1초를 자는 동안 reader가 비정상 종료 코드를 관측한다 —
    ///    그때 덮어쓰면 사용자가 누른 "중지"가 "오류"로 보고된다.
    #[test]
    fn should_record_exit_protects_terminal_states_but_not_paused() {
        // 아직 결말이 나지 않은 상태 — 종료를 기록해야 한다
        assert!(should_record_exit("running"));
        for paused in ["paused", "paused_memory_pressure", "paused_battery", "paused_thermal"] {
            assert!(
                should_record_exit(paused),
                "{paused}에서 죽은 프로세스가 기록되지 않으면 화면이 일시정지에 고립된다"
            );
        }
        // 이미 결말이 난 상태 — 덮어쓰면 안 된다
        for terminal in ["done", "error", "killed"] {
            assert!(
                !should_record_exit(terminal),
                "{terminal}은 종착 상태인데 종료 기록이 덮어썼다"
            );
        }
    }

    /// 포트 탐지 자체의 검증은 `services::ports`가 소유한다(와일드카드 점유까지 본다).
    /// 여기서는 서빙 규격이 그 레지스트리에 실제로 존재하는지만 고정한다 — 없으면
    /// `serving_port_spec()`이 패닉하므로 기동 경로가 통째로 죽는다.
    #[test]
    fn serving_port_spec_is_registered() {
        let (preferred, range_end) = serving_port_spec();
        assert_eq!(preferred, 8080, "D1 assigns 8080 as the preferred serving port");
        assert!(range_end >= preferred);
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

    /// 테스트 전용 임시 디렉터리를 만든다. tempfile 크레이트 없이 다른 모듈(kagent.rs)과
    /// 같은 관례(`std::env::temp_dir()` + 고유 접미사)를 따른다.
    fn make_temp_model_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kubemetal-mlx-test-{name}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("failed to create temp model dir");
        dir
    }

    #[test]
    fn is_quantized_model_detects_quantization_field() {
        let dir = make_temp_model_dir("quantized");
        std::fs::write(
            dir.join("config.json"),
            r#"{"model_type":"qwen2_vl","quantization":{"group_size":64,"bits":4}}"#,
        )
        .unwrap();
        assert_eq!(is_quantized_model(&dir), Some(true));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn is_quantized_model_returns_false_without_quantization_field() {
        let dir = make_temp_model_dir("bf16");
        std::fs::write(dir.join("config.json"), r#"{"model_type":"qwen2_vl"}"#).unwrap();
        assert_eq!(is_quantized_model(&dir), Some(false));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn is_quantized_model_returns_none_when_undeterminable() {
        // config.json이 아예 없는 디렉터리 — 판별 불가는 지어내지 않고 None이어야 한다(D22).
        let dir = make_temp_model_dir("no-config");
        assert_eq!(is_quantized_model(&dir), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reject_incompatible_runtime_combo_rejects_quantized_with_train_vision() {
        let dir = make_temp_model_dir("reject-quantized-vision");
        std::fs::write(
            dir.join("config.json"),
            r#"{"quantization":{"group_size":64,"bits":4}}"#,
        )
        .unwrap();
        assert!(reject_incompatible_runtime_combo(&dir, true).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reject_incompatible_runtime_combo_allows_non_quantized_with_train_vision() {
        let dir = make_temp_model_dir("allow-bf16-vision");
        std::fs::write(dir.join("config.json"), r#"{"model_type":"qwen2_vl"}"#).unwrap();
        assert!(reject_incompatible_runtime_combo(&dir, true).is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reject_incompatible_runtime_combo_allows_quantized_without_train_vision() {
        // LoRA-only 학습은 quantized 모델에서도 문제없다(D29) — train_vision이 꺼져 있으면
        // 통과해야 한다.
        let dir = make_temp_model_dir("allow-quantized-no-vision");
        std::fs::write(
            dir.join("config.json"),
            r#"{"quantization":{"group_size":64,"bits":4}}"#,
        )
        .unwrap();
        assert!(reject_incompatible_runtime_combo(&dir, false).is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reject_incompatible_runtime_combo_allows_undeterminable_with_train_vision() {
        // 판별 불가(config.json 없음)면 train_vision이 켜져 있어도 통과시켜야 한다(D22).
        let dir = make_temp_model_dir("allow-undeterminable-vision");
        assert!(reject_incompatible_runtime_combo(&dir, true).is_ok());
        std::fs::remove_dir_all(&dir).ok();
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

    fn dummy_serving_status(pid: u32) -> ServingStatus {
        ServingStatus {
            pid,
            port: 8080,
            model_path: "/models/base".into(),
            adapter_path: None,
            runtime: MlxRuntime::MlxLm,
        }
    }

    #[test]
    fn record_serving_success_fills_last_known_good_when_pid_still_current() {
        let state = MlxState::default();
        let config = dummy_serving_status(123);
        *state.serving.lock().unwrap() = Some(config.clone());

        let wrote = record_serving_success(&state, 123, config);

        assert!(wrote);
        assert_eq!(
            state
                .last_known_good_serving
                .lock()
                .unwrap()
                .as_ref()
                .map(|s| s.pid),
            Some(123)
        );
    }

    #[test]
    fn record_serving_success_skips_when_pid_no_longer_current() {
        // 헬스체크가 도는 사이 프로세스가 죽거나 다른 서빙으로 교체된 경우 —
        // 죽은/교체된 구성을 "마지막 성공"으로 남기면 안 된다(D22).
        let state = MlxState::default();
        *state.serving.lock().unwrap() = None;

        let wrote = record_serving_success(&state, 123, dummy_serving_status(123));

        assert!(!wrote);
        assert!(state.last_known_good_serving.lock().unwrap().is_none());
    }

    #[test]
    fn revert_config_or_error_errors_when_nothing_saved() {
        let state = MlxState::default();
        let err = revert_config_or_error(&state).expect_err("되돌릴 이전 구성이 없습니다");
        assert!(err.contains("되돌릴 이전 구성이 없습니다"));
    }

    #[test]
    fn revert_config_or_error_returns_saved_config() {
        let state = MlxState::default();
        *state.last_known_good_serving.lock().unwrap() = Some(dummy_serving_status(42));

        let got = revert_config_or_error(&state).expect("saved config should be returned");

        assert_eq!(got.pid, 42);
    }

    #[test]
    fn write_training_manifest_records_matching_sha256() {
        let dir = make_temp_model_dir("manifest");
        std::fs::write(dir.join("adapters.safetensors"), b"dummy-lora-weights").unwrap();
        std::fs::write(dir.join("adapter_config.json"), br#"{"model":"/base"}"#).unwrap();

        write_training_manifest(&dir).expect("manifest write should succeed");

        let manifest_content = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&manifest_content).unwrap();
        let files = manifest["files"].as_array().expect("files must be an array");
        // manifest.json 자신은 대상에서 제외되므로 원본 2개 파일만 남는다.
        assert_eq!(files.len(), 2);

        for entry in files {
            let name = entry["file"].as_str().unwrap();
            let expected_bytes = std::fs::read(dir.join(name)).unwrap();
            let expected_hash = hex::encode(Sha256::digest(&expected_bytes));
            assert_eq!(entry["sha256"].as_str().unwrap(), expected_hash);
            assert_eq!(
                entry["size_bytes"].as_u64().unwrap(),
                expected_bytes.len() as u64
            );
        }

        std::fs::remove_dir_all(&dir).ok();
    }
}
