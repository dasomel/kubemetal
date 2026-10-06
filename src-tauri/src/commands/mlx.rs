use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::services;
use crate::services::adapter_staging::{self, Attempt, AttemptSpec};
#[cfg(test)]
use crate::services::artifact_manifest::{write_manifest, ManifestContext};

mod training_staging;
#[allow(unused_imports)]
pub use crate::services::mlx_lifecycle::{
    check_for_orphaned_mlx_processes, terminate_orphaned_mlx_process, OrphanedProcessInfo,
};
use crate::services::ports;
use crate::services::process::{
    augmented_path, external_command, resolve_bundled_resource, resolve_cli_path,
};
pub use training_staging::list_adapter_staging;

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
    /// 알려진 상태와 종료 분류: `services::mlx_lifecycle::admission::TRAINING_STATUSES`.
    pub status: String,
    pub current_iter: u32,
    pub total_iters: u32,
    pub last_loss: Option<f64>,
    pub adapter_path: Option<String>,
    pub error: Option<String>,
    /// D45: reserves the final name during training. The wrapper writes staging out/,
    /// outside the delete IPC root; promotion and deletion share adapter_admission.
    pub adapter_name: String,
    /// `finetune_wrapper.py`가 `reporter.start_run` 성공 직후 보고하는 실제 MLflow run id
    /// (GitHub #13). 이 값이 있어야 wrapper가 자기 `end_run`을 못 부르고 죽었을 때(시그널로
    /// kill됨) Rust가 대신 MLflow에 종료 상태를 알릴 수 있다. MLflow 비활성/미도달이면
    /// wrapper가 이 이벤트를 아예 보내지 않으므로 `None`으로 남는다(D22 — 지어내지 않는다).
    pub mlflow_run_id: Option<String>,
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
    /// A post-wake process identity check is still pending for this slot.
    pub training_needs_reverification: bool,
    /// A post-wake process and `/v1/models` health check is still pending for this slot.
    pub serving_needs_reverification: bool,
}

#[derive(Default)]
pub struct MlxState {
    /// Serializes adapter deletion with training/serving slot admission.
    pub adapter_admission: Mutex<()>,
    pub env_setup: Mutex<EnvSetupStatus>,
    pub training: Mutex<Option<TrainingStatus>>,
    // Per-child stop intent survives a killed slot being replaced by the next run.
    training_stop: Mutex<Option<(u32, Arc<AtomicBool>)>>,
    pub serving: Mutex<Option<ServingStatus>>,
    pub last_serving_error: Mutex<Option<String>>,
    pub training_needs_reverification: AtomicBool,
    pub serving_needs_reverification: AtomicBool,
    /// 헬스체크를 통과한 마지막 서빙 구성(이슈 #12) — `revert_to_last_serving`이 되돌릴
    /// 대상. 스폰 성공만으로는 채우지 않는다: 모델 로드 실패로 죽는 프로세스를 "성공"으로
    /// 남기면 되돌리기가 똑같이 죽는 구성으로 되돌아간다(D22).
    pub last_known_good_serving: Mutex<Option<ServingStatus>>,
    /// 실측 GPU matmul 벤치마크 실행 중 여부(이슈 #11 상호 배제).
    /// 벤치마크 admission 시 `compare_exchange`로 원자적 선점되며, RAII 가드가 해제한다.
    pub benchmark_active: AtomicBool,
    /// `revert_to_last_serving` 실행 중 여부(#12 리뷰 HIGH — 동시 되돌리기 요청이 서빙을
    /// 이중으로 stop/restart하지 않도록 직렬화). `adapter_admission`(std::sync::Mutex)은
    /// 재사용하지 않는다 — revert는 `stop_model_serving`/`start_model_serving`의 `.await`를
    /// 거치는데, 그 구간에 걸쳐 std Mutex 가드를 들고 있을 수 없기 때문이다(AGENTS.md).
    pub reverting_serving: AtomicBool,
}

/// GPU 벤치마크 상호 배제 예약 RAII 가드 (GitHub #11).
/// 정상 완료, Err 반환, 타임아웃, panic unwinding 등 모든 탈출 경로에서 드롭되어
/// 예약 상태를 반드시 해제한다.
#[derive(Debug)]
pub struct BenchmarkReservationGuard<'a> {
    reserved: &'a AtomicBool,
}

impl<'a> Drop for BenchmarkReservationGuard<'a> {
    fn drop(&mut self) {
        self.reserved.store(false, Ordering::SeqCst);
    }
}

/// `revert_to_last_serving` 상호 배제 예약 RAII 가드 (#12 리뷰 HIGH).
/// 정상 완료, `?`로 인한 조기 반환, panic unwinding 등 모든 탈출 경로에서 드롭되어
/// 예약 상태를 반드시 해제한다.
#[derive(Debug)]
pub struct RevertReservationGuard<'a> {
    reserved: &'a AtomicBool,
}

impl<'a> Drop for RevertReservationGuard<'a> {
    fn drop(&mut self) {
        self.reserved.store(false, Ordering::SeqCst);
    }
}

impl MlxState {
    /// GPU 벤치마크 예약을 원자적으로 선점한다 (`compare_exchange`).
    /// 이미 다른 벤치마크가 실행 중이면 거부 Err를 반환한다.
    pub fn claim_benchmark_reservation(&self) -> Result<BenchmarkReservationGuard<'_>, String> {
        self.benchmark_active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "GPU benchmark is already in progress.".to_string())?;
        Ok(BenchmarkReservationGuard {
            reserved: &self.benchmark_active,
        })
    }

    /// 현재 GPU 벤치마크 예약이 선점되어 있는지 확인한다.
    pub fn is_benchmark_active(&self) -> bool {
        self.benchmark_active.load(Ordering::SeqCst)
    }

    /// 학습 시작 시 벤치마크 상호 배제를 검사한다.
    pub fn check_training_admission(&self) -> Result<(), String> {
        if self.is_benchmark_active() {
            return Err("Cannot start fine-tuning while GPU benchmark is running.".to_string());
        }
        Ok(())
    }

    /// 모델 서빙 시작 시 벤치마크 상호 배제를 검사한다.
    pub fn check_serving_admission(&self) -> Result<(), String> {
        if self.is_benchmark_active() {
            return Err("Cannot start model serving while GPU benchmark is running.".to_string());
        }
        Ok(())
    }

    /// `revert_to_last_serving` 예약을 원자적으로 선점한다 (`compare_exchange`).
    /// 이미 다른 되돌리기가 진행 중이면 거부 Err를 반환한다(#12 리뷰 HIGH).
    pub fn claim_revert_reservation(&self) -> Result<RevertReservationGuard<'_>, String> {
        self.reverting_serving
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| {
                "A revert to the last known-good serving is already in progress.".to_string()
            })?;
        Ok(RevertReservationGuard {
            reserved: &self.reverting_serving,
        })
    }
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
    let expanded = services::home_path::expand_home_path(Path::new(p), &home);
    let canonical = expanded
        .canonicalize()
        .map_err(|e| format!("Path not found: {p} ({e})"))?;
    if !canonical.starts_with(&home) {
        return Err(format!(
            "Path not allowed (only paths under the home directory are allowed): {p}"
        ));
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

/// 어댑터 디렉터리의 매니페스트 검증 상태(이슈 #33 축소 스코프 — 체크포인트 상태
/// 판정). 순수 판정 로직은 `services::mlx_artifacts::manifest_verification_status`에
/// 있다(2026-09-23 리뷰 — mlx.rs가 1128→1612줄로 불어난 것을 서비스 모듈로 덜어낸다,
/// AGENTS.md "파일은 ~300줄 넘으면 쪼갠다"). 이 함수는 그 얇은 재노출이다.
///
/// 프런트 소비자가 아직 없어 IPC로는 노출하지 않는다 — `commands` 모듈이 `lib.rs`에서
/// `pub`이 아니므로 이 함수는 어차피 크레이트 외부에 도달 불가능하다. dead_code는
/// "미등록 상태에서는 호출부가 없다"는 사실 그대로이므로 지어내지 않고 `allow`로
/// 명시한다(IPC 등록 시 이 allow를 제거한다).
#[allow(dead_code)]
pub(crate) fn manifest_verification_status(adapter_dir: &Path) -> &'static str {
    services::mlx_artifacts::manifest_verification_status(adapter_dir)
}

fn adapter_deletion_home(home: Result<PathBuf, String>) -> Option<PathBuf> {
    // D-c (#33): 검증 경로와 HOME 별칭을 맞춘다. 정규화 실패만 원본을 유지하고 조회 실패는 거부한다.
    home.ok().map(|home| home.canonicalize().unwrap_or(home))
}

/// 어댑터가 삭제해도 안전한지 판정하는 GC 가드(이슈 #33).
///
/// 순수 판정 로직(canonical 경로 비교로 서빙 중/last-known-good/진행 중인 학습
/// 세 슬롯 중 하나라도 일치하는지)은 `services::mlx_artifacts::is_adapter_safe_to_delete`에
/// 있다(2026-09-23 리뷰로 이동). 이 함수가 맡는 건 `MlxState`의 세 Mutex를 잠그고
/// 값을 뽑아 그 순수 함수에 넘기는 얇은 호출부뿐이다.
///
/// **잠금이 poison되면(다른 스레드가 그 락을 쥔 채 panic) 즉시 false(삭제 불가)를
/// 반환한다** — 예전 구현은 `.lock().ok().unwrap_or(false)`로 "잠금 실패 = 보호 없음"을
/// 거쳐 최종적으로 "안전"을 반환했다. 셋 중 어느 것도 모른다는 사실을 "안전하다"로
/// 지어내지 않는다(D22, fail-closed) — 서비스 함수의 계약대로, poison된
/// 슬롯은 `None`으로 뭉개 넘기지 않고 그 자리에서 함수를 빠져나온다.
///
/// 진행 중인 학습의 예약된 최종 디렉터리는 `TrainingStatus.adapter_name`(스폰 시점부터
/// 항상 채워짐, `adapter_path`와 달리 "done"을 기다리지 않는다)으로 역산한다 —
/// 2026-09-23 리뷰 전에는 이 세 번째 경우가 아예 없어 진행 중인 학습의 산출물이
/// 삭제 가능하다고 오판했다. D45의 staging은 삭제 루트 밖이다. 최종 이름 예약은
/// 승격 직전까지 유지하며 승격·done 갱신과 삭제는 같은 admission 락으로 직렬화한다.
/// HOME 조회 실패도 서비스에 전달해 삭제를 거부한다.
///
pub(crate) fn is_adapter_safe_to_delete(
    adapter_dir: &Path,
    mlx_state: &MlxState,
    home: Result<PathBuf, String>,
) -> bool {
    let home = adapter_deletion_home(home);
    let serving_adapter_path = match mlx_state.serving.lock() {
        Ok(g) => g.as_ref().and_then(|s| s.adapter_path.clone()),
        Err(_) => return false,
    };
    let last_known_good_adapter_path = match mlx_state.last_known_good_serving.lock() {
        Ok(g) => g.as_ref().and_then(|s| s.adapter_path.clone()),
        Err(_) => return false,
    };
    let in_progress_adapter_name = match mlx_state.training.lock() {
        Ok(g) => g
            .as_ref()
            .filter(|t| should_record_exit(&t.status))
            .map(|t| t.adapter_name.clone()),
        Err(_) => return false,
    };

    services::mlx_artifacts::is_adapter_safe_to_delete(
        adapter_dir,
        home.as_deref(),
        serving_adapter_path.as_deref(),
        last_known_good_adapter_path.as_deref(),
        in_progress_adapter_name.as_deref(),
    )
}

fn remove_adapter_checkpoint(
    adapter_path: &str,
    home: Result<PathBuf, String>,
    mlx_state: &MlxState,
) -> Result<(), String> {
    let _admission = mlx_state
        .adapter_admission
        .lock()
        .map_err(|_| "Failed to lock MLX adapter admission.".to_string())?;
    let requested = Path::new(adapter_path);
    if !requested.is_absolute() {
        return Err("Adapter path must be absolute.".into());
    }
    if requested
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("Adapter path must not contain '..'.".into());
    }
    let home = home?;
    let requested_root = home.join(".kubemetal").join("adapters");
    let adapters_root = requested_root
        .canonicalize()
        .map_err(|e| format!("Failed to resolve adapters directory: {e}"))?;
    // Rebuild components to discard trailing separators before lstat; on macOS a trailing
    // slash makes symlink_metadata follow a symlink instead of inspecting the link itself.
    let normalized = requested
        .components()
        .fold(PathBuf::new(), |mut path, part| {
            path.push(part.as_os_str());
            path
        });
    let target_metadata = std::fs::symlink_metadata(&normalized)
        .map_err(|e| format!("Failed to inspect adapter path: {e}"))?;
    if target_metadata.file_type().is_symlink() {
        return Err("Adapter path must not be a symlink.".into());
    }
    if !target_metadata.is_dir() {
        return Err("Adapter path must be a directory.".into());
    }
    if !normalized.starts_with(&requested_root) || normalized == requested_root {
        return Err("Adapter path must be a directory inside the adapters directory.".into());
    }
    let mut component_path = requested_root.clone();
    for component in normalized
        .strip_prefix(&requested_root)
        .unwrap()
        .components()
    {
        component_path.push(component);
        if std::fs::symlink_metadata(&component_path)
            .map_err(|e| format!("Failed to inspect adapter path component: {e}"))?
            .file_type()
            .is_symlink()
        {
            return Err("Adapter path must not contain symlinks.".into());
        }
    }
    let target = normalized
        .canonicalize()
        .map_err(|e| format!("Failed to resolve adapter path: {e}"))?;
    if target == adapters_root || !target.starts_with(&adapters_root) {
        return Err("Adapter path must be a directory inside the adapters directory.".into());
    }
    if !is_adapter_safe_to_delete(&target, mlx_state, Ok(home.clone())) {
        return Err(
            "Adapter is protected because it is serving, last-known-good, or training.".into(),
        );
    }
    let training_adapter_matches = mlx_state
        .training
        .lock()
        .map_err(|_| "Failed to refresh MLX training state after adapter deletion.".to_string())?
        .as_ref()
        .and_then(|status| status.adapter_path.as_deref())
        .and_then(|path| Path::new(path).canonicalize().ok())
        .as_deref()
        == Some(target.as_path());
    std::fs::remove_dir_all(&target)
        .map_err(|e| format!("Failed to remove adapter checkpoint: {e}"))?;
    let mut training = mlx_state
        .training
        .lock()
        .map_err(|_| "Failed to refresh MLX training state after adapter deletion.".to_string())?;
    if let Some(status) = training.as_mut() {
        if training_adapter_matches {
            status.adapter_path = None;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_adapter_checkpoint(
    app: tauri::AppHandle,
    adapter_path: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let state = app.state::<MlxState>();
        remove_adapter_checkpoint(&adapter_path, home_dir(), &state)
    })
    .await
    .map_err(|e| format!("Adapter deletion task failed: {e}"))?
}

#[derive(Debug, Deserialize)]
struct ModelConfigFile {
    quantization: Option<serde_json::Value>,
    /// transformers 계열(bitsandbytes/GPTQ/AWQ) 컨버전에서 흔한 표기 — mlx-community
    /// 자체 quantize 산출물은 `quantization`을 쓰지만(위 필드), 다른 툴체인을 거친
    /// config.json이 이 키를 대신 쓸 수 있다(2026-09-23 리뷰로 추가; 이 기기의
    /// ~/.cache/huggingface에 실제 사례는 없었고 방어적으로 추가한다 — 아래
    /// is_quantized_model 문서에 실측 결과를 남긴다).
    quantization_config: Option<serde_json::Value>,
    text_config: Option<TextConfigFile>,
}

#[derive(Debug, Deserialize)]
struct TextConfigFile {
    quantization_config: Option<serde_json::Value>,
}

/// 모델이 quantized인지 판별한다. 세 위치 중 하나라도 있으면 quantized로 본다:
/// 최상위 `quantization`(mlx-community 자체 quantize 산출물, group_size/bits),
/// 최상위 `quantization_config`, `text_config.quantization_config`(VLM에서 텍스트
/// 백본만 양자화된 경우 — 위 `ModelConfigFile` 문서 참고). `config.json`이 없거나
/// 파싱에 실패하면 판별 불가로 `None`을 반환한다: 이슈 #23은 "모르면 통과시켜라"(D22)를
/// 요구한다 — false positive로 정상 학습을 막는 것이 크래시를 막는 것보다 나쁘다.
///
/// **실측(2026-09-23, 이 기기의 ~/.cache/huggingface)**: mlx-community의
/// `Qwen2-VL-2B-Instruct-4bit`는 최상위 `quantization: {group_size: 64, bits: 4}`만
/// 쓰고 `quantization_config`/`text_config`는 없다 — bf16 짝(`Qwen2-VL-2B-Instruct-bf16`)은
/// 둘 다 없다. 로컬 캐시 전체(허깅페이스 hub 디렉터리)를 훑어도 `quantization_config`를
/// 쓰는 config.json은 하나도 없었다 — 그 분기는 실사례가 아니라 리뷰 요청에 따른
/// 방어적 추가다.
fn is_quantized_model(model_dir: &std::path::Path) -> Option<bool> {
    let content = std::fs::read_to_string(model_dir.join("config.json")).ok()?;
    let parsed: ModelConfigFile = serde_json::from_str(&content).ok()?;
    Some(
        parsed.quantization.is_some()
            || parsed.quantization_config.is_some()
            || parsed
                .text_config
                .and_then(|t| t.quantization_config)
                .is_some(),
    )
}

/// D29 실측 비호환 조합을 spawn 전에 거부한다: 양자화된(4-bit/8-bit 등) 모델에
/// `--train-vision`을 얹으면 양자화된 가중치에 대한 gradient를 요구해
/// `QuantizedMatmul::vjp`에서 죽는다(LoRA-only는 문제없다 — frozen quantized layer는
/// forward-only). 판별 불가(`None`)면 통과시킨다 — 알 수 없는 것을 지어내 정상 학습을
/// 막지 않는다(D22).
fn reject_incompatible_runtime_combo(
    model_dir: &std::path::Path,
    train_vision: bool,
) -> Result<(), String> {
    if !train_vision {
        return Ok(());
    }
    if is_quantized_model(model_dir) == Some(true) {
        return Err(
            "양자화된(4-bit/8-bit 등) 모델은 --train-vision을 지원하지 않습니다 — non-quantized(bf16) 모델을 사용하세요."
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
    Ok(resolve_bundled_resource(
        &resource_dir,
        "scripts/mlx/finetune_wrapper.py",
    ))
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

fn apply_training_event(app: &tauri::AppHandle, child_pid: u32, value: &serde_json::Value) {
    let state = app.state::<MlxState>();
    if let Ok(mut guard) = state.training.lock() {
        if let Some(training) = guard.as_mut().filter(|t| t.pid == child_pid) {
            training_staging::apply_event(training, value);
        }
    };
}

async fn read_stdout_lines(
    app: tauri::AppHandle,
    child_pid: u32,
    stdout: tokio::process::ChildStdout,
    expected: PathBuf,
) -> training_staging::CompletionReport {
    let mut lines = BufReader::new(stdout).lines();
    let mut report = training_staging::CompletionReport::default();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    report.observe(&value, &expected);
                    apply_training_event(&app, child_pid, &value);
                }
            }
            Ok(None) => break,
            Err(e) => {
                report.read_error(format!("Failed to read training output: {e}"));
                break;
            }
        }
    }
    report
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
///
/// 종착 상태 기준은 `run_mlx_finetune`의 재요청 거부 가드(GitHub #101)와 같아야 하므로
/// `services::mlx_lifecycle::is_non_terminal_training_status`에 단일 정의를 두고 재사용한다.
fn should_record_exit(status: &str) -> bool {
    crate::services::mlx_lifecycle::is_non_terminal_training_status(status)
}

async fn run_training_reader(
    app: tauri::AppHandle,
    mut child: tokio::process::Child,
    mut attempt: Attempt,
    stopped: Arc<AtomicBool>,
    setup_error: Option<String>,
    pid: u32,
) {
    let stdout_task = child
        .stdout
        .take()
        .map(|out| tokio::spawn(read_stdout_lines(app.clone(), pid, out, attempt.out_dir())));
    let stderr_task = child
        .stderr
        .take()
        .map(|err| tokio::spawn(collect_stderr(err)));
    let report = if let Some(t) = stdout_task {
        t.await.unwrap_or_default()
    } else {
        training_staging::CompletionReport::default()
    };
    let stderr_text = if let Some(t) = stderr_task {
        t.await.unwrap_or_default()
    } else {
        String::new()
    };
    let exit = child.wait().await;
    if let Ok(home) = home_dir() {
        crate::services::mlx_lifecycle::remove_pid_marker(&home, "training", pid).await;
    }
    let result = tokio::task::spawn_blocking(move || {
        let state = app.state::<MlxState>();
        training_staging::finalize(
            &state,
            &mut attempt,
            &stopped,
            training_staging::TrainingExit {
                pid,
                exit,
                report,
                stderr: stderr_text,
                setup_error,
            },
        )
    })
    .await;
    match result {
        Ok(Ok(Some(r))) => crate::services::mlx_lifecycle::reconcile_mlflow_run(r).await,
        Ok(Ok(None)) => {}
        Ok(Err(e)) => eprintln!("[mlx] Failed to finalize staging attempt: {e}"),
        Err(e) => eprintln!("[mlx] Staging finalization task failed: {e}"),
    }
}

/// 현재 메모리 압력, 발열 상태, 발열 일시정지 설정을 조회하여 프로세스 스폰 허용 여부를 검사한다(D40).
/// 슬롯 선점 및 동기 준비(경로 검증, config 읽기, 포트 탐색) 전에 1회 검사하며,
/// 검사 시점과 실제 스폰 사이의 상태 변화는 막지 못한다.
pub(crate) async fn check_current_spawn_admission(
    state: &crate::commands::guardrails::GuardrailState,
) -> Result<(), String> {
    let memory_pressure_level = crate::commands::guardrails::measure_memory_pressure_level().await;
    let thermal_state = crate::commands::metrics::read_thermal_state();
    // D40/D22: a poisoned opt-in setting is unknown, not disabled; retry after restart.
    let thermal_pause_enabled = *state
        .thermal_pause_enabled
        .lock()
        .map_err(|e| format!("Cannot read thermal pause configuration: {e} — restart KubeMetal to reset guardrail settings."))?;
    crate::commands::guardrails::check_spawn_admission(
        &memory_pressure_level,
        thermal_state.as_deref(),
        thermal_pause_enabled,
    )
}

#[tauri::command]
pub async fn run_mlx_finetune(
    app: tauri::AppHandle,
    state: State<'_, MlxState>,
    config: FineTuneConfig,
) -> Result<u32, String> {
    let training_runtime = config.runtime.unwrap_or(MlxRuntime::MlxLm);

    // GitHub #11: GPU benchmark 상호 배제 — 벤치마크 실행 중이면 즉시 거부한다.
    state.check_training_admission()?;

    // 스폰 전 admission 게이트(D40, GitHub #32) — 슬롯 선점 및 동기 준비 전 1회 검사.
    // 이미 메모리 압력이 critical이거나(D16) 발열 일시정지가 켜진 채 serious 이상이면(D28)
    // 슬롯을 점유하지 않고 거부한다. 검사~스폰 사이 상태 변화는 막지 못하며, 학습은 스폰 후
    // spawn_guardrail_loop가 사후 방어한다.
    check_current_spawn_admission(&app.state::<crate::commands::guardrails::GuardrailState>())
        .await?;

    let prev_training = {
        let _admission = state.adapter_admission.lock().map_err(|e| e.to_string())?;
        let mut guard = state.training.lock().map_err(|e| e.to_string())?;
        // 슬롯 점유 락을 잡은 상태에서 벤치마크 선점 여부를 재확인하여 레이스 윈도우를 차단한다.
        state.check_training_admission()?;
        if let Some(t) = guard.as_ref() {
            // GitHub #101 — `status == "running"`만 보면 가드레일이 SIGSTOP한 paused* 학습의
            // 슬롯을 새 요청이 덮어쓴다. running/paused* 등 비종료 상태 전체를 거부한다.
            if crate::services::mlx_lifecycle::is_non_terminal_training_status(&t.status) {
                return Err(
                    crate::services::mlx_lifecycle::in_progress_rejection_message(&t.status, t.pid),
                );
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
            adapter_name: config.adapter_name.clone(),
            mlflow_run_id: None,
        });
        prev
    };

    let prepare_app = app.clone();
    let prepare_config = config.clone();
    let res = tokio::task::spawn_blocking(
        move || -> Result<(u32, tokio::process::Child, Attempt, Option<String>), String> {
            let app = prepare_app;
            let config = prepare_config;
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

            let root = home_dir()?.join(".kubemetal");
            let mut attempt = adapter_staging::create_attempt(
                &root,
                &AttemptSpec {
                    adapter_name: config.adapter_name.clone(),
                    runtime: match training_runtime {
                        MlxRuntime::MlxLm => "mlx-lm",
                        MlxRuntime::MlxVlm => "mlx-vlm",
                    }
                    .into(),
                    base_model: model_path.to_string_lossy().into_owned(),
                    iters: config.iters,
                    mlflow_run_id: None,
                },
            )
            .map_err(|e| e.to_string())?;
            let mut cmd = tokio::process::Command::new(&venv_py);
            cmd.args(finetune_wrapper_args(
                &wrapper,
                &model_path,
                &data_path,
                &config,
                training_runtime,
                &attempt.out_dir(),
                &ports::local_url("mlflow"),
            ));

            let child = training_staging::spawn(&mut cmd, &attempt)?;

            let pid = child.id().ok_or_else(|| "Could not get PID.".to_string())?;

            // D45: reuse the marker's PID/sysinfo birth-time identity. One refresh costs
            // startup time; unknown identity fails completion instead of granting authority.
            let start_time = services::process::process_start_time(pid);
            let setup_error = adapter_staging::mark_running(&mut attempt, pid, start_time)
                .err()
                .map(|e| e.to_string())
                .or_else(|| {
                    start_time
                        .is_none()
                        .then(|| "Cannot determine training process start time".into())
                });
            Ok((pid, child, attempt, setup_error))
        },
    )
    .await
    .unwrap_or_else(|e| Err(format!("Training preparation task failed: {e}")));

    match res {
        Ok((pid, child, attempt, setup_error)) => {
            let stopped = Arc::new(AtomicBool::new(false));
            {
                let mut guard = state.training.lock().map_err(|e| e.to_string())?;
                *state.training_stop.lock().map_err(|e| e.to_string())? =
                    Some((pid, stopped.clone()));
                *guard = Some(TrainingStatus {
                    pid,
                    status: "running".into(),
                    current_iter: 0,
                    total_iters: config.iters,
                    last_loss: None,
                    adapter_path: None,
                    error: None,
                    adapter_name: config.adapter_name.clone(),
                    mlflow_run_id: None,
                });
            }

            // 앱 크래시 후에도 이 pid를 다음 실행이 고아로 탐지할 수 있게 남긴다(GitHub #13).
            if let Ok(home) = home_dir() {
                if let Err(e) =
                    crate::services::mlx_lifecycle::write_pid_marker(&home, "training", pid).await
                {
                    eprintln!("[mlx] Failed to write training pid marker: {e}");
                }
            }

            crate::commands::guardrails::start_caffeinate(&app, pid);
            crate::commands::guardrails::spawn_guardrail_loop(app.clone(), pid);

            if setup_error.is_some() {
                // Missing process identity cannot grant group-signal authority. A known
                // marker identity is rechecked by terminate_pid; otherwise keep tracking
                // the child until exit and refuse promotion, rather than guessing a PID.
                if let Some(start_time) = attempt.record().start_time {
                    let _ = terminate_pid(pid, true, Some(start_time)).await;
                }
            }
            tokio::spawn(run_training_reader(
                app,
                child,
                attempt,
                stopped,
                setup_error,
                pid,
            ));

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

    let training_needs_reverification =
        if state.training_needs_reverification.load(Ordering::SeqCst) {
            let verified = match training.as_ref() {
                None => true,
                Some(training) if services::process::pid_is_alive(training.pid) => {
                    matches!(
                        services::mlx_lifecycle::inspect_mlx_process(training.pid).await,
                        services::mlx_lifecycle::CmdlineVerification::Mlx(_)
                    )
                }
                Some(_) => false,
            };
            if verified {
                state
                    .training_needs_reverification
                    .store(false, Ordering::SeqCst);
            }
            !verified
        } else {
            false
        };
    let serving_needs_reverification = if state.serving_needs_reverification.load(Ordering::SeqCst)
    {
        let verified = match serving.as_ref() {
            None => true,
            Some(serving) if services::process::pid_is_alive(serving.pid) => {
                is_serving_healthy(&format!("http://127.0.0.1:{}/v1", serving.port)).await
            }
            Some(_) => false,
        };
        if verified {
            state
                .serving_needs_reverification
                .store(false, Ordering::SeqCst);
        }
        !verified
    } else {
        false
    };

    Ok(MlxStatus {
        env,
        env_setup,
        training,
        serving,
        last_serving_error,
        training_needs_reverification,
        serving_needs_reverification,
    })
}

/// terminate_pid가 시그널을 보낼 대상 PID/PGID를 결정한다.
/// PID가 0인 경우(시작 중인 플레이스홀더 등) 앱 자신의 프로세스(그룹)에 시그널이 가는 것을
/// 방지하기 위해 `None`을 반환한다.
pub(crate) fn resolve_signal_target(pid: u32, use_process_group: bool) -> Option<i32> {
    if pid == 0 {
        return None;
    }
    if use_process_group {
        Some(-(pid as i32))
    } else {
        Some(pid as i32)
    }
}

fn resolve_training_signal_target(pid: u32, group_is_alive: bool) -> Option<i32> {
    if pid == 0 {
        None
    } else if group_is_alive {
        Some(-(pid as i32))
    } else {
        Some(pid as i32)
    }
}

fn marker_start_time_matches(expected: Option<u64>, actual: Option<u64>) -> bool {
    expected.is_some() && expected == actual
}

/// SIGTERM 전송 후 1초 대기하고, 남아 있는 대상은 명령줄을 다시 검증한 뒤 SIGKILL 한다.
/// `use_process_group`이면 시그널을 `-pid`(프로세스 그룹)로 보낸다 — 학습 래퍼는
/// `.process_group(0)`으로 기동되어 자신이 그룹 리더이므로, 그룹으로 보내야 내부에서
/// `subprocess.Popen`으로 띄운 `mlx_lm` 학습 자식까지 함께 종료된다(D17). 서빙 프로세스는
/// 새 그룹 없이 앱과 그룹을 공유하므로 단일 pid로 보낸다.
///
/// PID가 0이면 앱 자신의 프로세스 그룹에 시그널이 전송되는 것을 방지하기 위해 아무것도 하지 않고 즉시 반환한다.
fn signal_target_is_alive(target: i32) -> bool {
    let result = unsafe { libc::kill(target, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// SIGTERM 후 그룹 리더만 먼저 사라진 경우에도, 살아 있는 PGID는 같은 그룹의 자식을
/// 가리킨다. 단일 서빙 PID와 리더가 아직 살아 있는 경우에는 이 예외를 적용하지 않는다.
async fn surviving_training_group_witness(pid: u32, members: &[(u32, u64)]) -> Option<(u32, u64)> {
    // D-MLX-GROUP-1 (#124): a live group number alone cannot prove identity after
    // its leader exits. Retain a pre-TERM member's birth time and group membership;
    // without that witness we refuse, at the cost of manual orphan recovery.
    for &(member, start_time) in members {
        if tokio::task::spawn_blocking(move || {
            services::process::process_still_in_group(member, start_time, pid)
        })
        .await
        .unwrap_or(false)
            && matches!(
                services::mlx_lifecycle::inspect_mlx_process(member).await,
                services::mlx_lifecycle::CmdlineVerification::Mlx(_)
            )
            && tokio::task::spawn_blocking(move || {
                services::process::process_still_in_group(member, start_time, pid)
            })
            .await
            .unwrap_or(false)
        {
            return Some((member, start_time));
        }
    }
    None
}

/// SIGTERM 뒤의 대기만 blocking worker에서 수행한다. 학습은 `-pgid`에 signal 0을 보내
/// 리더가 아닌 남은 자식도 확인한다. SIGKILL 직전에는 PID의 명령줄을 다시 읽어 PID 재사용으로
/// 무관한 프로세스(또는 그룹)를 종료하지 않게 한다.
pub(crate) async fn terminate_pid(
    pid: u32,
    use_process_group: bool,
    expected_start_time: Option<u64>,
) -> Result<(), String> {
    let target = match resolve_signal_target(pid, use_process_group) {
        Some(t) => t,
        None => return Ok(()),
    };
    let target = if use_process_group {
        resolve_training_signal_target(pid, signal_target_is_alive(target))
            .ok_or_else(|| format!("Invalid training PID {pid}"))?
    } else {
        target
    };
    if expected_start_time.is_some()
        && !marker_start_time_matches(
            expected_start_time,
            tokio::task::spawn_blocking(move || services::process::process_start_time(pid))
                .await
                .map_err(|e| format!("Failed to inspect process {pid}: {e}"))?,
        )
    {
        return Err(format!(
            "Refusing to signal process {pid}: process start time differs from its marker."
        ));
    }
    let members = if target < 0 {
        tokio::task::spawn_blocking(move || services::process::process_group_members(pid))
            .await
            .map_err(|e| format!("Failed to inspect training group {pid}: {e}"))?
    } else {
        Vec::new()
    };
    let target_survives = tokio::task::spawn_blocking(move || -> Result<bool, String> {
        if expected_start_time.is_some()
            && !marker_start_time_matches(
                expected_start_time,
                services::process::process_start_time(pid),
            )
        {
            return Err(format!(
                "Refusing to signal process {pid}: process identity changed before SIGTERM."
            ));
        }
        let term_result = unsafe { libc::kill(target, libc::SIGTERM) };
        if term_result != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(false);
            }
            return Err(format!("Failed to SIGTERM process {pid}: {error}"));
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
        Ok(signal_target_is_alive(target))
    })
    .await
    .map_err(|e| format!("Failed to wait for process termination: {e}"))??;
    if !target_survives {
        return Ok(());
    }

    let leader_start_time =
        tokio::task::spawn_blocking(move || services::process::process_start_time(pid))
            .await
            .map_err(|e| format!("Failed to inspect process {pid}: {e}"))?;
    let surviving_group_witness = if target < 0 && leader_start_time.is_none() {
        surviving_training_group_witness(pid, &members).await
    } else {
        None
    };
    let verified_surviving_group = surviving_group_witness.is_some();
    if expected_start_time.is_some()
        && !verified_surviving_group
        && !marker_start_time_matches(expected_start_time, leader_start_time)
    {
        return Err(format!(
            "Refusing to SIGKILL process {pid}: process start time differs from its marker."
        ));
    }

    if !verified_surviving_group
        && !matches!(
            services::mlx_lifecycle::inspect_mlx_process(pid).await,
            services::mlx_lifecycle::CmdlineVerification::Mlx(_)
        )
    {
        return Err(format!(
            "Refusing to SIGKILL process {pid}: could not re-verify MLX argv identity."
        ));
    }

    tokio::task::spawn_blocking(move || {
        if let Some((member, start_time)) = surviving_group_witness {
            if services::process::process_start_time(pid).is_some()
                || !services::process::process_still_in_group(member, start_time, pid)
            {
                return Err(std::io::Error::other(
                    "Training group identity changed before SIGKILL",
                ));
            }
        } else if expected_start_time.is_some()
            && !marker_start_time_matches(
                expected_start_time,
                services::process::process_start_time(pid),
            )
        {
            return Err(std::io::Error::other(
                "Process identity changed before SIGKILL",
            ));
        }
        let kill_result = unsafe { libc::kill(target, libc::SIGKILL) };
        if kill_result != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok::<(), std::io::Error>(())
    })
    .await
    .map_err(|e| format!("Failed to SIGKILL process {pid}: {e}"))?
    .map_err(|e| format!("Failed to SIGKILL process {pid}: {e}"))?;
    Ok(())
}

/// 프런트엔드는 현재 세션이 추적하는 학습 또는 서빙 PID만 종료할 수 있다. `Some(true)`는
/// 학습 프로세스 그룹을, `Some(false)`는 서빙 단일 PID를 뜻한다.
pub(crate) fn tracked_mlx_process_is_training(
    pid: u32,
    training_pid: Option<u32>,
    serving_pid: Option<u32>,
) -> Option<bool> {
    if training_pid == Some(pid) {
        Some(true)
    } else if serving_pid == Some(pid) {
        Some(false)
    } else {
        None
    }
}

/// pid가 현재 추적되는 학습/서빙 프로세스가 아닐 때 Stop 요청의 처리를 결정한다.
/// 경합(서빙이 이미 종료돼 reaper인 `run_serving_reader`가 state를 비운 뒤 사용자가 Stop을
/// 누름)이면 pid는 이미 죽어 있으므로 이미 멈춘 것으로 보아 성공 처리한다 — 살아 있는데
/// 추적되지 않는 pid는 권한 밖이므로 계속 거부한다(D22, GitHub #13 MED).
pub(crate) fn untracked_pid_stop_outcome(pid: u32, is_alive: bool) -> Result<bool, String> {
    if is_alive {
        Err(format!(
            "Process {pid} is not the currently tracked MLX training or serving process."
        ))
    } else {
        Ok(true)
    }
}

/// 시그널 전송이 실패했을 때 `kill_mlx_process`가 시그널 전에 낙관적으로 써둔 "killed"
/// 상태를 되돌려야 하는지 결정한다. 그 사이 다른 경로(예: training_staging::finalize)가 이미 다른
/// 종착 상태로 갱신했다면 건드리지 않는다(D22, GitHub #13 LOW).
pub(crate) fn should_revert_optimistic_killed_status(current_status: &str) -> bool {
    current_status == "killed"
}

#[tauri::command]
pub async fn kill_mlx_process(state: State<'_, MlxState>, pid: u32) -> Result<bool, String> {
    if pid == 0 {
        return Err("Process is still starting; nothing to stop yet.".into());
    }

    let training_pid = {
        let guard = state.training.lock().map_err(|e| e.to_string())?;
        guard.as_ref().map(|t| t.pid)
    };
    let serving_pid = {
        let guard = state.serving.lock().map_err(|e| e.to_string())?;
        guard.as_ref().map(|s| s.pid)
    };
    let is_training = match tracked_mlx_process_is_training(pid, training_pid, serving_pid) {
        Some(is_training) => is_training,
        None => {
            return untracked_pid_stop_outcome(pid, crate::services::process::pid_is_alive(pid))
        }
    };

    // **시그널을 보내기 전에** 의도를 기록한다. 이 블록이 terminate_pid 뒤에 있었을 때는
    // 사용자가 중지를 눌러도 화면에 "Training process exited abnormally" 오류가 떴다:
    // terminate_pid는 SIGTERM 후 1초를 자는데, 그 사이 run_training_reader가 자식의 종료를
    // 관측해 training_staging::finalize을 부르고, 그때 status는 아직 "running"이라 exit code
    // 비정상을 이유로 "error"가 먼저 쓰인다. 그다음 여기 도달해도 `!= "error"` 가드에
    // 걸려 "killed" 갱신이 통째로 스킵됐다.
    //
    // D45: slot status and per-child stop intent are recorded before signaling. The
    // reader retains that intent even if a new run replaces the killed slot; no late
    // child done event can authorize promotion after stop.
    training_staging::request_stop(&state, pid)?;

    if let Err(error) = terminate_pid(pid, is_training, None).await {
        // 시그널 전송이 실패했으니 위에서 낙관적으로 쓴 "killed"를 되돌린다 — 프로세스가
        // 여전히 살아있을 수 있는데 종료된 것으로 보여주면 안 된다(D22).
        let mut guard = state.training.lock().map_err(|e| e.to_string())?;
        if let Some(t) = guard.as_mut() {
            if t.pid == pid && should_revert_optimistic_killed_status(&t.status) {
                training_staging::clear_stop_intent(&state, pid)?;
                t.status = "error".into();
                t.error = Some(error.clone());
            }
        }
        return Err(error);
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
    // child.wait()가 이미 완료됐다 — 프로세스는 실제로 종료됐으므로 아래 still_current
    // 판정과 무관하게 marker를 지운다(GitHub #13). 그래야 다음 실행이 이미 죽은 프로세스를
    // 고아로 오탐하지 않는다.
    if let Ok(home) = home_dir() {
        crate::services::mlx_lifecycle::remove_pid_marker(&home, "serving", pid).await;
    }
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

    // GitHub #11: GPU benchmark 상호 배제 — 벤치마크 실행 중이면 즉시 거부한다.
    state.check_serving_admission()?;

    // 스폰 전 admission 게이트(D40, GitHub #32) — 슬롯 선점 및 동기 준비 전 1회 검사.
    // 학습과 동일한 기준이나, 서빙에는 `spawn_guardrail_loop`가 붙지 않아(학습 전용)
    // 검사~스폰 사이나 스폰 후의 상태 악화를 사후 방어하지 못하는 한계가 있다.
    check_current_spawn_admission(&app.state::<crate::commands::guardrails::GuardrailState>())
        .await?;

    {
        let _admission = state.adapter_admission.lock().map_err(|e| e.to_string())?;
        let mut guard = state.serving.lock().map_err(|e| e.to_string())?;
        // 슬롯 점유 락을 잡은 상태에서 벤치마크 선점 여부를 재확인하여 레이스 윈도우를 차단한다.
        state.check_serving_admission()?;
        if guard.is_some() {
            return Err("Model serving is already in progress.".into());
        }
        *guard = Some(ServingStatus {
            pid: 0,
            port,
            model_path: model_path.clone(),
            // Until path validation determines whether model_path is itself an adapter,
            // reserve both possible request paths so deletion cannot race that resolution.
            adapter_path: adapter_path.clone().or_else(|| Some(model_path.clone())),
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

        let pid = child.id().ok_or_else(|| "Could not get PID.".to_string())?;

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
            // 앱 크래시 후에도 이 pid를 다음 실행이 고아로 탐지할 수 있게 남긴다(GitHub #13).
            if let Ok(home) = home_dir() {
                if let Err(e) =
                    crate::services::mlx_lifecycle::write_pid_marker(&home, "serving", pid).await
                {
                    eprintln!("[mlx] Failed to write serving pid marker: {e}");
                }
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
    if pid == 0 {
        return Err("Serving process is still starting; nothing to stop yet.".into());
    }

    terminate_pid(pid, false, None).await?;

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

/// 서빙 헬스체크. `access.rs::check_serving_health`를 그대로 재사용한다 — 판정 기준
/// (OpenAI 호환 `/v1/models`가 HTTP 200일 때만 ok, 실측 2026-08-06)이 완전히 같은데도
/// 이전 재랜드는 harness.md의 lane 스코프(mlx.rs 단일 파일)를 이유로 curl 호출을
/// 그대로 복제해뒀다 — 두 곳 중 한쪽만 고쳐지면 판정 기준이 갈라진다(2026-09-23
/// 리뷰로 통합, AGENTS.md "같은 사실 두 곳 금지").
async fn is_serving_healthy(base_url: &str) -> bool {
    crate::commands::access::check_serving_health(base_url).await == "ok"
}

/// pid가 여전히 현재 서빙과 일치할 때만 `config`를 last_known_good으로 기록한다 — 그
/// 판정 자체(`should_record_as_last_known_good`)는
/// `services::mlx_serving_recovery`의 순수 함수에 있다(2026-09-23 리뷰로 이동). 여기서는
/// `MlxState`의 두 Mutex를 잠그고 쓰는 얇은 호출부만 담당한다.
fn record_serving_success(state: &MlxState, pid: u32, config: ServingStatus) -> bool {
    let current_serving_pid = state
        .serving
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.pid));
    if !services::mlx_serving_recovery::should_record_as_last_known_good(current_serving_pid, pid) {
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

/// 헬스체크를 통과해 기록된 마지막 정상 서빙 구성을 조회한다(이슈 #12).
/// 저장된 구성이 없으면 `None`을 반환하며(지어내지 않음, D22),
/// 프런트엔드는 이 정보를 기반으로 롤백 대상 안내 표시 및 버튼 활성화 여부를 결정한다.
#[tauri::command]
pub async fn get_last_known_good_serving(
    state: State<'_, MlxState>,
) -> Result<Option<ServingStatus>, String> {
    get_last_known_good_serving_inner(&state)
}

pub(crate) fn get_last_known_good_serving_inner(
    state: &MlxState,
) -> Result<Option<ServingStatus>, String> {
    let saved = state
        .last_known_good_serving
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    Ok(saved)
}

/// state.last_known_good_serving에서 되돌릴 구성을 꺼낸다. 없을 때 지어내지 않고
/// 명확한 에러를 반환하는 판정(D22)은 `services::mlx_serving_recovery::pick_revert_target`에
/// 있다(2026-09-23 리뷰로 이동) — 여기서는 Mutex를 잠그는 얇은 호출부만 담당한다.
fn revert_config_or_error(
    state: &MlxState,
    expected: &ServingStatus,
) -> Result<ServingStatus, String> {
    let saved = state
        .last_known_good_serving
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let target = services::mlx_serving_recovery::pick_revert_target(saved)?;
    if target.model_path != expected.model_path
        || target.adapter_path != expected.adapter_path
        || target.runtime != expected.runtime
    {
        return Err("The last known-good serving configuration changed. Review the updated target and try again.".to_string());
    }
    Ok(target)
}

/// `revert_to_last_serving`이 현재 서빙을 멈추기 전에 반드시 통과해야 하는 게이트다
/// (#12 리뷰 MED). `state.check_serving_admission()`(벤치마크 상호 배제, GitHub #11)이
/// Err면 이 함수도 Err를 반환하고 `state.serving`은 전혀 읽거나 쓰지 않은 채 끝난다 —
/// 호출부는 그 Err를 `?`로 즉시 전파해 `stop_model_serving`을 부르지 않으므로, admission이
/// 거부됐을 때 이미 정상 동작 중인 known-good 서빙까지 죽이는 일이 없다. 스폰
/// admission(`check_current_spawn_admission`, 메모리/발열 실측)은 AppHandle이 필요해
/// 여기 넣지 않고 호출부에서 이 함수 직후 같은 순서로 별도 호출한다.
fn revert_should_stop_current_serving(state: &MlxState) -> Result<bool, String> {
    state.check_serving_admission()?;
    let guard = state.serving.lock().map_err(|e| e.to_string())?;
    Ok(guard.is_some())
}

/// 저장된 last_known_good 구성으로 현재 서빙을 중지 후 재시작한다(이슈 #12).
/// 헬스체크를 통과했던 이전 서빙 구성(모델/어댑터/포트/런타임)으로 되돌린다.
/// 현재 서빙 중인 프로세스가 있으면 먼저 중지하고 재시작하며,
/// 저장된 구성이 없으면 `services::mlx_serving_recovery::pick_revert_target`에서
/// 지어내지 않고 에러를 반환한다(D22).
///
/// #12 리뷰(HIGH/MED, 2026-09-28) 이후:
/// - `MlxState::reverting_serving`을 `compare_exchange`로 선점해 동시 호출을 직렬화한다
///   (프런트도 확인 다이얼로그가 열리기 전부터 useRef로 중복 클릭을 막지만, 백엔드가
///   IPC 자체의 이중 호출까지 마지막 방어선으로 막는다).
/// - `start_model_serving`이 스폰 직전 거는 것과 같은 admission 게이트
///   (`check_serving_admission` + `check_current_spawn_admission`)를 **현재 서빙을 멈추기
///   전에** 그대로 재호출한다 — 로직을 복제하지 않고 같은 함수를 재사용하되 호출 순서만
///   앞당겨서, admission이 거부됐을 때 이미 죽여버린 known-good 서빙까지 잃지 않게 한다.
///   (이 재확인과 `start_model_serving` 내부 재확인 사이에는 여전히 검사~스폰 TOCTOU
///   윈도우가 남는다 — 이 파일의 다른 스폰 경로와 동일한, 문서화된 한계다.)
#[tauri::command]
pub async fn revert_to_last_serving(
    app: tauri::AppHandle,
    expected_model_path: String,
    expected_adapter_path: Option<String>,
    expected_runtime: MlxRuntime,
) -> Result<String, String> {
    let state = app.state::<MlxState>();
    let _revert_guard = state.claim_revert_reservation()?;
    let expected = ServingStatus {
        pid: 0,
        port: 0,
        model_path: expected_model_path,
        adapter_path: expected_adapter_path,
        runtime: expected_runtime,
    };
    let target = revert_config_or_error(&state, &expected)?;

    let should_stop = revert_should_stop_current_serving(&state)?;
    check_current_spawn_admission(&app.state::<crate::commands::guardrails::GuardrailState>())
        .await?;

    if should_stop {
        stop_model_serving(app.state::<MlxState>()).await?;
    }

    start_model_serving(
        app.clone(),
        app.state::<MlxState>(),
        target.model_path,
        target.adapter_path,
        target.port,
        Some(target.runtime),
    )
    .await
}

/// D45/D47: the Rust path always hands the wrapper a staging `--output-dir`; the wrapper has
/// no direct-write mode any more. Kept as a pure function so a test can pin the argument.
fn finetune_wrapper_args(
    wrapper: &Path,
    model_path: &Path,
    data_path: &Path,
    config: &FineTuneConfig,
    runtime: MlxRuntime,
    out_dir: &Path,
    mlflow_uri: &str,
) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = vec![
        wrapper.into(),
        "--model".into(),
        model_path.into(),
        "--data".into(),
        data_path.into(),
        "--iters".into(),
        config.iters.to_string().into(),
        "--batch-size".into(),
        config.batch_size.to_string().into(),
        "--learning-rate".into(),
        config.learning_rate.to_string().into(),
        "--adapter-name".into(),
        (&config.adapter_name).into(),
        "--output-dir".into(),
        out_dir.into(),
        "--runtime".into(),
        match runtime {
            MlxRuntime::MlxLm => "mlx-lm",
            MlxRuntime::MlxVlm => "mlx-vlm",
        }
        .into(),
        // MLflow 주소를 명시로 넘긴다. 넘기지 않으면 래퍼가 자기 기본값(5001 고정)을
        // 쓰는데, 포트는 런타임 값이라(D1 개정) 5001이 점유되면 학습 기록이 통째로
        // 엉뚱한 곳으로 간다.
        "--mlflow-uri".into(),
        mlflow_uri.into(),
    ];
    if config.train_vision {
        args.push("--train-vision".into());
    }
    args
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
    fn direct_finetune_args_pass_output_dir() {
        let config = FineTuneConfig {
            model_path: "m".into(),
            data_path: "d".into(),
            iters: 1,
            batch_size: 1,
            learning_rate: 0.001,
            adapter_name: "a".into(),
            runtime: None,
            train_vision: true,
        };
        for runtime in [MlxRuntime::MlxLm, MlxRuntime::MlxVlm] {
            let args = finetune_wrapper_args(
                Path::new("w.py"),
                Path::new("m"),
                Path::new("d"),
                &config,
                runtime,
                Path::new("/stage/out"),
                "http://127.0.0.1:5001",
            );
            let pos = args
                .iter()
                .position(|a| a == "--output-dir")
                .expect("--output-dir");
            assert_eq!(args[pos + 1], "/stage/out");
        }
    }

    #[test]
    fn orphan_termination_requires_matching_process_start_time() {
        assert!(marker_start_time_matches(Some(42), Some(42)));
        assert!(!marker_start_time_matches(Some(42), Some(43)));
        assert!(!marker_start_time_matches(None, Some(42)));
    }

    #[test]
    fn training_termination_falls_back_to_pid_when_no_process_group_exists() {
        assert_eq!(resolve_training_signal_target(123, false), Some(123));
        assert_eq!(resolve_training_signal_target(123, true), Some(-123));
        assert_eq!(resolve_training_signal_target(0, false), None);
    }

    #[test]
    fn tracked_mlx_process_decision_allows_only_current_training_or_serving_pid() {
        assert_eq!(
            tracked_mlx_process_is_training(41, Some(41), Some(42)),
            Some(true)
        );
        assert_eq!(
            tracked_mlx_process_is_training(42, Some(41), Some(42)),
            Some(false)
        );
    }

    #[test]
    fn tracked_mlx_process_decision_refuses_untracked_pid() {
        assert_eq!(
            tracked_mlx_process_is_training(43, Some(41), Some(42)),
            None
        );
        assert_eq!(tracked_mlx_process_is_training(0, Some(41), Some(42)), None);
    }

    /// 경합: 서빙이 이미 종료돼 reaper가 state를 비운 뒤 사용자가 Stop을 눌렀다 — pid가
    /// 죽어 있으면 오류가 아니라 이미 멈춘 것으로 처리해야 한다(GitHub #13 MED).
    #[test]
    fn untracked_pid_stop_outcome_treats_dead_pid_as_already_stopped() {
        assert_eq!(untracked_pid_stop_outcome(4242, false), Ok(true));
    }

    /// 추적되지 않는데 아직 살아있는 pid는 권한 밖이므로 계속 거부해야 한다(D22).
    #[test]
    fn untracked_pid_stop_outcome_refuses_live_pid() {
        let err = untracked_pid_stop_outcome(4242, true).unwrap_err();
        assert!(err.contains("4242"));
        assert!(err.contains("not the currently tracked"));
    }

    /// kill_mlx_process가 시그널 전에 낙관적으로 쓴 "killed"만 되돌려야 한다 — 다른 종착
    /// 상태(done/error)를 시그널 실패로 덮어쓰면 안 된다(GitHub #13 LOW).
    #[test]
    fn should_revert_optimistic_killed_status_only_reverts_killed() {
        assert!(should_revert_optimistic_killed_status("killed"));
        assert!(!should_revert_optimistic_killed_status("done"));
        assert!(!should_revert_optimistic_killed_status("error"));
        assert!(!should_revert_optimistic_killed_status("running"));
    }

    #[tokio::test]
    async fn surviving_training_group_requires_preterm_identity_witness() {
        assert!(surviving_training_group_witness(std::process::id(), &[])
            .await
            .is_none());
        assert!(
            surviving_training_group_witness(std::process::id(), &[(std::process::id(), 0)])
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn orphan_training_stop_kills_verified_child_after_leader_exits() {
        exercise_orphan_training_group(true).await;
    }

    #[tokio::test]
    async fn orphan_training_stop_refuses_unverified_surviving_child() {
        exercise_orphan_training_group(false).await;
    }

    async fn exercise_orphan_training_group(child_is_mlx: bool) {
        use std::os::unix::process::CommandExt;
        let root = std::env::temp_dir().join(format!(
            "kubemetal group stop-{child_is_mlx}-{}",
            std::process::id()
        ));
        let script = root.join("scripts/mlx/finetune_wrapper.py");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        // Only this test's dedicated process group is signaled. Child initialization
        // is acknowledged before the leader writes its PID file.
        std::fs::write(&script, "import os,signal,sys,time\npid=os.fork()\nif pid == 0:\n signal.signal(signal.SIGTERM,signal.SIG_IGN)\n open(sys.argv[1]+'.ready','w').close()\n if sys.argv[2] == 'unverified': os.execl('/bin/sleep','sleep','30')\n time.sleep(30)\nelse:\n while not os.path.exists(sys.argv[1]+'.ready'): time.sleep(.01)\n open(sys.argv[1],'w').write(str(pid))\n time.sleep(30)\n").unwrap();
        let pid_file = root.join("child.pid");
        let mut leader = std::process::Command::new(resolve_cli_path("python3").unwrap())
            .arg(&script)
            .arg(&pid_file)
            .arg(if child_is_mlx { "mlx" } else { "unverified" })
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = leader.id();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        // Existence is not completion: python creates the file empty before writing
        // the pid, so poll until the content parses.
        let child_pid: u32 = loop {
            if let Some(n) = std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|s| s.trim().parse().ok())
            {
                break n;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "child pid was not written in time"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        };
        let start_time = services::process::process_start_time(pid).unwrap();
        let child_start_time = services::process::process_start_time(child_pid).unwrap();
        assert!(terminate_pid(pid, true, Some(start_time + 1))
            .await
            .is_err());
        assert!(services::process::pid_is_alive(pid));
        assert!(services::process::pid_is_alive(child_pid));
        // Reap the leader during terminate_pid's wait, as the app's reader does.
        let reaper = tokio::task::spawn_blocking(move || leader.wait().unwrap());
        let result = terminate_pid(pid, true, Some(start_time)).await;
        let child_cmdline = services::process::get_process_cmdline(child_pid)
            .await
            .unwrap_or_default();
        let exited = !child_cmdline.contains("finetune_wrapper.py");
        // Cleanup is limited to the group created above even when the assertion fails.
        let child_still_alive =
            services::process::process_still_in_group(child_pid, child_start_time, pid);
        if child_still_alive {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
        let _ = reaper.await;
        let _ = std::fs::remove_dir_all(root);
        if child_is_mlx {
            assert!(result.is_ok(), "{result:?}");
            assert!(exited, "SIGTERM-resistant training child survived");
        } else {
            assert!(result.is_err(), "unverified group was authorized");
            assert!(
                child_still_alive,
                "unverified child was signaled with SIGKILL"
            );
        }
    }

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
        for paused in [
            "paused",
            "paused_memory_pressure",
            "paused_battery",
            "paused_thermal",
        ] {
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
        assert_eq!(
            preferred, 8080,
            "D1 assigns 8080 as the preferred serving port"
        );
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
    fn is_quantized_model_detects_top_level_quantization_config_field() {
        // transformers 계열(bitsandbytes/GPTQ/AWQ) 표기 — 이 기기에서 실사례는 못
        // 찾았지만(위 is_quantized_model 문서 참고) 2026-09-23 리뷰가 요구한 방어적
        // 커버리지다.
        let dir = make_temp_model_dir("quantization-config-top-level");
        std::fs::write(
            dir.join("config.json"),
            r#"{"model_type":"llama","quantization_config":{"quant_method":"bitsandbytes"}}"#,
        )
        .unwrap();
        assert_eq!(is_quantized_model(&dir), Some(true));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn is_quantized_model_detects_text_config_quantization_config_field() {
        // VLM에서 vision 스택은 그대로 두고 텍스트 백본만 양자화된 구성을 흉내낸다.
        let dir = make_temp_model_dir("quantization-config-text-config");
        std::fs::write(
            dir.join("config.json"),
            r#"{"model_type":"qwen2_vl","text_config":{"quantization_config":{"quant_method":"gptq"}}}"#,
        )
        .unwrap();
        assert_eq!(is_quantized_model(&dir), Some(true));
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
        let expected = dummy_serving_status(0);
        let err =
            revert_config_or_error(&state, &expected).expect_err("되돌릴 이전 구성이 없습니다");
        assert!(err.contains("되돌릴 이전 구성이 없습니다"));
    }

    #[test]
    fn revert_config_or_error_returns_saved_config() {
        let state = MlxState::default();
        *state.last_known_good_serving.lock().unwrap() = Some(dummy_serving_status(42));

        let expected = dummy_serving_status(0);
        let got =
            revert_config_or_error(&state, &expected).expect("saved config should be returned");

        assert_eq!(got.pid, 42);
    }

    #[test]
    fn revert_config_or_error_refuses_changed_confirmation_before_serving_mutation() {
        let state = MlxState::default();
        let saved = dummy_serving_status(42);
        *state.serving.lock().unwrap() = Some(dummy_serving_status(7));
        *state.last_known_good_serving.lock().unwrap() = Some(saved);
        let mut stale_confirmation = dummy_serving_status(0);
        stale_confirmation.model_path.push_str("-stale");

        let error = revert_config_or_error(&state, &stale_confirmation)
            .expect_err("stale confirmation must be refused");

        assert!(error.contains("configuration changed"));
        assert_eq!(state.serving.lock().unwrap().as_ref().unwrap().pid, 7);
    }

    #[test]
    fn matching_revert_confirmation_proceeds_to_admission() {
        let state = MlxState::default();
        let saved = dummy_serving_status(42);
        *state.last_known_good_serving.lock().unwrap() = Some(saved.clone());

        let target = revert_config_or_error(&state, &saved).expect("matching target accepted");

        assert_eq!(target.pid, 42);
        assert_eq!(revert_should_stop_current_serving(&state), Ok(false));
    }

    #[test]
    fn get_last_known_good_serving_returns_none_when_empty() {
        let state = MlxState::default();
        let result = get_last_known_good_serving_inner(&state).expect("getter should succeed");
        assert!(result.is_none());
    }

    #[test]
    fn get_last_known_good_serving_returns_saved_config() {
        let state = MlxState::default();
        *state.last_known_good_serving.lock().unwrap() = Some(dummy_serving_status(42));

        let result = get_last_known_good_serving_inner(&state).expect("getter should succeed");
        assert_eq!(result.map(|s| s.pid), Some(42));
    }

    /// #12 리뷰 HIGH: 확인 다이얼로그가 열려 있는 동안 또는 첫 요청이 아직 끝나기 전에
    /// 두 번째 `revert_to_last_serving` IPC 호출이 겹치면 서빙이 이중으로 stop/restart된다.
    /// `claim_revert_reservation`의 `compare_exchange`가 이를 막는다 — 이 가드(선점 검사)를
    /// 지우면(예: 항상 `Ok`를 반환하도록 바꾸면) 이 테스트는 실패한다(2026-09-28 확인 후 복구).
    #[test]
    fn claim_revert_reservation_refuses_concurrent_revert() {
        let state = MlxState::default();
        let _first = state
            .claim_revert_reservation()
            .expect("first revert claim should succeed");

        let err = state
            .claim_revert_reservation()
            .expect_err("a second revert while one is in flight must be refused");
        assert!(err.contains("already in progress"));

        drop(_first);
        assert!(
            state.claim_revert_reservation().is_ok(),
            "reservation must be claimable again once the in-flight guard drops"
        );
    }

    /// #12 리뷰 MED: 되돌리기가 현재 서빙을 멈춘 뒤에야 admission을 확인하면, admission이
    /// 거부됐을 때 이미 정상 동작 중이던 known-good 서빙까지 죽인 채로 끝난다.
    /// `revert_should_stop_current_serving`이 `state.serving`을 건드리기 전에 admission부터
    /// 확인하는 게이트다 — 이 함수에서 `state.check_serving_admission()?`을 지우면(admission을
    /// 건너뛰면) 이 테스트는 실패한다(2026-09-28 확인 후 복구).
    #[test]
    fn revert_should_stop_current_serving_refuses_when_benchmark_active() {
        let state = MlxState::default();
        *state.serving.lock().unwrap() = Some(dummy_serving_status(7));
        let _benchmark_guard = state
            .claim_benchmark_reservation()
            .expect("benchmark reservation should succeed");

        let err = revert_should_stop_current_serving(&state)
            .expect_err("revert admission must be refused while a GPU benchmark is running");
        assert!(err.contains("GPU benchmark"));

        assert!(
            state.serving.lock().unwrap().is_some(),
            "admission이 거부됐는데 서빙 슬롯이 비워졌다 — stop이 admission 확인보다 먼저 실행된 회귀다"
        );
    }

    #[test]
    fn revert_should_stop_current_serving_reports_true_when_admitted_and_serving() {
        let state = MlxState::default();
        *state.serving.lock().unwrap() = Some(dummy_serving_status(7));
        assert_eq!(revert_should_stop_current_serving(&state), Ok(true));
    }

    #[test]
    fn revert_should_stop_current_serving_reports_false_when_nothing_serving() {
        let state = MlxState::default();
        assert_eq!(revert_should_stop_current_serving(&state), Ok(false));
    }

    #[test]
    fn last_known_good_serving_fails_closed_when_mutex_poisoned() {
        let state = std::sync::Arc::new(MlxState::default());
        let clone = state.clone();
        let _ = std::panic::catch_unwind(move || {
            let _lock = clone.last_known_good_serving.lock().unwrap();
            panic!("force poison");
        });
        assert!(get_last_known_good_serving_inner(&state).is_err());
        assert!(revert_config_or_error(&state, &dummy_serving_status(0)).is_err());
    }

    // write_training_manifest_records_matching_sha256(원본 rescue 테스트)는 포트하지 않는다:
    // 재랜드 시점의 main은 매니페스트 기록을 이미 `write_manifest`/`ManifestContext`로
    // 통합했고, 그 계약(정렬·sha256·자기 제외)은 artifact_manifest.rs의 자체 테스트
    // (`writes_sorted_manifest_with_sha256_and_excludes_itself`)가 이미 고정하고 있다 —
    // 같은 사실을 이 파일에서 다시 고정하지 않는다.

    fn manifest_context() -> ManifestContext {
        ManifestContext {
            runtime: "mlx-lm".into(),
            base_model: "/base".into(),
        }
    }

    #[test]
    fn manifest_verification_status_returns_missing_without_manifest() {
        let dir = make_temp_model_dir("manifest-missing");
        assert_eq!(manifest_verification_status(&dir), "missing");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn manifest_verification_status_returns_verified_when_hashes_match() {
        let dir = make_temp_model_dir("manifest-verified");
        std::fs::write(dir.join("adapters.safetensors"), b"weights").unwrap();
        write_manifest(&dir, manifest_context()).expect("manifest write should succeed");
        assert_eq!(manifest_verification_status(&dir), "verified");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn manifest_verification_status_returns_corrupt_when_hash_mismatches() {
        let dir = make_temp_model_dir("manifest-corrupt");
        std::fs::write(dir.join("adapters.safetensors"), b"weights").unwrap();
        write_manifest(&dir, manifest_context()).expect("manifest write should succeed");
        // 학습 산출물이 매니페스트 작성 이후 손상된 상황을 재현한다.
        std::fs::write(dir.join("adapters.safetensors"), b"tampered").unwrap();
        assert_eq!(manifest_verification_status(&dir), "corrupt");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn adapter_deletion_home_canonicalizes_symlink() {
        let root = make_temp_model_dir("deletion-home-alias");
        let real_home = root.join("home");
        let alias = root.join("alias");
        std::fs::create_dir(&real_home).unwrap();
        std::os::unix::fs::symlink(&real_home, &alias).unwrap();
        let expected = real_home.canonicalize().unwrap();
        let actual = adapter_deletion_home(Ok(alias));
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(actual, Some(expected));
    }

    #[test]
    fn adapter_deletion_home_preserves_unresolved_path_and_lookup_failure() {
        let root = make_temp_model_dir("deletion-home-missing");
        let missing = root.join("missing");
        let actual = adapter_deletion_home(Ok(missing.clone()));
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(actual, Some(missing));
        assert_eq!(adapter_deletion_home(Err("HOME unavailable".into())), None);
    }

    fn deletion_fixture(name: &str) -> (PathBuf, PathBuf) {
        let home = make_temp_model_dir(name);
        let adapters = home.join(".kubemetal/adapters");
        std::fs::create_dir_all(&adapters).unwrap();
        (home, adapters)
    }

    #[test]
    fn delete_adapter_checkpoint_removes_only_a_safe_adapter_directory() {
        let (home, adapters) = deletion_fixture("delete-adapter-happy");
        let target = adapters.join("ready");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("weights"), b"checkpoint").unwrap();
        remove_adapter_checkpoint(
            target.to_str().unwrap(),
            Ok(home.clone()),
            &MlxState::default(),
        )
        .unwrap();
        assert!(!target.exists());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn delete_adapter_checkpoint_refuses_traversal_absolute_outside_root_and_root() {
        let (home, adapters) = deletion_fixture("delete-adapter-boundaries");
        let outside = home.join("outside");
        std::fs::create_dir(&outside).unwrap();
        let state = MlxState::default();
        assert!(remove_adapter_checkpoint(
            &format!("{}/../outside", adapters.display()),
            Ok(home.clone()),
            &state
        )
        .is_err());
        assert!(
            remove_adapter_checkpoint(outside.to_str().unwrap(), Ok(home.clone()), &state).is_err()
        );
        assert!(
            remove_adapter_checkpoint(adapters.to_str().unwrap(), Ok(home.clone()), &state)
                .is_err()
        );
        assert!(outside.exists());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn delete_adapter_checkpoint_refuses_symlinks_into_and_out_of_adapters_root() {
        let (home, adapters) = deletion_fixture("delete-adapter-symlink");
        let real = adapters.join("real");
        let outside = home.join("elsewhere");
        let inside_alias = adapters.join("inside-alias");
        let intermediate_alias = adapters.join("intermediate-alias");
        let nested_target = adapters.join("nested").join("target");
        let outside_alias = adapters.join("outside-alias");
        std::fs::create_dir(&real).unwrap();
        std::fs::create_dir_all(&nested_target).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&real, &inside_alias).unwrap();
        std::os::unix::fs::symlink(adapters.join("nested"), &intermediate_alias).unwrap();
        std::os::unix::fs::symlink(&outside, &outside_alias).unwrap();
        let state = MlxState::default();
        assert!(remove_adapter_checkpoint(
            inside_alias.to_str().unwrap(),
            Ok(home.clone()),
            &state
        )
        .is_err());
        assert!(remove_adapter_checkpoint(
            &format!("{}/", inside_alias.display()),
            Ok(home.clone()),
            &state
        )
        .is_err());
        assert!(remove_adapter_checkpoint(
            intermediate_alias.join("target").to_str().unwrap(),
            Ok(home.clone()),
            &state
        )
        .is_err());
        assert!(remove_adapter_checkpoint(
            outside_alias.to_str().unwrap(),
            Ok(home.clone()),
            &state
        )
        .is_err());
        assert!(real.exists() && outside.exists());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn delete_adapter_checkpoint_requires_absolute_path() {
        let (home, _) = deletion_fixture("delete-adapter-relative");
        assert!(remove_adapter_checkpoint(
            "adapters/relative",
            Ok(home.clone()),
            &MlxState::default()
        )
        .is_err());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn adapter_deletion_admission_lock_serializes_start_with_deletion() {
        let state = std::sync::Arc::new(MlxState::default());
        let held = state.adapter_admission.lock().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker_state = state.clone();
        let worker = std::thread::spawn(move || {
            let _admission = worker_state.adapter_admission.lock().unwrap();
            tx.send(()).unwrap();
        });
        assert!(rx
            .recv_timeout(std::time::Duration::from_millis(50))
            .is_err());
        drop(held);
        rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn delete_adapter_checkpoint_refuses_serving_and_last_known_good_adapters() {
        for protected in ["serving", "last-good"] {
            let (home, adapters) = deletion_fixture(&format!("delete-adapter-{protected}"));
            let target = adapters.join("protected");
            std::fs::create_dir(&target).unwrap();
            let state = MlxState::default();
            let serving = ServingStatus {
                pid: 1,
                port: 8080,
                model_path: "/model".into(),
                adapter_path: Some(target.to_string_lossy().into()),
                runtime: MlxRuntime::MlxLm,
            };
            if protected == "serving" {
                *state.serving.lock().unwrap() = Some(serving);
            } else {
                *state.last_known_good_serving.lock().unwrap() = Some(serving);
            }
            assert!(
                remove_adapter_checkpoint(target.to_str().unwrap(), Ok(home.clone()), &state)
                    .is_err()
            );
            assert!(target.exists());
            std::fs::remove_dir_all(home).unwrap();
        }
    }

    #[test]
    fn delete_adapter_checkpoint_refuses_adapter_being_trained() {
        let name = format!("delete-in-progress-{}", std::process::id());
        let (fixture_home, adapters) = deletion_fixture("delete-adapter-training");
        let target = adapters.join(&name);
        std::fs::create_dir(&target).unwrap();
        let state = MlxState::default();
        *state.training.lock().unwrap() = Some(TrainingStatus {
            pid: 12,
            status: "running".into(),
            current_iter: 1,
            total_iters: 2,
            last_loss: None,
            adapter_path: None,
            error: None,
            adapter_name: name,
            mlflow_run_id: None,
        });
        assert!(remove_adapter_checkpoint(
            target.to_str().unwrap(),
            Ok(fixture_home.clone()),
            &state
        )
        .is_err());
        assert!(target.exists());
        std::fs::remove_dir_all(fixture_home).unwrap();
    }

    #[test]
    fn is_adapter_safe_to_delete_forbids_currently_serving_adapter() {
        let dir = make_temp_model_dir("safe-delete-serving");
        let state = MlxState::default();
        *state.serving.lock().unwrap() = Some(ServingStatus {
            pid: 1,
            port: 8080,
            model_path: "/models/base".into(),
            adapter_path: Some(dir.to_string_lossy().to_string()),
            runtime: MlxRuntime::MlxLm,
        });
        assert!(!is_adapter_safe_to_delete(&dir, &state, home_dir()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn is_adapter_safe_to_delete_forbids_case_variant_serving_adapter_path() {
        let (home, adapters) = deletion_fixture("safe-delete-case-variant");
        let dir = adapters.join("My-Lora");
        std::fs::create_dir(&dir).unwrap();
        let state = MlxState::default();
        *state.serving.lock().unwrap() = Some(ServingStatus {
            pid: 1,
            port: 8080,
            model_path: "/models/base".into(),
            adapter_path: Some(adapters.join("MY-LORA").to_string_lossy().into()),
            runtime: MlxRuntime::MlxLm,
        });
        assert!(!is_adapter_safe_to_delete(&dir, &state, Ok(home.clone())));
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn is_adapter_safe_to_delete_forbids_last_known_good_adapter() {
        let dir = make_temp_model_dir("safe-delete-lkg");
        let state = MlxState::default();
        *state.last_known_good_serving.lock().unwrap() = Some(ServingStatus {
            pid: 2,
            port: 8080,
            model_path: "/models/base".into(),
            adapter_path: Some(dir.to_string_lossy().to_string()),
            runtime: MlxRuntime::MlxLm,
        });
        assert!(!is_adapter_safe_to_delete(&dir, &state, home_dir()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn is_adapter_safe_to_delete_allows_unrelated_adapter() {
        let dir = make_temp_model_dir("safe-delete-unrelated");
        let other_dir = make_temp_model_dir("safe-delete-other");
        let state = MlxState::default();
        *state.serving.lock().unwrap() = Some(ServingStatus {
            pid: 3,
            port: 8080,
            model_path: "/models/base".into(),
            adapter_path: Some(other_dir.to_string_lossy().to_string()),
            runtime: MlxRuntime::MlxLm,
        });
        assert!(is_adapter_safe_to_delete(&dir, &state, home_dir()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&other_dir).ok();
    }

    #[test]
    fn is_adapter_safe_to_delete_forbids_symlink_of_serving_adapter() {
        let root = make_temp_model_dir("safe-delete-symlink");
        let dir = root.join("adapter");
        let alias = root.join("alias");
        std::fs::create_dir(&dir).unwrap();
        std::os::unix::fs::symlink(&dir, &alias).unwrap();
        let state = MlxState::default();
        *state.serving.lock().unwrap() = Some(ServingStatus {
            pid: 9,
            port: 8080,
            model_path: "/models/base".into(),
            adapter_path: Some(alias.to_string_lossy().to_string()),
            runtime: MlxRuntime::MlxLm,
        });
        assert!(!is_adapter_safe_to_delete(&dir, &state, home_dir()));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn is_adapter_safe_to_delete_forbids_missing_training_output_even_after_training_ends() {
        // TrainingStatus.adapter_path는 "done"에서만 채워진다(mlx.rs:459 인근) — 진행
        // 중인 학습은 그 필드가 비어 있으므로, adapter_name으로 출력 디렉터리를 역산해서
        // 판별해야 한다. 출력 생성 전에는 대상 canonicalize 실패로도 삭제를 거부한다.
        let home = home_dir().expect("HOME must be set for this test");
        let adapter_name = format!(
            "reland-review-test-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let in_progress_dir = services::mlx_artifacts::adapter_output_dir(&home, &adapter_name);

        let state = MlxState::default();
        *state.training.lock().unwrap() = Some(TrainingStatus {
            pid: 123,
            status: "running".into(),
            current_iter: 1,
            total_iters: 10,
            last_loss: None,
            adapter_path: None, // 아직 done이 아니다 — 이 테스트의 전제.
            error: None,
            adapter_name,
            mlflow_run_id: None,
        });

        assert!(!is_adapter_safe_to_delete(
            &in_progress_dir,
            &state,
            home_dir()
        ));

        // D-b: 학습이 끝나도 없는 대상은 거부한다. 실제 출력의 보호 해제는 서비스 테스트가 검증한다.
        *state.training.lock().unwrap() = None;
        assert!(!is_adapter_safe_to_delete(
            &in_progress_dir,
            &state,
            home_dir()
        ));
    }

    #[test]
    fn is_adapter_safe_to_delete_fails_closed_when_serving_lock_is_poisoned() {
        // 예전 구현은 `.lock().ok().unwrap_or(false)`를 거쳐 poison된 잠금을 "보호 없음"
        // 으로 읽고 최종적으로 "안전"을 반환했다 — 무엇을 보호해야 하는지 모르는 상태를
        // "안전하다"로 지어내면 안 된다(D22, fail-closed). std::panic::catch_unwind로
        // 잠금을 쥔 채 panic시켜 poison을 재현한다(스레드를 새로 띄울 필요는 없다 —
        // poison은 "그 락을 쥔 채 unwind"로 발생하고, catch_unwind는 그 unwind를 같은
        // 스레드 안에서 안전하게 가둔다).
        let dir = make_temp_model_dir("safe-delete-poisoned");
        let state = MlxState::default();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = state.serving.lock().unwrap();
            panic!("intentionally poison the serving lock for this test");
        }));
        assert!(state.serving.is_poisoned());

        assert!(!is_adapter_safe_to_delete(&dir, &state, home_dir()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_signal_target_never_signals_pid_zero() {
        assert_eq!(resolve_signal_target(0, false), None);
        assert_eq!(resolve_signal_target(0, true), None);
    }

    #[test]
    fn resolve_signal_target_handles_single_process_and_process_group() {
        assert_eq!(resolve_signal_target(1234, false), Some(1234));
        assert_eq!(resolve_signal_target(1234, true), Some(-1234));
    }

    #[tokio::test]
    async fn terminate_pid_noop_on_zero() {
        // PID 0은 시그널 전송이나 sleep 대기 없이 즉시 no-op으로 반환되어야 한다.
        assert!(terminate_pid(0, false, None).await.is_ok());
        assert!(terminate_pid(0, true, None).await.is_ok());
    }
}
