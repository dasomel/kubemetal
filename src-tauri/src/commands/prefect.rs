use std::process::Stdio;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{Manager, State};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::commands::mlx::{
    validate_adapter_name, validate_home_subpath, venv_pip, venv_python, EnvSetupStatus,
    FineTuneConfig,
};
use crate::services::process::{augmented_path, external_command, resolve_bundled_resource};

/// 포트포워딩(`port_forward.rs`의 `("prefect","svc/prefect","4200:4200")`)이 살아있다는
/// 전제 하에 호스트에서 접근하는 Prefect REST 베이스 URL. `host_runner.py`도 동일 URL을
/// `PREFECT_API_URL` 환경변수로 주입받는다.
const PREFECT_API_BASE: &str = "http://127.0.0.1:4200/api";
const PREFECT_API_URL: &str = "http://127.0.0.1:4200/api";

#[derive(Debug, Clone, Serialize, Default)]
pub struct FlowRunInfo {
    pub id: String,
    pub name: String,
    pub state_type: String,
    pub state_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrefectStatus {
    pub server_ready: bool,
    pub env_installed: bool,
    pub eval_env_installed: bool,
    pub runner_running: bool,
    pub runner_pid: Option<u32>,
    pub recent_runs: Vec<FlowRunInfo>,
}

/// Phase 4b — `docs/05-mlops-research.md` Q2, D20. MLflow REST `runs/search`
/// (experiment "kubemetal-eval")를 평탄화한 결과. `run_id`가 같은 여러 행이 한 평가
/// run의 태스크별 메트릭들을 나타낸다.
#[derive(Debug, Clone, Serialize)]
pub struct EvalMetric {
    pub run_id: String,
    pub task: String,
    pub metric: String,
    pub value: f64,
    pub timestamp_ms: i64,
}

#[derive(Default)]
pub struct PrefectState {
    pub env_setup: Mutex<EnvSetupStatus>,
    /// `setup_eval_env` 전용 진행 상태 — `env_setup`(prefect 자체 설치)과 동시 진행될 수
    /// 있으므로 별도 Mutex로 분리한다.
    pub eval_env_setup: Mutex<EnvSetupStatus>,
    pub runner_pid: Mutex<Option<u32>>,
    pub last_runner_error: Mutex<Option<String>>,
}

/// `curl`(기존 `external_command` 패턴, D5 augmented PATH)로 GET 요청을 보내고 JSON을
/// 파싱한다. 연결 실패·비-JSON 응답은 모두 `None`으로 수렴시켜 호출부가 "실패 시 빈 값"
/// 규칙을 균일하게 적용할 수 있게 한다.
async fn curl_get_json(url: &str) -> Option<serde_json::Value> {
    let mut cmd = external_command("curl").ok()?;
    let output = cmd.args(["-s", "-m", "3", url]).output().await.ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

async fn curl_post_json(url: &str, body: &serde_json::Value) -> Option<serde_json::Value> {
    let mut cmd = external_command("curl").ok()?;
    let body_str = body.to_string();
    let output = cmd
        .args([
            "-s",
            "-m",
            "10",
            "-X",
            "POST",
            url,
            "-H",
            "Content-Type: application/json",
            "-d",
            &body_str,
        ])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

/// `kubectl get deploy prefect -o json`의 `status.availableReplicas`로 준비 여부를
/// 판정한다(colima.rs `get_cluster_status`의 deploy JSON 패턴과 동일).
async fn check_prefect_server_ready() -> bool {
    let Ok(mut cmd) = external_command("kubectl") else {
        return false;
    };
    let output = cmd
        .args([
            "--context", "colima", "get", "deploy", "prefect", "-n", "default", "-o", "json",
        ])
        .output()
        .await;
    let Ok(out) = output else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else {
        return false;
    };
    json["status"]["availableReplicas"].as_u64().unwrap_or(0) > 0
}

async fn check_prefect_env_installed() -> bool {
    let Ok(venv_py) = venv_python() else {
        return false;
    };
    if !venv_py.is_file() {
        return false;
    }
    let output = tokio::process::Command::new(&venv_py)
        .args(["-c", "import prefect"])
        .env("PATH", augmented_path())
        .output()
        .await;
    matches!(output, Ok(out) if out.status.success())
}

/// venv `python -c "import lm_eval"` 성공 여부로 평가 스택(lm-eval-harness) 설치를
/// 판정한다. `check_prefect_env_installed`와 동일 패턴(D20).
async fn check_eval_env_installed() -> bool {
    let Ok(venv_py) = venv_python() else {
        return false;
    };
    if !venv_py.is_file() {
        return false;
    }
    let output = tokio::process::Command::new(&venv_py)
        .args(["-c", "import lm_eval"])
        .env("PATH", augmented_path())
        .output()
        .await;
    matches!(output, Ok(out) if out.status.success())
}

/// `POST /flow_runs/filter`(실기기 실측, 2026-07-23 prefect 3.7.8)로 최근 실행 5건을
/// 최신순으로 조회한다. 포워딩 미활성 등 어떤 이유로든 실패하면 빈 배열을 반환한다.
async fn fetch_recent_flow_runs() -> Vec<FlowRunInfo> {
    let body = serde_json::json!({ "limit": 5, "sort": "START_TIME_DESC" });
    let Some(value) = curl_post_json(&format!("{PREFECT_API_BASE}/flow_runs/filter"), &body).await
    else {
        return Vec::new();
    };
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|r| {
            Some(FlowRunInfo {
                id: r.get("id")?.as_str()?.to_string(),
                name: r.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                state_type: r
                    .get("state_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("UNKNOWN")
                    .to_string(),
                state_name: r
                    .get("state_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect()
}

#[tauri::command]
pub async fn get_prefect_status(state: State<'_, PrefectState>) -> Result<PrefectStatus, String> {
    let (server_ready, env_installed, eval_env_installed) = tokio::join!(
        check_prefect_server_ready(),
        check_prefect_env_installed(),
        check_eval_env_installed()
    );

    let runner_pid = *state.runner_pid.lock().map_err(|e| e.to_string())?;
    let recent_runs = if server_ready {
        fetch_recent_flow_runs().await
    } else {
        Vec::new()
    };

    Ok(PrefectStatus {
        server_ready,
        env_installed,
        eval_env_installed,
        runner_running: runner_pid.is_some(),
        runner_pid,
        recent_runs,
    })
}

async fn run_prefect_env_setup_inner() -> Result<(), String> {
    let venv_py = venv_python()?;
    if !venv_py.is_file() {
        return Err("MLX venv가 없습니다. MLX 스튜디오에서 setup_mlx_env를 먼저 실행하세요.".into());
    }

    let pip = venv_pip()?;
    let out = tokio::process::Command::new(&pip)
        .args(["install", "-U", "prefect"])
        .env("PATH", augmented_path())
        .output()
        .await
        .map_err(|e| format!("pip install 실행 실패: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "prefect 설치 실패: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

async fn run_prefect_env_setup(app: tauri::AppHandle) {
    let result = run_prefect_env_setup_inner().await;
    let state = app.state::<PrefectState>();
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
pub async fn setup_prefect_env(
    app: tauri::AppHandle,
    state: State<'_, PrefectState>,
) -> Result<String, String> {
    {
        let mut guard = state.env_setup.lock().map_err(|e| e.to_string())?;
        if guard.state == "installing" {
            return Err("Prefect 설치가 이미 진행 중입니다.".into());
        }
        guard.state = "installing".into();
        guard.error = None;
    }

    tokio::spawn(run_prefect_env_setup(app));
    Ok("Prefect 설치를 시작했습니다.".into())
}

/// `lm-eval[api]`를 설치한다(`api` extra는 `local-completions` 모델 타입에 필요한
/// `tenacity`/`tiktoken`을 포함 — 실기기 실측 2026-07-23: extra 없이 설치하면
/// `ModuleNotFoundError: tenacity`로 즉시 실패). `run_prefect_env_setup_inner`와 동일
/// 패턴, venv 부재 시 즉시 Err.
async fn run_eval_env_setup_inner() -> Result<(), String> {
    let venv_py = venv_python()?;
    if !venv_py.is_file() {
        return Err("MLX venv가 없습니다. MLX 스튜디오에서 setup_mlx_env를 먼저 실행하세요.".into());
    }

    let pip = venv_pip()?;
    let out = tokio::process::Command::new(&pip)
        .args(["install", "-U", "lm-eval[api]"])
        .env("PATH", augmented_path())
        .output()
        .await
        .map_err(|e| format!("pip install 실행 실패: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "lm-eval 설치 실패: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

async fn run_eval_env_setup(app: tauri::AppHandle) {
    let result = run_eval_env_setup_inner().await;
    let state = app.state::<PrefectState>();
    let mut guard = match state.eval_env_setup.lock() {
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
pub async fn setup_eval_env(
    app: tauri::AppHandle,
    state: State<'_, PrefectState>,
) -> Result<String, String> {
    {
        let mut guard = state.eval_env_setup.lock().map_err(|e| e.to_string())?;
        if guard.state == "installing" {
            return Err("평가 환경 설치가 이미 진행 중입니다.".into());
        }
        guard.state = "installing".into();
        guard.error = None;
    }

    tokio::spawn(run_eval_env_setup(app));
    Ok("평가 환경(lm-eval) 설치를 시작했습니다.".into())
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

/// 러너 Child의 종료를 백그라운드에서 대기한다. `mlx.rs::run_serving_reader`와 동일한
/// "still_current" 판별 패턴 — `stop_prefect_runner`가 의도적으로 정지시킨 경우
/// (state.runner_pid를 먼저 비움)에는 조용히 반환하고, 아무도 멈추라 하지 않았는데
/// 죽었으면 `last_runner_error`에 원인을 남긴다.
async fn run_runner_reader(app: tauri::AppHandle, mut child: tokio::process::Child, pid: u32) {
    let stderr = child.stderr.take();
    let stderr_task = stderr.map(|err| tokio::spawn(collect_stderr(err)));

    let exit = child.wait().await;
    let stderr_text = if let Some(t) = stderr_task {
        t.await.unwrap_or_default()
    } else {
        String::new()
    };

    let state = app.state::<PrefectState>();
    let still_current = {
        let guard = match state.runner_pid.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        *guard == Some(pid)
    };
    if !still_current {
        return;
    }
    if let Ok(mut guard) = state.runner_pid.lock() {
        *guard = None;
    }

    let mut err_guard = match state.last_runner_error.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    match exit {
        Ok(status) if status.success() => {
            *err_guard = None;
        }
        Ok(status) => {
            *err_guard = Some(if stderr_text.trim().is_empty() {
                format!("Prefect 러너가 예기치 않게 종료되었습니다({status})")
            } else {
                format!(
                    "Prefect 러너가 예기치 않게 종료되었습니다({status}): {}",
                    stderr_text.trim()
                )
            });
        }
        Err(e) => {
            *err_guard = Some(format!("러너 프로세스 대기 실패: {e}"));
        }
    }
}

#[tauri::command]
pub async fn start_prefect_runner(
    app: tauri::AppHandle,
    state: State<'_, PrefectState>,
) -> Result<String, String> {
    {
        let guard = state.runner_pid.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            return Err("Prefect 러너가 이미 실행 중입니다.".into());
        }
    }

    let venv_py = venv_python()?;
    if !venv_py.is_file() {
        return Err("MLX venv가 없습니다. setup_mlx_env를 먼저 실행하세요.".into());
    }

    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let runner_script = resolve_bundled_resource(&resource_dir, "scripts/prefect/host_runner.py");
    if !runner_script.is_file() {
        return Err(format!(
            "Prefect 러너 스크립트를 찾을 수 없습니다: {}",
            runner_script.display()
        ));
    }

    // finetune_wrapper.py(및 그 mlx_lm 학습 자식)를 서브프로세스로 띄우는 러너이므로
    // D17과 동일하게 새 프로세스 그룹의 리더로 기동해, 정지 시 그룹 전체(-pid)로
    // 시그널을 보내면 트리 전체가 함께 종료되도록 한다.
    let child = tokio::process::Command::new(&venv_py)
        .arg(&runner_script)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .env("PATH", augmented_path())
        .env("PREFECT_API_URL", PREFECT_API_URL)
        .process_group(0)
        .spawn()
        .map_err(|e| format!("Prefect 러너 실행 실패: {e}"))?;

    let pid = child
        .id()
        .ok_or_else(|| "PID를 가져올 수 없습니다.".to_string())?;

    {
        let mut guard = state.runner_pid.lock().map_err(|e| e.to_string())?;
        *guard = Some(pid);
    }
    {
        let mut err_guard = state.last_runner_error.lock().map_err(|e| e.to_string())?;
        *err_guard = None;
    }

    tokio::spawn(run_runner_reader(app, child, pid));

    Ok(format!("Prefect 러너를 시작했습니다(PID {pid})."))
}

fn terminate_process_group(pid: u32) {
    let target = -(pid as i32);
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
pub async fn stop_prefect_runner(state: State<'_, PrefectState>) -> Result<String, String> {
    let pid = {
        let guard = state.runner_pid.lock().map_err(|e| e.to_string())?;
        match *guard {
            Some(p) => p,
            None => return Err("실행 중인 Prefect 러너가 없습니다.".into()),
        }
    };

    tokio::task::spawn_blocking(move || terminate_process_group(pid))
        .await
        .map_err(|e| format!("프로세스 종료 대기 실패: {e}"))?;

    {
        let mut guard = state.runner_pid.lock().map_err(|e| e.to_string())?;
        if *guard == Some(pid) {
            *guard = None;
        }
    }

    Ok("Prefect 러너를 정지했습니다.".into())
}

/// `GET /deployments/name/{flow_name}/{deployment_name}`(실기기 실측, 2026-07-23)로
/// finetune deployment id를 조회한 뒤 `POST /deployments/{id}/create_flow_run`으로
/// flow run을 생성한다. 경로 검증은 `mlx.rs::validate_home_subpath`/`validate_adapter_name`을
/// 그대로 재사용한다(D15 venv 재사용과 동일한 원칙 — 검증 로직 분산 금지).
#[tauri::command]
pub async fn trigger_finetune_flow(config: FineTuneConfig) -> Result<String, String> {
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

    let deployment = curl_get_json(&format!("{PREFECT_API_BASE}/deployments/name/finetune/finetune"))
        .await
        .ok_or_else(|| {
            "Prefect 서버에 연결할 수 없습니다 — 포트포워딩(4200)이 활성인지 확인하세요.".to_string()
        })?;
    let deployment_id = deployment
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            "finetune deployment를 찾을 수 없습니다 — Prefect 러너가 실행 중인지 확인하세요."
                .to_string()
        })?;

    let body = serde_json::json!({
        "parameters": {
            "model_path": model_path.to_string_lossy(),
            "data_path": data_path.to_string_lossy(),
            "iters": config.iters,
            "batch_size": config.batch_size,
            "learning_rate": config.learning_rate,
            "adapter_name": config.adapter_name,
        }
    });

    let run = curl_post_json(
        &format!("{PREFECT_API_BASE}/deployments/{deployment_id}/create_flow_run"),
        &body,
    )
    .await
    .ok_or_else(|| "flow run 생성 요청이 실패했습니다.".to_string())?;

    run.get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "flow run 응답에서 id를 읽지 못했습니다.".to_string())
}

/// MLflow REST 호출 베이스 — `modelhub.rs`의 기존 MLflow REST 호출과 동일 호스트 표기
/// (`localhost:5001`, 포트포워딩 `svc/mlflow 5001:5000` 전제).
const MLFLOW_BASE: &str = "http://localhost:5001";

/// `trigger_finetune_flow`와 동일 패턴으로 evaluate deployment id를 조회해 flow run을
/// 생성한다. `serving_port`로 `host_runner.py::evaluate_flow`의 `serving_url` 파라미터
/// (`http://127.0.0.1:{port}/v1`)를 구성 — mlx_lm.server는 IPv4(127.0.0.1)에만 bind하므로
/// `localhost`를 쓰지 않는다(mistakes-log.md 2026-07-21 macOS 항목).
#[tauri::command]
pub async fn trigger_evaluate_flow(
    tasks: String,
    limit: u32,
    serving_port: u16,
) -> Result<String, String> {
    if tasks.trim().is_empty() {
        return Err("tasks는 비어있을 수 없습니다.".into());
    }
    if limit == 0 {
        return Err("limit은 1 이상이어야 합니다.".into());
    }

    let serving_url = format!("http://127.0.0.1:{serving_port}/v1");

    let deployment = curl_get_json(&format!("{PREFECT_API_BASE}/deployments/name/evaluate/evaluate"))
        .await
        .ok_or_else(|| {
            "Prefect 서버에 연결할 수 없습니다 — 포트포워딩(4200)이 활성인지 확인하세요.".to_string()
        })?;
    let deployment_id = deployment
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            "evaluate deployment를 찾을 수 없습니다 — Prefect 러너가 실행 중인지 확인하세요."
                .to_string()
        })?;

    let body = serde_json::json!({
        "parameters": {
            "serving_url": serving_url,
            "tasks": tasks,
            "limit": limit,
        }
    });

    let run = curl_post_json(
        &format!("{PREFECT_API_BASE}/deployments/{deployment_id}/create_flow_run"),
        &body,
    )
    .await
    .ok_or_else(|| "flow run 생성 요청이 실패했습니다.".to_string())?;

    run.get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "flow run 응답에서 id를 읽지 못했습니다.".to_string())
}

/// MLflow experiment "kubemetal-eval"의 최근 run들을 조회해 평탄화한다
/// (`host_runner.py::evaluate_flow`의 `_flatten_metrics`가 남기는 `"task/metric/filter"`
/// 메트릭 키 형식 실측 2026-07-23 기준 — 첫 `/`를 기준으로 task/metric을 분리). experiment가
/// 아직 없거나(평가를 한 번도 실행하지 않음) MLflow에 연결할 수 없으면 빈 배열을 반환한다
/// (다른 조회 커맨드들과 동일한 "실패 시 빈 값" 규칙).
#[tauri::command]
pub async fn get_eval_results() -> Result<Vec<EvalMetric>, String> {
    let Some(exp) = curl_get_json(&format!(
        "{MLFLOW_BASE}/api/2.0/mlflow/experiments/get-by-name?experiment_name=kubemetal-eval"
    ))
    .await
    else {
        return Ok(Vec::new());
    };
    let Some(experiment_id) = exp
        .get("experiment")
        .and_then(|e| e.get("experiment_id"))
        .and_then(|v| v.as_str())
    else {
        return Ok(Vec::new());
    };

    let body = serde_json::json!({
        "experiment_ids": [experiment_id],
        "max_results": 10,
        "order_by": ["attribute.start_time DESC"],
    });
    let Some(search) =
        curl_post_json(&format!("{MLFLOW_BASE}/api/2.0/mlflow/runs/search"), &body).await
    else {
        return Ok(Vec::new());
    };
    let Some(runs) = search.get("runs").and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    for run in runs {
        let Some(run_id) = run
            .get("info")
            .and_then(|i| i.get("run_id"))
            .and_then(|v| v.as_str())
        else {
            continue;
        };
        let Some(metrics) = run
            .get("data")
            .and_then(|d| d.get("metrics"))
            .and_then(|v| v.as_array())
        else {
            continue;
        };
        for m in metrics {
            let (Some(key), Some(value)) = (
                m.get("key").and_then(|v| v.as_str()),
                m.get("value").and_then(|v| v.as_f64()),
            ) else {
                continue;
            };
            let timestamp_ms = m.get("timestamp").and_then(|v| v.as_i64()).unwrap_or(0);
            let (task, metric) = match key.split_once('/') {
                Some((t, rest)) => (t.to_string(), rest.to_string()),
                None => (String::new(), key.to_string()),
            };
            out.push(EvalMetric {
                run_id: run_id.to_string(),
                task,
                metric,
                value,
                timestamp_ms,
            });
        }
    }
    Ok(out)
}
