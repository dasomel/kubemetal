use super::super::scan_orphaned_mlx_processes;
use super::{is_tracked_pid, tracked_mlx_pids};
use crate::commands::mlx::{MlxRuntime, MlxState, ServingStatus, TrainingStatus};
use crate::services::process::external_command;

fn state_with_pids(training_pid: u32, serving_pid: u32) -> MlxState {
    let state = MlxState::default();
    *state.training.lock().unwrap() = Some(TrainingStatus {
        pid: training_pid,
        status: "paused_thermal".into(),
        current_iter: 0,
        total_iters: 1,
        last_loss: None,
        adapter_path: None,
        error: None,
        adapter_name: "test".into(),
        mlflow_run_id: None,
    });
    *state.serving.lock().unwrap() = Some(ServingStatus {
        pid: serving_pid,
        port: 8080,
        model_path: "test".into(),
        adapter_path: None,
        runtime: MlxRuntime::MlxLm,
    });
    state
}

#[test]
fn tracked_pid_exclusion_matches_both_slots_but_never_zero() {
    let state = state_with_pids(41, 42);
    let tracked = tracked_mlx_pids(&state).unwrap();
    assert_eq!(tracked, [41, 42]);
    assert!(is_tracked_pid(41, &tracked));
    assert!(is_tracked_pid(42, &tracked));
    assert!(!is_tracked_pid(43, &tracked));
    assert!(!is_tracked_pid(0, &[0]));
    assert!(!is_tracked_pid(41, &[]));
}

#[test]
fn empty_and_placeholder_slots_do_not_exclude_any_pid() {
    assert!(tracked_mlx_pids(&MlxState::default()).unwrap().is_empty());
    assert!(tracked_mlx_pids(&state_with_pids(0, 0)).unwrap().is_empty());
    assert_eq!(tracked_mlx_pids(&state_with_pids(0, 42)).unwrap(), [42]);
    assert_eq!(tracked_mlx_pids(&state_with_pids(41, 0)).unwrap(), [41]);
}

#[test]
fn poisoned_slots_fail_instead_of_claiming_no_owned_processes() {
    let training = MlxState::default();
    let _ = std::panic::catch_unwind(|| {
        let _guard = training.training.lock().unwrap();
        panic!("poison training slot");
    });
    assert!(tracked_mlx_pids(&training)
        .unwrap_err()
        .contains("training"));

    let serving = MlxState::default();
    let _ = std::panic::catch_unwind(|| {
        let _guard = serving.serving.lock().unwrap();
        panic!("poison serving slot");
    });
    assert!(tracked_mlx_pids(&serving).unwrap_err().contains("serving"));
}

#[tokio::test]
async fn scan_excludes_session_markers_without_deleting_them() {
    let dir = std::env::temp_dir().join(format!("kubemetal-session-scan-{}", std::process::id()));
    tokio::fs::create_dir_all(&dir).await.unwrap();
    let script = dir.join("finetune_wrapper.py");
    tokio::fs::write(&script, "read line\n").await.unwrap();
    let spawn = || {
        external_command("sh")
            .unwrap()
            .arg(&script)
            .stdin(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap()
    };
    let mut owned = spawn();
    let mut orphan = spawn();
    let training_pid = owned.id().unwrap();
    // MLX가 아닌 테스트 프로세스도 추적 중이면 ps 조회/삭제 전에 제외해야 한다.
    let serving_pid = std::process::id();
    let orphan_pid = orphan.id().unwrap();
    let markers = dir.join("markers");
    tokio::fs::create_dir_all(&markers).await.unwrap();
    let entries = [
        ("training", training_pid),
        ("serving", serving_pid),
        ("training", orphan_pid),
    ];
    for (kind, pid) in entries {
        tokio::fs::write(markers.join(format!("{kind}-{pid}.pid")), pid.to_string())
            .await
            .unwrap();
    }

    let tracked = tracked_mlx_pids(&state_with_pids(training_pid, serving_pid)).unwrap();
    let scan = scan_orphaned_mlx_processes(&markers, &tracked)
        .await
        .unwrap();
    owned.kill().await.unwrap();
    orphan.kill().await.unwrap();
    let mut markers_preserved = true;
    for (kind, pid) in entries {
        markers_preserved &= tokio::fs::read_to_string(markers.join(format!("{kind}-{pid}.pid")))
            .await
            .is_ok_and(|content| content == pid.to_string());
    }
    tokio::fs::remove_dir_all(&dir).await.unwrap();
    assert_eq!(
        scan.orphans.len(),
        1,
        "only the untracked marker is an orphan: {scan:?}"
    );
    assert_eq!(scan.orphans[0].pid, orphan_pid);
    assert!(scan.unreadable.is_empty());
    assert!(markers_preserved, "live markers must survive the scan");
}
