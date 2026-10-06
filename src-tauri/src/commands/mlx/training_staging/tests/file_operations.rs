use super::*;

#[test]
fn legacy_name_collision_is_refused_byte_for_byte() {
    let home = TempHome::new();
    let dir = home.root().join("adapters/s2");
    fs::create_dir_all(&dir).unwrap();
    let bytes = b"legacy weights\x00\xff";
    fs::write(dir.join("adapters.safetensors"), bytes).unwrap();
    let before = fs::metadata(&dir).unwrap().modified().unwrap();
    assert!(matches!(
        adapter_staging::create_attempt(&home.root(), &spec()),
        Err(StagingError::NameTaken(_))
    ));
    assert_eq!(fs::read(dir.join("adapters.safetensors")).unwrap(), bytes);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    assert_eq!(fs::metadata(dir).unwrap().modified().unwrap(), before);
    assert!(!home.root().join("adapter-staging").exists());
}

#[test]
fn promotion_waits_for_delete_admission_mutex() {
    let home = TempHome::new();
    let (state, mut attempt) = ready(&home);
    let state = Arc::new(state);
    let final_path = attempt.final_path();
    let outcome = done(&attempt, 0);
    let held = state.adapter_admission.lock().unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let worker_state = state.clone();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        finalize(
            &worker_state,
            &mut attempt,
            &active_stop(&worker_state),
            outcome,
        )
        .unwrap();
        done_tx.send(()).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let blocked = done_rx.recv_timeout(Duration::from_millis(150)).is_err();
    // Lock order: admission first. The waiter must not already hold the training
    // slot (async commands lock it), so M6-style inversion fails here.
    let slot_free = state.training.try_lock().is_ok();
    let absent = !final_path.exists();
    drop(held);
    worker.join().unwrap();
    assert!(
        blocked && absent && slot_free,
        "promotion must wait for the delete IPC's admission lock without holding the training slot"
    );
    assert_eq!(
        state.training.lock().unwrap().as_ref().unwrap().status,
        "done"
    );
    assert_eq!(
        state
            .training
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .adapter_path
            .as_deref(),
        final_path.to_str()
    );
    assert!(
        crate::services::artifact_manifest::verify_manifest(&final_path)
            .unwrap()
            .is_valid()
    );
    crate::commands::mlx::remove_adapter_checkpoint(
        final_path.to_str().unwrap(),
        Ok(home.0.clone()),
        &state,
    )
    .unwrap();
    assert!(!final_path.exists());
}

#[test]
fn staging_is_protected_and_outside_delete_ipc_root() {
    let home = TempHome::new();
    let (state, attempt) = ready(&home);
    assert!(adapter_staging::staging_path_is_protected(
        &home.root(),
        &attempt.out_dir()
    ));
    assert!(crate::commands::mlx::remove_adapter_checkpoint(
        attempt.out_dir().to_str().unwrap(),
        Ok(home.0.clone()),
        &state
    )
    .is_err());
    assert!(attempt.out_dir().join("adapters.safetensors").is_file());
}

#[tokio::test]
async fn spawn_failure_removes_only_new_empty_attempt() {
    let home = TempHome::new();
    let older = adapter_staging::create_attempt(
        &home.root(),
        &AttemptSpec {
            adapter_name: "older".into(),
            ..spec()
        },
    )
    .unwrap();
    fs::write(older.out_dir().join("keep"), b"old").unwrap();
    let attempt = adapter_staging::create_attempt(&home.root(), &spec()).unwrap();
    let missing_binary = home.0.join("missing-python");
    assert!(spawn(&mut tokio::process::Command::new(&missing_binary), &attempt).is_err());
    assert!(!attempt.dir().exists());
    assert_eq!(fs::read(older.out_dir().join("keep")).unwrap(), b"old");
    assert!(older.dir().join("attempt.json").is_file());
    assert!(home.root().join("adapter-staging").is_dir());
    // rmdir, rather than recursive deletion, must preserve unexpected content.
    let attempt = adapter_staging::create_attempt(&home.root(), &spec()).unwrap();
    fs::write(attempt.out_dir().join("unexpected"), b"keep").unwrap();
    assert!(spawn(&mut tokio::process::Command::new(&missing_binary), &attempt).is_err());
    assert_eq!(
        fs::read(attempt.out_dir().join("unexpected")).unwrap(),
        b"keep"
    );
    assert!(attempt.dir().join("attempt.json").is_file());
}

#[test]
fn promotion_collision_records_failed_and_preserves_both_outputs() {
    let home = TempHome::new();
    let (state, mut attempt) = ready(&home);
    let outcome = done(&attempt, 0);
    fs::create_dir(attempt.final_path()).unwrap();
    fs::write(attempt.final_path().join("sentinel"), b"legacy").unwrap();
    finalize(&state, &mut attempt, &active_stop(&state), outcome).unwrap();
    assert_eq!(
        state.training.lock().unwrap().as_ref().unwrap().status,
        "error"
    );
    assert!(state
        .training
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .adapter_path
        .is_none());
    assert_eq!(persisted(&attempt), AttemptState::Failed);
    assert_eq!(
        fs::read(attempt.final_path().join("sentinel")).unwrap(),
        b"legacy"
    );
    assert_eq!(fs::read_dir(attempt.final_path()).unwrap().count(), 1);
    assert!(
        crate::services::artifact_manifest::verify_manifest(&attempt.out_dir())
            .unwrap()
            .is_valid()
    );
}

#[test]
fn promotion_permission_failure_records_failed() {
    let home = TempHome::new();
    let (state, mut attempt) = ready(&home);
    let outcome = done(&attempt, 0);
    let adapters = home.root().join("adapters");
    fs::set_permissions(&adapters, fs::Permissions::from_mode(0o500)).unwrap();
    finalize(&state, &mut attempt, &active_stop(&state), outcome).unwrap();
    fs::set_permissions(&adapters, fs::Permissions::from_mode(0o700)).unwrap();
    assert_failed(&state, &attempt, AttemptState::Failed);
    assert!(
        crate::services::artifact_manifest::verify_manifest(&attempt.out_dir())
            .unwrap()
            .is_valid()
    );
}

#[test]
fn hashing_runs_with_admission_held_and_training_slot_free() {
    let home = TempHome::new();
    let (state, mut attempt) = ready(&home);
    let outcome = done(&attempt, 0);
    let stopped = active_stop(&state);
    let observed = std::cell::Cell::new(None);
    finalize_with(&state, &mut attempt, &stopped, outcome, &|| {
        // D45 lock order: admission is held across verify+promote, the training
        // slot (read by async commands) is not.
        observed.set(Some((
            state.adapter_admission.try_lock().is_err(),
            state.training.try_lock().is_ok(),
        )));
    })
    .unwrap();
    assert_eq!(observed.get(), Some((true, true)));
    assert_eq!(
        state.training.lock().unwrap().as_ref().unwrap().status,
        "done"
    );
}

#[test]
fn stop_during_hashing_is_not_overwritten_by_done() {
    let home = TempHome::new();
    let (state, mut attempt) = ready(&home);
    let outcome = done(&attempt, 0);
    let stopped = active_stop(&state);
    finalize_with(&state, &mut attempt, &stopped, outcome, &|| {
        request_stop(&state, 42).unwrap();
    })
    .unwrap();
    let slot = state.training.lock().unwrap();
    let status = slot.as_ref().unwrap();
    assert_eq!(status.status, "killed");
    assert!(status.adapter_path.is_none());
}
