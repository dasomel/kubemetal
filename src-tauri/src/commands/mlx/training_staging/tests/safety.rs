use super::*;

#[test]
fn forged_done_path_never_publishes_or_promotes() {
    let home = TempHome::new();
    let (state, mut attempt) = ready(&home);
    let foreign = home.0.join("foreign");
    fs::create_dir(&foreign).unwrap();
    fs::write(foreign.join("sentinel"), b"untouched").unwrap();
    let mut outcome = done(&attempt, 0);
    outcome.report = CompletionReport::default();
    let event = serde_json::json!({"type":"done", "adapter_path":foreign});
    apply_event(state.training.lock().unwrap().as_mut().unwrap(), &event);
    assert_eq!(
        state.training.lock().unwrap().as_ref().unwrap().status,
        "running"
    );
    assert!(state
        .training
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .adapter_path
        .is_none());
    outcome.report.observe(&event, &attempt.out_dir());
    // A later valid report cannot erase the forged report.
    outcome.report.observe(
        &serde_json::json!({"type":"done", "adapter_path":attempt.out_dir()}),
        &attempt.out_dir(),
    );
    finalize(&state, &mut attempt, &active_stop(&state), outcome).unwrap();
    assert_failed(&state, &attempt, AttemptState::Failed);
    assert_eq!(fs::read(foreign.join("sentinel")).unwrap(), b"untouched");
    assert_eq!(fs::read_dir(foreign).unwrap().count(), 1);
}

#[test]
fn zero_exit_without_done_is_error() {
    let home = TempHome::new();
    let (state, mut attempt) = ready(&home);
    let mut outcome = done(&attempt, 0);
    outcome.report = CompletionReport::default();
    finalize(&state, &mut attempt, &active_stop(&state), outcome).unwrap();
    assert_failed(&state, &attempt, AttemptState::Failed);
}

#[test]
fn manifest_write_failure_is_error_never_done() {
    let home = TempHome::new();
    let (state, mut attempt) = ready(&home);
    // A readable, unwritable manifest passes scan_out but makes fs::write fail.
    let manifest = attempt.out_dir().join("manifest.json");
    fs::write(&manifest, b"unwritable").unwrap();
    fs::set_permissions(&manifest, fs::Permissions::from_mode(0o400)).unwrap();
    let outcome = done(&attempt, 0);
    finalize(&state, &mut attempt, &active_stop(&state), outcome).unwrap();
    assert_failed(&state, &attempt, AttemptState::Failed);
    assert!(state
        .training
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .error
        .as_ref()
        .unwrap()
        .contains("Failed to write artifact manifest"));
    fs::set_permissions(&manifest, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn failed_exit_cannot_be_overridden_by_done() {
    let home = TempHome::new();
    let (state, mut attempt) = ready(&home);
    let outcome = done(&attempt, 1);
    finalize(&state, &mut attempt, &active_stop(&state), outcome).unwrap();
    assert_failed(&state, &attempt, AttemptState::Failed);
}

#[test]
fn killed_zero_exit_with_done_stays_killed() {
    let home = TempHome::new();
    let (state, mut attempt) = ready(&home);
    let outcome = done(&attempt, 0);
    let stopped = state
        .training_stop
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .1
        .clone();
    request_stop(&state, 42).unwrap();
    finalize(&state, &mut attempt, &stopped, outcome).unwrap();
    assert_failed(&state, &attempt, AttemptState::Killed);
}

#[test]
fn killed_attempt_survives_slot_replacement() {
    let home = TempHome::new();
    let (state, mut attempt) = ready(&home);
    let outcome = done(&attempt, 0);
    let stopped = state
        .training_stop
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .1
        .clone();
    request_stop(&state, 42).unwrap();
    *state.training.lock().unwrap() = Some(TrainingStatus {
        pid: 99,
        ..training()
    });
    *state.training_stop.lock().unwrap() = Some((99, Arc::new(AtomicBool::new(false))));
    finalize(&state, &mut attempt, &stopped, outcome).unwrap();
    assert_eq!(persisted(&attempt), AttemptState::Killed);
    assert!(attempt.out_dir().exists());
    assert!(!attempt.final_path().exists());
    assert_eq!(
        state.training.lock().unwrap().as_ref().unwrap().status,
        "running"
    );
}

#[test]
fn wrapper_error_and_late_done_cannot_erase_stop() {
    let mut training = training();
    training.status = "killed".into();
    for event in [
        serde_json::json!({"type":"error","message":"child error"}),
        serde_json::json!({"type":"done","adapter_path":"/forged"}),
    ] {
        apply_event(&mut training, &event);
    }
    assert_eq!(training.status, "killed");
    assert!(training.adapter_path.is_none());
}

#[test]
fn failed_stop_does_not_claim_killed_or_done() {
    let home = TempHome::new();
    let (state, mut attempt) = ready(&home);
    let outcome = done(&attempt, 0);
    let stopped = state
        .training_stop
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .1
        .clone();
    request_stop(&state, 42).unwrap();
    {
        let mut slot = state.training.lock().unwrap();
        clear_stop_intent(&state, 42).unwrap();
        slot.as_mut().unwrap().status = "error".into();
    }
    finalize(&state, &mut attempt, &stopped, outcome).unwrap();
    assert_failed(&state, &attempt, AttemptState::Failed);
}

#[test]
fn stopped_child_late_mlflow_run_id_is_retained() {
    let mut training = training();
    training.status = "killed".into();
    apply_event(
        &mut training,
        &serde_json::json!({"type":"mlflow_run_started", "run_id":"actual-child-run"}),
    );
    assert_eq!(training.status, "killed");
    assert_eq!(training.mlflow_run_id.as_deref(), Some("actual-child-run"));
    assert!(training.adapter_path.is_none());
}

#[test]
fn missing_done_cannot_match_non_utf8_expected_path() {
    use std::os::unix::ffi::OsStringExt;
    let home = TempHome::new();
    let expected = home
        .root()
        .join(std::ffi::OsString::from_vec(b"out-\xff".to_vec()));
    // Predicate only: macOS/sandbox refused creating a non-UTF-8 directory.
    assert!(!CompletionReport::default().matches_expected(&expected));
}

#[test]
fn reused_pid_does_not_overwrite_next_attempt() {
    let home = TempHome::new();
    let (state, mut attempt) = ready(&home);
    let outcome = done(&attempt, 0);
    let stopped = active_stop(&state);
    request_stop(&state, 42).unwrap();
    *state.training.lock().unwrap() = Some(TrainingStatus {
        total_iters: 777,
        ..training()
    });
    *state.training_stop.lock().unwrap() = Some((42, Arc::new(AtomicBool::new(false))));
    finalize(&state, &mut attempt, &stopped, outcome).unwrap();
    assert_eq!(persisted(&attempt), AttemptState::Killed);
    let slot = state.training.lock().unwrap();
    assert_eq!(slot.as_ref().unwrap().status, "running");
    assert_eq!(slot.as_ref().unwrap().total_iters, 777);
    assert!(slot.as_ref().unwrap().adapter_path.is_none());
}

#[test]
fn failed_exit_surfaces_wrapper_error_over_generic_stderr() {
    let home = TempHome::new();
    let (state, mut attempt) = ready(&home);
    let mut outcome = done(&attempt, 1);
    outcome.report = CompletionReport::default();
    outcome.report.observe(
        &serde_json::json!({"type":"error","message":"wrapper: out of memory at iter 7"}),
        &attempt.out_dir(),
    );
    outcome.stderr = "Training process exited abnormally (exit status: 1)".into();
    finalize(&state, &mut attempt, &active_stop(&state), outcome).unwrap();
    assert_failed(&state, &attempt, AttemptState::Failed);
    assert_eq!(
        state
            .training
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .error
            .as_deref(),
        Some("wrapper: out of memory at iter 7")
    );
}

#[test]
fn failed_exit_without_wrapper_error_keeps_stderr_text() {
    let home = TempHome::new();
    let (state, mut attempt) = ready(&home);
    let mut outcome = done(&attempt, 1);
    outcome.report = CompletionReport::default();
    outcome.stderr = "Traceback: boom".into();
    finalize(&state, &mut attempt, &active_stop(&state), outcome).unwrap();
    assert_eq!(
        state
            .training
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .error
            .as_deref(),
        Some("Traceback: boom")
    );
}

#[test]
fn killed_attempt_that_never_started_ends_failed_not_created() {
    let home = TempHome::new();
    let mut attempt = adapter_staging::create_attempt(&home.root(), &spec()).unwrap();
    let state = MlxState::default();
    *state.training.lock().unwrap() = Some(training());
    *state.training_stop.lock().unwrap() = Some((42, Arc::new(AtomicBool::new(false))));
    let mut outcome = done(&attempt, 0);
    outcome.setup_error = Some("mark_running failed".into());
    let stopped = active_stop(&state);
    request_stop(&state, 42).unwrap();
    finalize(&state, &mut attempt, &stopped, outcome).unwrap();
    assert_eq!(persisted(&attempt), AttemptState::Failed);
    assert_eq!(
        state.training.lock().unwrap().as_ref().unwrap().status,
        "killed"
    );
}
