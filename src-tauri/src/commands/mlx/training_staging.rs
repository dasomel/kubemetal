//! D45 training completion boundary. Filesystem work runs on spawn_blocking;
//! the child report is evidence, never authority to publish a checkpoint.
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

use super::{MlxState, TrainingStatus};
use crate::services::adapter_staging::{self, Attempt, AttemptState, AttemptSummary};
use crate::services::mlx_lifecycle::{self, MlflowRunReconciliation};

#[derive(Default)]
pub(super) struct CompletionReport {
    path: Option<String>,
    error: Option<String>,
    terminal: bool,
}

/// Same bound as the stderr fallback (`collect_stderr`), cut on a char boundary: the
/// wrapper's message is untrusted and reaches the UI.
const MAX_ERROR_BYTES: usize = 4000;

fn cap_error(message: &str) -> String {
    let mut end = message.len().min(MAX_ERROR_BYTES);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message[..end].into()
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
                let message = event
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Training wrapper reported an error");
                self.error = Some(cap_error(message));
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
    finalize_with(state, attempt, stopped, outcome, &|| {})
}

fn finalize_with(
    state: &MlxState,
    attempt: &mut Attempt,
    stopped: &AtomicBool,
    outcome: TrainingExit,
    before_hashing: &dyn Fn(),
) -> Result<Option<MlflowRunReconciliation>, String> {
    // D45 lock order everywhere: adapter_admission, then training, then training_stop;
    // never admission while holding training. Admission alone serializes promotion against
    // deletion, so it is held across hash/verify/promote. The training slot is read by
    // async commands (status, kill, guardrails), so it is NOT held while hashing: it is
    // taken once to snapshot ownership and once to publish the outcome.
    let admission = state.adapter_admission.lock().map_err(|e| e.to_string())?;
    let (reconciliation, killed, precheck) = {
        let mut slot = state.training.lock().map_err(|e| e.to_string())?;
        let training = owned_slot(state, &mut slot, stopped, outcome.pid)?;
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
        let killed = stopped.load(Ordering::SeqCst)
            || training.as_ref().is_some_and(|t| t.status == "killed");
        let precheck = if killed {
            Err("Training was stopped; staged output kept".to_string())
        } else if training
            .as_ref()
            .is_none_or(|t| !mlx_lifecycle::is_non_terminal_training_status(&t.status))
        {
            Err(training
                .as_ref()
                .and_then(|t| t.error.clone())
                .unwrap_or_else(|| {
                    "Training slot no longer owns this attempt; staged output kept".into()
                }))
        } else {
            Ok(())
        };
        (reconciliation, killed, precheck)
    };
    let result = precheck.and_then(|()| {
        if let Some(error) = &outcome.setup_error {
            return Err(error.clone());
        }
        match &outcome.exit {
            Ok(status) if status.success() => {}
            // The wrapper's own error event is stdout-only; stderr is the generic fallback.
            Ok(status) => {
                return Err(if let Some(error) = &outcome.report.error {
                    error.clone()
                } else if outcome.stderr.trim().is_empty() {
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
        before_hashing();
        adapter_staging::transition(attempt, AttemptState::ExitedOk).map_err(|e| e.to_string())?;
        adapter_staging::verify_out(attempt).map_err(|e| e.to_string())?;
        adapter_staging::promote(attempt, &admission).map_err(|e| e.to_string())
    });
    // D22 invariant: the published slot must match attempt.json and the filesystem.
    // `killed` is decided once, before hashing, from whether the stop reached the child
    // before it ended (a child that exited 0 with `done` cannot be killed afterwards).
    // A stop landing during hashing is a no-op on a dead process: it must neither
    // un-promote a committed adapter nor hide a real failure behind "killed".
    let mut published = None;
    {
        let mut slot = state.training.lock().map_err(|e| e.to_string())?;
        let training = owned_slot(state, &mut slot, stopped, outcome.pid)?;
        if let Some(training) = training {
            // "killed" here is only the optimistic mark of a late stop (see above).
            let live = mlx_lifecycle::is_non_terminal_training_status(&training.status)
                || training.status == "killed";
            match &result {
                Ok(promoted) if live => {
                    training.adapter_path =
                        Some(promoted.final_path.to_string_lossy().into_owned());
                    training.error = None;
                    training.status = "done".into();
                    published = Some(());
                }
                Ok(_) => {}
                Err(error) if live || killed => {
                    training.adapter_path = None;
                    training.status = if killed { "killed" } else { "error" }.into();
                    training.error = (!killed).then_some(error.clone());
                }
                Err(_) => {}
            }
        }
    }
    match result {
        Ok(promoted) => {
            if let Some(warning) = promoted.warning {
                eprintln!("[mlx] {warning}");
            }
            if published.is_none() {
                eprintln!("[mlx] Output was promoted but the training slot no longer owns this attempt; slot left unchanged");
            }
        }
        Err(error) => {
            // S1 allows only Running -> Killed; every other state (Created, ExitedOk,
            // Verified, VerifiedUnpromoted) records its failure as Failed.
            let terminal = if killed && attempt.record().state == AttemptState::Running {
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

/// The slot, only if it still belongs to this exact attempt. The per-attempt stop object
/// binds ownership: a reused PID alone must not let an older reader rewrite a newer run.
/// This grants no signal authority.
fn owned_slot<'a>(
    state: &MlxState,
    slot: &'a mut Option<TrainingStatus>,
    stopped: &AtomicBool,
    pid: u32,
) -> Result<Option<&'a mut TrainingStatus>, String> {
    let same_attempt = state
        .training_stop
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        .is_some_and(|(p, stop)| *p == pid && std::ptr::eq(stopped, stop.as_ref()));
    Ok(slot.as_mut().filter(|t| t.pid == pid && same_attempt))
}

/// D46: read-only inventory of adapter staging attempts. Never repairs, resumes or deletes;
/// `Unknown` rows are reported as-is. Same root as `run_mlx_finetune` (`~/.kubemetal`).
#[tauri::command]
pub async fn list_adapter_staging() -> Result<Vec<AttemptSummary>, String> {
    let root = super::home_dir()?.join(".kubemetal");
    tokio::task::spawn_blocking(move || adapter_staging::list_attempts(&root))
        .await
        .map_err(|e| format!("Adapter staging listing task failed: {e}"))
}

#[cfg(test)]
mod tests;
