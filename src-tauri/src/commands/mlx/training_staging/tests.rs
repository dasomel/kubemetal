use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use super::*;
use crate::services::adapter_staging::{AttemptSpec, StagingError};

static NONCE: AtomicU64 = AtomicU64::new(0);
struct TempHome(PathBuf);
impl TempHome {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "kubemetal-s2-{}-{}",
            std::process::id(),
            NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(home.join(".kubemetal")).unwrap();
        Self(home)
    }
    fn root(&self) -> PathBuf {
        self.0.join(".kubemetal")
    }
}
impl Drop for TempHome {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn spec() -> AttemptSpec {
    AttemptSpec {
        adapter_name: "s2".into(),
        runtime: "mlx-lm".into(),
        base_model: "/base".into(),
        iters: 1,
        mlflow_run_id: None,
    }
}
fn training() -> TrainingStatus {
    TrainingStatus {
        pid: 42,
        status: "running".into(),
        current_iter: 0,
        total_iters: 1,
        last_loss: None,
        adapter_path: None,
        error: None,
        adapter_name: "s2".into(),
        mlflow_run_id: None,
    }
}
fn ready(home: &TempHome) -> (MlxState, Attempt) {
    let mut attempt = adapter_staging::create_attempt(&home.root(), &spec()).unwrap();
    adapter_staging::mark_running(&mut attempt, 42, Some(123)).unwrap();
    fs::write(attempt.out_dir().join("adapters.safetensors"), b"weights").unwrap();
    fs::write(attempt.out_dir().join("adapter_config.json"), b"{}").unwrap();
    let state = MlxState::default();
    *state.training.lock().unwrap() = Some(training());
    *state.training_stop.lock().unwrap() = Some((42, Arc::new(AtomicBool::new(false))));
    (state, attempt)
}
fn done(attempt: &Attempt, exit_code: i32) -> TrainingExit {
    let mut report = CompletionReport::default();
    report.observe(
        &serde_json::json!({"type":"done", "adapter_path":attempt.out_dir()}),
        &attempt.out_dir(),
    );
    TrainingExit {
        pid: 42,
        exit: Ok(ExitStatus::from_raw(exit_code << 8)),
        report,
        stderr: String::new(),
        setup_error: None,
    }
}
fn persisted(attempt: &Attempt) -> AttemptState {
    let record: adapter_staging::AttemptRecord =
        serde_json::from_slice(&fs::read(attempt.dir().join("attempt.json")).unwrap()).unwrap();
    record.state
}
fn assert_failed(state: &MlxState, attempt: &Attempt, expected: AttemptState) {
    let slot = state.training.lock().unwrap();
    let status = slot.as_ref().unwrap();
    assert_eq!(
        status.status,
        if expected == AttemptState::Killed {
            "killed"
        } else {
            "error"
        }
    );
    assert!(status.adapter_path.is_none());
    assert_eq!(persisted(attempt), expected);
    assert!(attempt.out_dir().exists());
    assert!(!attempt.final_path().exists());
}

mod file_operations;
mod safety;

fn active_stop(state: &MlxState) -> Arc<AtomicBool> {
    state
        .training_stop
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .1
        .clone()
}
