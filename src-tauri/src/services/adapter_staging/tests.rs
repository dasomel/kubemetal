use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};

use super::*;

static NONCE: AtomicU64 = AtomicU64::new(0);

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "kubemetal-staging-{}-{}",
            std::process::id(),
            NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn spec(name: &str) -> AttemptSpec {
    AttemptSpec {
        adapter_name: name.into(),
        runtime: "mlx-lm".into(),
        base_model: "base/model".into(),
        iters: 10,
        mlflow_run_id: None,
    }
}

fn same_dev(_: &Path) -> io::Result<u64> {
    Ok(1)
}

/// Attempt that has finished training successfully with valid output files.
fn exited_ok(root: &Path, name: &str) -> Attempt {
    let mut a = create_attempt(root, &spec(name)).unwrap();
    mark_running(&mut a, 1, Some(1)).unwrap();
    fs::write(a.out_dir().join("adapters.safetensors"), b"weights").unwrap();
    fs::write(a.out_dir().join("adapter_config.json"), b"{}").unwrap();
    transition(&mut a, AttemptState::ExitedOk).unwrap();
    a
}

fn verified(root: &Path, name: &str) -> Attempt {
    let mut a = exited_ok(root, name);
    verify_out(&mut a).unwrap();
    a
}

fn raw_record(dir: &Path, state: &str, extra: &str) {
    let id = dir.file_name().unwrap().to_str().unwrap();
    fs::write(
        dir.join(RECORD_FILE),
        format!(
            r#"{{"attempt_id":"{id}","adapter_name":"n","runtime":"r","base_model":"b","iters":1,
            "mlflow_run_id":null,"pid":null,"start_time":null,"manifest_sha256":null,
            "state":"{state}"{extra}}}"#
        ),
    )
    .unwrap();
}

// --- record parsing fails closed -------------------------------------------------------

#[test]
fn record_corrupt_unknown_field_or_unknown_state_is_err() {
    let t = TempRoot::new();
    let a = create_attempt(&t.0, &spec("a")).unwrap();
    let dir = a.dir();
    assert!(read_record(&dir).is_ok());
    fs::write(dir.join(RECORD_FILE), b"{ not json").unwrap();
    assert!(read_record(&dir).is_err());
    raw_record(&dir, "created", r#","surprise":1"#);
    assert!(read_record(&dir).is_err());
    raw_record(&dir, "interrupted", "");
    assert!(read_record(&dir).is_err(), "interrupted is never persisted");
    raw_record(&dir, "created", "");
    assert!(read_record(&dir).is_ok());
}

#[test]
fn record_with_foreign_attempt_id_is_err() {
    let t = TempRoot::new();
    let a = create_attempt(&t.0, &spec("a")).unwrap();
    let mut rec = a.record.clone();
    rec.attempt_id = "someone-else".into();
    fs::write(a.dir().join(RECORD_FILE), serde_json::to_vec(&rec).unwrap()).unwrap();
    assert!(read_record(&a.dir()).is_err());
}

#[test]
fn attempt_json_is_never_observable_half_written() {
    let t = TempRoot::new();
    let a = create_attempt(&t.0, &spec("a")).unwrap();
    let dir = a.dir();
    // A stale temp from a crashed writer must not influence readers.
    fs::write(dir.join(".attempt.json.tmp-0-0"), b"{ half").unwrap();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reader = {
        let (dir, stop) = (dir.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut reads = 0_u32;
            while !stop.load(Ordering::Relaxed) {
                read_record(&dir).expect("record must always parse");
                reads += 1;
                // Yield so the poller does not starve unrelated timing-sensitive tests.
                std::thread::sleep(std::time::Duration::from_micros(200));
            }
            reads
        })
    };
    let mut rec = a.record.clone();
    for i in 0..40 {
        rec.iters = i;
        write_record(&dir, &rec).unwrap();
    }
    stop.store(true, Ordering::Relaxed);
    assert!(reader.join().unwrap() > 0);
    assert_eq!(read_record(&dir).unwrap().iters, 39);
}

// --- name collision ---------------------------------------------------------------------

#[test]
fn create_refuses_any_existing_form_at_final_path_and_leaves_it_untouched() {
    for form in ["file", "dir", "empty_dir", "symlink", "dangling_symlink"] {
        let t = TempRoot::new();
        let adapters = t.0.join(ADAPTERS_DIR);
        fs::create_dir_all(&adapters).unwrap();
        let target = adapters.join("taken");
        match form {
            "file" => fs::write(&target, b"bytes").unwrap(),
            "dir" => {
                fs::create_dir(&target).unwrap();
                fs::write(target.join("f"), b"bytes").unwrap();
            }
            "empty_dir" => fs::create_dir(&target).unwrap(),
            "symlink" => symlink(&adapters, &target).unwrap(),
            _ => symlink(t.0.join("nowhere"), &target).unwrap(),
        }
        let before = fs::symlink_metadata(&target).unwrap();
        let err = create_attempt(&t.0, &spec("taken")).unwrap_err();
        assert!(matches!(err, StagingError::NameTaken(_)), "{form}: {err}");
        let after = fs::symlink_metadata(&target).unwrap();
        assert_eq!(before.file_type(), after.file_type(), "{form}");
        assert_eq!(before.len(), after.len(), "{form}");
        assert!(!t.0.join(STAGING_DIR).exists(), "{form}: no staging debris");
        if form == "dir" {
            assert_eq!(fs::read(target.join("f")).unwrap(), b"bytes");
        }
    }
}

#[test]
fn rename_excl_refuses_empty_and_non_empty_directories() {
    let t = TempRoot::new();
    for populated in [false, true] {
        let src = t.0.join(format!("src{populated}"));
        fs::create_dir(&src).unwrap();
        fs::write(src.join("new"), b"new").unwrap();
        let dst = t.0.join(format!("dst{populated}"));
        fs::create_dir(&dst).unwrap();
        if populated {
            fs::write(dst.join("old"), b"old").unwrap();
        }
        let e = rename_excl(&src, &dst).unwrap_err();
        assert!(matches!(
            e.raw_os_error(),
            Some(libc::EEXIST | libc::ENOTEMPTY)
        ));
        assert!(src.join("new").is_file(), "source untouched");
        assert_eq!(fs::read_dir(&dst).unwrap().count(), usize::from(populated));
    }
}

#[test]
fn promote_refuses_name_taken_before_and_during_the_rename_window() {
    for racing in [false, true] {
        let t = TempRoot::new();
        let mut a = verified(&t.0, "n");
        let final_path = a.final_path();
        let make = || {
            fs::create_dir(&final_path).unwrap();
            fs::write(final_path.join("keep"), b"legacy").unwrap();
        };
        if racing {
            // Appears after the lstat pre-check: only RENAME_EXCL can catch this.
            let e = promote_with(&mut a, &same_dev, &make, &|| {}).unwrap_err();
            assert!(matches!(e, StagingError::NameTaken(_)));
        } else {
            make();
            let e = promote_with(&mut a, &same_dev, &|| {}, &|| {}).unwrap_err();
            assert!(matches!(e, StagingError::NameTaken(_)));
        }
        assert_eq!(fs::read(final_path.join("keep")).unwrap(), b"legacy");
        assert_eq!(fs::read_dir(&final_path).unwrap().count(), 1);
        assert!(a.out_dir().join("adapters.safetensors").is_file());
        assert_eq!(a.record.state, AttemptState::VerifiedUnpromoted);
        assert_eq!(
            read_record(&a.dir()).unwrap().state,
            AttemptState::VerifiedUnpromoted
        );
    }
}

#[test]
fn promote_moves_out_to_final_and_records_promoted() {
    let t = TempRoot::new();
    let mut a = verified(&t.0, "n");
    let p = promote(&mut a, &()).unwrap();
    assert_eq!(p.final_path, t.0.join("adapters/n"));
    assert!(p.final_path.join("adapters.safetensors").is_file());
    assert!(p.final_path.join("manifest.json").is_file());
    assert!(!a.out_dir().exists());
    assert_eq!(read_record(&a.dir()).unwrap().state, AttemptState::Promoted);
}

#[test]
fn promote_from_non_verified_state_is_err() {
    let t = TempRoot::new();
    let mut created = create_attempt(&t.0, &spec("c")).unwrap();
    assert!(promote(&mut created, &()).is_err());
    let mut exited = exited_ok(&t.0, "e");
    assert!(promote(&mut exited, &()).is_err());
    assert!(exited.out_dir().join("adapters.safetensors").is_file());
    let mut done = verified(&t.0, "d");
    promote(&mut done, &()).unwrap();
    assert!(promote(&mut done, &()).is_err(), "promoted is terminal");
    assert!(verify_out(&mut created).is_err());
}

#[test]
fn st_dev_mismatch_is_err_and_never_copies() {
    let t = TempRoot::new();
    let split = |p: &Path| {
        Ok(if p.ends_with(STAGING_DIR) || p.ends_with(OUT_DIR) {
            1
        } else {
            2
        })
    };
    let e = create_attempt_with(&t.0, &spec("n"), "id1", &split).unwrap_err();
    assert!(e.to_string().contains("different filesystems"));
    assert!(!t.0.join(STAGING_DIR).join("id1").exists());

    let mut a = verified(&t.0, "m");
    let e = promote_with(&mut a, &split, &|| {}, &|| {}).unwrap_err();
    assert!(e.to_string().contains("different filesystems"));
    assert!(a.out_dir().join("adapters.safetensors").is_file());
    assert!(!a.final_path().exists());
    assert_eq!(a.record.state, AttemptState::Verified);
}

// --- symlink / structure refusal ----------------------------------------------------------

#[test]
fn symlinked_staging_root_is_refused() {
    let t = TempRoot::new();
    let elsewhere = t.0.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, t.0.join(STAGING_DIR)).unwrap();
    assert!(create_attempt(&t.0, &spec("n")).is_err());
    assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
}

#[test]
fn preexisting_symlink_or_dir_at_attempt_path_is_refused() {
    let t = TempRoot::new();
    let staging = t.0.join(STAGING_DIR);
    fs::create_dir_all(&staging).unwrap();
    let elsewhere = t.0.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, staging.join("linked")).unwrap();
    fs::create_dir(staging.join("plain")).unwrap();
    for id in ["linked", "plain"] {
        assert!(create_attempt_with(&t.0, &spec("n"), id, &same_dev).is_err());
    }
    assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
}

#[test]
fn verify_refuses_symlinked_attempt_dir_out_dir_and_file() {
    // attempt dir replaced by a symlink to the real one
    let t = TempRoot::new();
    let mut a = exited_ok(&t.0, "a");
    let moved = t.0.join("moved");
    fs::rename(a.dir(), &moved).unwrap();
    symlink(&moved, a.dir()).unwrap();
    assert!(verify_out(&mut a).is_err());

    // out/ replaced by a symlink
    let mut b = exited_ok(&t.0, "b");
    let real_out = t.0.join("real_out");
    fs::rename(b.out_dir(), &real_out).unwrap();
    symlink(&real_out, b.out_dir()).unwrap();
    assert!(verify_out(&mut b).is_err());

    // a symlink among the files (manifest collector would skip it silently)
    let mut c = exited_ok(&t.0, "c");
    symlink("/etc/hosts", c.out_dir().join("extra.txt")).unwrap();
    assert!(verify_out(&mut c).is_err());
    assert_eq!(c.record.state, AttemptState::ExitedOk);
}

#[test]
fn verify_refuses_subdirectory_and_missing_or_empty_required_files() {
    let t = TempRoot::new();
    let mut a = exited_ok(&t.0, "a");
    fs::create_dir(a.out_dir().join("sub")).unwrap();
    assert!(verify_out(&mut a).is_err());

    let mut b = exited_ok(&t.0, "b");
    fs::remove_file(b.out_dir().join("adapter_config.json")).unwrap();
    assert!(verify_out(&mut b).is_err());

    let mut c = exited_ok(&t.0, "c");
    fs::write(c.out_dir().join("adapters.safetensors"), b"").unwrap();
    assert!(verify_out(&mut c).is_err());
    assert!(
        !c.out_dir().join(MANIFEST_FILE).exists(),
        "no manifest on refusal"
    );
}

#[test]
fn verify_records_manifest_hash_and_promote_refuses_symlink_planted_after_verify() {
    let t = TempRoot::new();
    let mut a = exited_ok(&t.0, "a");
    let v = verify_out(&mut a).unwrap();
    assert_eq!(v.file_count, 3);
    assert_eq!(a.record.state, AttemptState::Verified);
    assert_eq!(
        read_record(&a.dir()).unwrap().manifest_sha256.as_deref(),
        Some(v.manifest_sha256.as_str())
    );
    symlink("/etc/hosts", a.out_dir().join("late")).unwrap();
    assert!(promote(&mut a, &()).is_err());
    assert!(!a.final_path().exists());
}

// --- concurrency --------------------------------------------------------------------------

#[test]
fn concurrent_create_of_same_attempt_id_has_exactly_one_winner() {
    let t = TempRoot::new();
    let root = Arc::new(t.0.clone());
    let barrier = Arc::new(Barrier::new(4));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let (root, barrier) = (root.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                create_attempt_with(&root, &spec("n"), "same-id", &same_dev).is_ok()
            })
        })
        .collect();
    let wins = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .filter(|ok| *ok)
        .count();
    assert_eq!(wins, 1);
    assert_eq!(fs::read_dir(t.0.join(STAGING_DIR)).unwrap().count(), 1);
}

// --- reconcile ----------------------------------------------------------------------------

fn outcome(root: &Path, alive: bool) -> Vec<ReconcileOutcome> {
    reconcile_with(root, &|_, _| alive)
}

#[test]
fn reconcile_unreadable_or_foreign_entries_are_unknown_and_untouched() {
    let t = TempRoot::new();
    let staging = t.0.join(STAGING_DIR);
    fs::create_dir_all(staging.join("corrupt/out")).unwrap();
    fs::write(staging.join("corrupt/attempt.json"), b"garbage").unwrap();
    fs::write(staging.join("corrupt/out/w"), b"w").unwrap();
    fs::create_dir(staging.join("norecord")).unwrap();
    fs::write(staging.join("stray-file"), b"x").unwrap();
    symlink(&t.0, staging.join("linked")).unwrap();
    let out = outcome(&t.0, true);
    assert_eq!(out.len(), 4);
    assert!(out
        .iter()
        .all(|o| matches!(o, ReconcileOutcome::Unknown { .. })));
    assert_eq!(
        fs::read(staging.join("corrupt/attempt.json")).unwrap(),
        b"garbage"
    );
    assert!(staging.join("corrupt/out/w").is_file());
    assert!(staging.join("stray-file").is_file());
}

#[test]
fn reconcile_running_alive_vs_dead_identity() {
    let t = TempRoot::new();
    let a = exited_ok(&t.0, "a"); // exited_ok helper passes through running first
    let mut r = create_attempt(&t.0, &spec("r")).unwrap();
    mark_running(&mut r, 4242, Some(99)).unwrap();
    let seen = std::cell::Cell::new(None);
    let out = reconcile_with(&t.0, &|pid, start| {
        seen.set(Some((pid, start)));
        true
    });
    assert_eq!(seen.get(), Some((4242, Some(99))));
    assert!(out.contains(&ReconcileOutcome::StillRunning {
        attempt_id: r.record.attempt_id.clone()
    }));
    let out = outcome(&t.0, false);
    assert!(out.contains(&ReconcileOutcome::Interrupted {
        attempt_id: r.record.attempt_id.clone()
    }));
    // derived only: nothing persisted, staging kept
    assert_eq!(read_record(&r.dir()).unwrap().state, AttemptState::Running);
    assert!(r.out_dir().is_dir());
    assert!(out.contains(&ReconcileOutcome::Unchanged {
        attempt_id: a.record.attempt_id.clone(),
        state: AttemptState::ExitedOk
    }));
}

#[test]
fn reconcile_crash_window_matrix() {
    // (a) crash before rename: out/ present, record verified -> unchanged, nothing moved
    let t = TempRoot::new();
    let a = verified(&t.0, "a");
    assert_eq!(
        outcome(&t.0, true),
        vec![ReconcileOutcome::Unchanged {
            attempt_id: a.record.attempt_id.clone(),
            state: AttemptState::Verified
        }]
    );
    assert!(a.out_dir().is_dir());

    // (b) crash after rename, before record update, sha matches -> converge to promoted
    fs::rename(a.out_dir(), a.final_path()).unwrap();
    assert_eq!(
        outcome(&t.0, true),
        vec![ReconcileOutcome::ConvergedPromoted {
            attempt_id: a.record.attempt_id.clone()
        }]
    );
    assert_eq!(read_record(&a.dir()).unwrap().state, AttemptState::Promoted);

    // (c) same window but the final manifest differs -> Unknown, record untouched
    let t = TempRoot::new();
    let b = verified(&t.0, "b");
    fs::rename(b.out_dir(), b.final_path()).unwrap();
    fs::write(b.final_path().join(MANIFEST_FILE), b"tampered").unwrap();
    assert!(matches!(
        outcome(&t.0, true).as_slice(),
        [ReconcileOutcome::Unknown { .. }]
    ));
    assert_eq!(read_record(&b.dir()).unwrap().state, AttemptState::Verified);
    assert_eq!(
        fs::read(b.final_path().join(MANIFEST_FILE)).unwrap(),
        b"tampered"
    );

    // (d) out/ gone and final path absent -> Unknown
    let t = TempRoot::new();
    let c = verified(&t.0, "c");
    fs::remove_dir_all(c.out_dir()).unwrap();
    assert!(matches!(
        outcome(&t.0, true).as_slice(),
        [ReconcileOutcome::Unknown { .. }]
    ));

    // (e) out/ gone and a symlinked final path -> Unknown (never follow it)
    let t = TempRoot::new();
    let d = verified(&t.0, "d");
    let real = t.0.join("real");
    fs::rename(d.out_dir(), &real).unwrap();
    symlink(&real, d.final_path()).unwrap();
    assert!(matches!(
        outcome(&t.0, true).as_slice(),
        [ReconcileOutcome::Unknown { .. }]
    ));
    assert_eq!(read_record(&d.dir()).unwrap().state, AttemptState::Verified);
}

#[test]
fn legacy_adapter_without_manifest_is_never_touched() {
    let t = TempRoot::new();
    let legacy = t.0.join("adapters/legacy");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("adapters.safetensors"), b"old").unwrap();
    assert!(matches!(
        create_attempt(&t.0, &spec("legacy")),
        Err(StagingError::NameTaken(_))
    ));
    let mut a = verified(&t.0, "fresh");
    promote(&mut a, &()).unwrap();
    reconcile(&t.0);
    let names: Vec<_> = fs::read_dir(&legacy)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 1);
    assert_eq!(
        fs::read(legacy.join("adapters.safetensors")).unwrap(),
        b"old"
    );
}

// --- protection helper / validation ------------------------------------------------------

#[test]
fn staging_paths_are_protected_and_adapters_are_not() {
    let t = TempRoot::new();
    let a = create_attempt(&t.0, &spec("n")).unwrap();
    assert!(staging_path_is_protected(&t.0, &t.0.join(STAGING_DIR)));
    assert!(staging_path_is_protected(&t.0, &a.out_dir()));
    assert!(staging_path_is_protected(
        &t.0,
        &t.0.join("adapter-staging/not-yet/out")
    ));
    assert!(staging_path_is_protected(
        &t.0,
        &t.0.join("adapters/../adapter-staging/x")
    ));
    assert!(!staging_path_is_protected(&t.0, &t.0.join("adapters/n")));
    assert!(!staging_path_is_protected(
        &t.0,
        &t.0.join("adapter-staging-sibling")
    ));
    symlink(t.0.join(STAGING_DIR), t.0.join("adapters/via-link")).unwrap();
    assert!(staging_path_is_protected(
        &t.0,
        &t.0.join("adapters/via-link/id/out")
    ));
}

#[test]
fn adapter_name_validation_rejects_traversal_leading_dot_and_overlong() {
    let t = TempRoot::new();
    for bad in [
        "",
        "..",
        ".hidden",
        "a/b",
        "a b",
        &"x".repeat(MAX_NAME_LEN + 1),
    ] {
        assert!(create_attempt(&t.0, &spec(bad)).is_err(), "{bad:?}");
    }
    assert!(!t.0.join(STAGING_DIR).exists());
    assert!(create_attempt(&t.0, &spec("ok-name_1.2")).is_ok());
}

#[test]
fn transition_table_rejects_verified_and_promoted_targets() {
    let t = TempRoot::new();
    let mut a = create_attempt(&t.0, &spec("n")).unwrap();
    assert!(transition(&mut a, AttemptState::Verified).is_err());
    assert!(transition(&mut a, AttemptState::Promoted).is_err());
    assert!(transition(&mut a, AttemptState::ExitedOk).is_err());
    transition(&mut a, AttemptState::Failed).unwrap();
    assert!(transition(&mut a, AttemptState::Running).is_err());
}

// --- reviewer findings (S1 hardening) ----------------------------------------------------

#[test]
fn promote_refuses_bytes_changed_after_verify_and_leaves_everything_in_place() {
    let t = TempRoot::new();
    let mut a = verified(&t.0, "n");
    let weights = a.out_dir().join("adapters.safetensors");
    fs::write(&weights, b"swapped after verify").unwrap();
    assert!(promote(&mut a, &()).is_err());
    assert!(!a.final_path().exists());
    assert_eq!(fs::read(&weights).unwrap(), b"swapped after verify");
    assert_eq!(a.record.state, AttemptState::Verified);
}

#[test]
fn promote_refuses_bytes_changed_inside_the_rename_window() {
    let t = TempRoot::new();
    let mut a = verified(&t.0, "n");
    let weights = a.out_dir().join("adapters.safetensors");
    let swap = || fs::write(&weights, b"swapped in window").unwrap();
    assert!(promote_with(&mut a, &same_dev, &swap, &|| {}).is_err());
    assert!(!a.final_path().exists());
}

#[test]
fn promote_refuses_a_regenerated_but_different_manifest() {
    let t = TempRoot::new();
    let mut a = verified(&t.0, "n");
    // Internally consistent out/ (valid manifest) that is not the one that was verified.
    fs::write(a.out_dir().join("adapters.safetensors"), b"other weights").unwrap();
    write_manifest(
        &a.out_dir(),
        ManifestContext {
            runtime: "mlx-lm".into(),
            base_model: "base/model".into(),
        },
    )
    .unwrap();
    assert!(promote(&mut a, &()).is_err());
    assert!(!a.final_path().exists());
}

#[test]
fn reconcile_does_not_converge_when_final_weights_differ_from_the_manifest() {
    let t = TempRoot::new();
    let a = verified(&t.0, "n");
    fs::rename(a.out_dir(), a.final_path()).unwrap();
    fs::write(a.final_path().join("adapters.safetensors"), b"different").unwrap();
    assert!(matches!(
        outcome(&t.0, true).as_slice(),
        [ReconcileOutcome::Unknown { .. }]
    ));
    assert_eq!(read_record(&a.dir()).unwrap().state, AttemptState::Verified);
}

#[test]
fn reconcile_does_not_converge_with_a_copied_manifest_next_to_foreign_weights() {
    let t = TempRoot::new();
    let a = verified(&t.0, "n");
    let manifest = fs::read(a.out_dir().join(MANIFEST_FILE)).unwrap();
    let final_path = a.final_path();
    fs::create_dir(&final_path).unwrap();
    fs::write(final_path.join(MANIFEST_FILE), manifest).unwrap();
    fs::write(final_path.join("adapters.safetensors"), b"foreign").unwrap();
    fs::remove_dir_all(a.out_dir()).unwrap();
    assert!(matches!(
        outcome(&t.0, true).as_slice(),
        [ReconcileOutcome::Unknown { .. }]
    ));
    assert_eq!(read_record(&a.dir()).unwrap().state, AttemptState::Verified);
}

#[test]
fn hardlinked_output_file_is_refused() {
    let t = TempRoot::new();
    let mut a = create_attempt(&t.0, &spec("h")).unwrap();
    mark_running(&mut a, 1, Some(1)).unwrap();
    let outside = t.0.join("outside.bin");
    fs::write(&outside, b"weights").unwrap();
    fs::hard_link(&outside, a.out_dir().join("adapters.safetensors")).unwrap();
    fs::write(a.out_dir().join("adapter_config.json"), b"{}").unwrap();
    transition(&mut a, AttemptState::ExitedOk).unwrap();
    assert!(verify_out(&mut a).is_err());
    assert_eq!(a.record.state, AttemptState::ExitedOk);
}

#[test]
fn post_rename_persist_failure_is_not_an_error_and_reconcile_converges() {
    use std::os::unix::fs::PermissionsExt;
    let t = TempRoot::new();
    let mut a = verified(&t.0, "n");
    let dir = a.dir();
    // r-x: the rename already happened, but attempt.json can no longer be rewritten.
    let lock = || fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
    let p = promote_with(&mut a, &same_dev, &|| {}, &lock);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let p = p.expect("rename succeeded; a retry must not see VerifiedUnpromoted");
    assert!(p.final_path.join("adapters.safetensors").is_file());
    assert!(p.warning.is_some());
    assert_ne!(a.record.state, AttemptState::VerifiedUnpromoted);
    assert_eq!(read_record(&dir).unwrap().state, AttemptState::Verified);
    assert!(matches!(
        outcome(&t.0, true).as_slice(),
        [ReconcileOutcome::ConvergedPromoted { .. }]
    ));
}

#[test]
fn only_a_missing_out_dir_counts_as_gone() {
    assert!(out_gone(Err(io::Error::from(io::ErrorKind::NotFound))).unwrap());
    assert!(out_gone(Err(io::Error::from(io::ErrorKind::PermissionDenied))).is_err());
    let t = TempRoot::new();
    assert!(!out_gone(fs::symlink_metadata(&t.0)).unwrap());
}
