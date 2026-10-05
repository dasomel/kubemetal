//! D45 training completion boundary. Filesystem work runs on spawn_blocking;
//! the child report is evidence, never authority to publish a checkpoint.
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

use super::{MlxState, TrainingStatus};
use crate::services::adapter_staging::{self, Attempt, AttemptState};
use crate::services::mlx_lifecycle::{self, MlflowRunReconciliation};

#[derive(Default)]
pub(super) struct CompletionReport {
    path: Option<String>,
    error: Option<String>,
    terminal: bool,
}

impl CompletionReport {
    pub(super) fn observe(&mut self, event: &Value, expected: &Path) {
        match event.get("type").and_then(Value::as_str) {
            Some("done") => {
                self.terminal = true;
                let path = event.get("adapter_path").and_then(Value::as_str);
                if path.is_none() || path != expected.to_str() || self.path.is_some() {
                    self.error =
                        Some("Training reported an unexpected or duplicate output path".into());
                } else {
                    self.path = path.map(str::to_owned);
                }
            }
            Some("error") => {
                self.terminal = true;
                self.error = Some(
                    event
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("Training wrapper reported an error")
                        .into(),
                );
            }
            _ => {}
        }
    }

    fn matches_expected(&self, expected: &Path) -> bool {
        self.path.is_some() && self.path.as_deref() == expected.to_str()
    }

    pub(super) fn read_error(&mut self, error: String) {
        self.error = Some(error);
    }
}

pub(super) fn apply_event(training: &mut TrainingStatus, value: &Value) {
    // The same child's late run ID is still needed for MLflow kill reconciliation;
    // progress/done must not alter a terminal slot.
    if !mlx_lifecycle::is_non_terminal_training_status(&training.status)
        && value.get("type").and_then(Value::as_str) != Some("mlflow_run_started")
    {
        return;
    }
    match value.get("type").and_then(Value::as_str) {
        Some("progress") => {
            if let Some(i) = value.get("iter").and_then(Value::as_u64) {
                training.current_iter = i as u32;
            }
            if let Some(loss) = value.get("train_loss").and_then(Value::as_f64) {
                training.last_loss = Some(loss);
            }
        }
        Some("done") => {
            // Only finalize may publish done/path; even a valid child report precedes hashing.
            if let Some(loss) = value.get("last_loss").and_then(Value::as_f64) {
                training.last_loss = Some(loss);
            }
        }
        Some("mlflow_run_started") => {
            if let Some(id) = value.get("run_id").and_then(Value::as_str) {
                training.mlflow_run_id = Some(id.into());
            }
        }
        _ => {}
    }
}

pub(super) fn request_stop(state: &MlxState, pid: u32) -> Result<(), String> {
    let mut slot = state.training.lock().map_err(|e| e.to_string())?;
    if let Some(training) = slot
        .as_mut()
        .filter(|t| t.pid == pid && mlx_lifecycle::is_non_terminal_training_status(&t.status))
    {
        if let Some((stop_pid, stopped)) = state
            .training_stop
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
        {
            if *stop_pid == pid {
                stopped.store(true, Ordering::SeqCst);
            }
        }
        training.status = "killed".into();
    }
    Ok(())
}

// Caller holds the training slot lock, so finalize cannot consume an optimistic
// stop while the signal-error path restores its error outcome.
pub(super) fn clear_stop_intent(state: &MlxState, pid: u32) -> Result<(), String> {
    if let Some((stop_pid, stopped)) = state
        .training_stop
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
    {
        if *stop_pid == pid {
            stopped.store(false, Ordering::SeqCst);
        }
    }
    Ok(())
}

pub(super) fn spawn(
    cmd: &mut tokio::process::Command,
    attempt: &Attempt,
) -> Result<tokio::process::Child, String> {
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PATH", crate::services::process::augmented_path())
        .process_group(0)
        .spawn()
        .map_err(|e| {
            // D45: only this exclusive attempt is owned. rmdir refuses unexpected content;
            // shared staging parents and all older attempts remain untouched (no TTL).
            if let Err(cleanup) = cleanup_unspawned(attempt) {
                eprintln!("[mlx] {cleanup}");
            }
            format!("Failed to launch fine-tuning process: {e}")
        })
}

fn cleanup_unspawned(attempt: &Attempt) -> Result<(), String> {
    if attempt.record().state != AttemptState::Created {
        return Err("Attempt already started; staging kept".into());
    }
    std::fs::remove_dir(attempt.out_dir()).map_err(|e| format!("Staging out kept: {e}"))?;
    std::fs::remove_file(attempt.dir().join("attempt.json"))
        .map_err(|e| format!("Staging record kept: {e}"))?;
    std::fs::remove_dir(attempt.dir()).map_err(|e| format!("Staging attempt kept: {e}"))
}

pub(super) struct TrainingExit {
    pub pid: u32,
    pub exit: std::io::Result<ExitStatus>,
    pub report: CompletionReport,
    pub stderr: String,
    pub setup_error: Option<String>,
}

pub(super) fn finalize(
    state: &MlxState,
    attempt: &mut Attempt,
    stopped: &AtomicBool,
    outcome: TrainingExit,
) -> Result<Option<MlflowRunReconciliation>, String> {
    // D45: same lock and ordering as remove_adapter_checkpoint. Publish final path/status
    // before releasing it, so deletion cannot race verification -> rename -> slot update.
    let admission = state.adapter_admission.lock().map_err(|e| e.to_string())?;
    let mut slot = state.training.lock().map_err(|e| e.to_string())?;
    // The per-attempt stop object also binds slot ownership: a reused PID alone
    // must not let an older reader rewrite a newer run. This grants no signal authority.
    let same_attempt = state
        .training_stop
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        .is_some_and(|(pid, stop)| *pid == outcome.pid && std::ptr::eq(stopped, stop.as_ref()));
    let training = slot
        .as_mut()
        .filter(|t| t.pid == outcome.pid && same_attempt);
    let reconciliation = training.as_ref().and_then(|t| {
        mlx_lifecycle::mlflow_reconciliation_decision(
            &t.status,
            Some(t.pid),
            outcome.pid,
            t.mlflow_run_id.as_deref(),
            outcome.report.terminal,
            outcome.exit.as_ref().ok(),
        )
    });
    let killed =
        stopped.load(Ordering::SeqCst) || training.as_ref().is_some_and(|t| t.status == "killed");
    let result = (|| -> Result<_, String> {
        if killed {
            return Err("Training was stopped; staged output kept".into());
        }
        if training
            .as_ref()
            .is_none_or(|t| !mlx_lifecycle::is_non_terminal_training_status(&t.status))
        {
            return Err(training
                .as_ref()
                .and_then(|t| t.error.clone())
                .unwrap_or_else(|| {
                    "Training slot no longer owns this attempt; staged output kept".into()
                }));
        }
        if let Some(error) = &outcome.setup_error {
            return Err(error.clone());
        }
        match &outcome.exit {
            Ok(status) if status.success() => {}
            Ok(status) => {
                return Err(if outcome.stderr.trim().is_empty() {
                    format!("Training process exited abnormally ({status})")
                } else {
                    outcome.stderr.trim().into()
                })
            }
            Err(error) => return Err(format!("Failed to wait for process: {error}")),
        }
        if let Some(error) = &outcome.report.error {
            return Err(error.clone());
        }
        if !outcome.report.matches_expected(&attempt.out_dir()) {
            return Err("Training did not report done for the expected staging output".into());
        }
        adapter_staging::transition(attempt, AttemptState::ExitedOk).map_err(|e| e.to_string())?;
        adapter_staging::verify_out(attempt).map_err(|e| e.to_string())?;
        adapter_staging::promote(attempt, &admission).map_err(|e| e.to_string())
    })();
    match result {
        Ok(promoted) => {
            if let Some(warning) = promoted.warning {
                eprintln!("[mlx] {warning}");
            }
            if let Some(training) = training {
                training.adapter_path = Some(promoted.final_path.to_string_lossy().into_owned());
                training.error = None;
                training.status = "done".into();
            }
        }
        Err(error) => {
            if let Some(training) = training {
                training.adapter_path = None;
                training.status = if killed { "killed" } else { "error" }.into();
                training.error = (!killed).then_some(error.clone());
            }
            let terminal = if killed {
                AttemptState::Killed
            } else {
                AttemptState::Failed
            };
            if matches!(
                attempt.record().state,
                AttemptState::Created
                    | AttemptState::Running
                    | AttemptState::ExitedOk
                    | AttemptState::Verified
                    | AttemptState::VerifiedUnpromoted
            ) {
                adapter_staging::transition(attempt, terminal)
                    .map_err(|e| format!("{error}; failed to persist outcome: {e}"))?;
            }
            // Publication failure is failed too; all staging bytes stay protected.
        }
    }
    Ok(reconciliation)
}

#[cfg(test)]
mod tests;
