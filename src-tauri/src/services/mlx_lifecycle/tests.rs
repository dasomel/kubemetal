use super::*;
use std::path::PathBuf;

fn make_temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kubemetal-test-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn scan_returns_empty_when_dir_does_not_exist() {
    let dir = make_temp_dir("nonexistent-dir").join("sub");
    let result = scan_orphaned_mlx_processes(&dir).await.unwrap();
    assert!(result.orphans.is_empty());
    assert!(result.unreadable.is_empty());
}

#[tokio::test]
async fn scan_detects_live_pid_marker() {
    let dir = make_temp_dir("live-marker");
    let script_dir = make_temp_dir("dummy-script");
    let script_path = script_dir.join("finetune_wrapper.py");
    std::fs::write(&script_path, "sleep 5\n").unwrap();
    let mut child = std::process::Command::new("/bin/sh")
        .arg(&script_path)
        .spawn()
        .expect("failed to spawn dummy finetune_wrapper");
    let pid = child.id();
    let marker_file = dir.join(format!("training-{pid}.pid"));
    std::fs::write(&marker_file, pid.to_string()).unwrap();

    let result = scan_orphaned_mlx_processes(&dir).await.unwrap();
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&script_dir);

    assert_eq!(result.orphans.len(), 1);
    assert_eq!(result.orphans[0].pid, pid);
    assert_eq!(result.orphans[0].kind, "training");
    assert!(result.unreadable.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn scan_removes_marker_for_non_mlx_live_process_and_excludes_from_orphans() {
    let dir = make_temp_dir("non-mlx-live");
    let pid = std::process::id(); // cargo test 프로세스 (MLX 아님)
    let marker_file = dir.join(format!("training-{pid}.pid"));
    std::fs::write(&marker_file, pid.to_string()).unwrap();

    let result = scan_orphaned_mlx_processes(&dir).await.unwrap();
    assert!(result.orphans.is_empty(), "Non-MLX process excluded");
    assert!(result.unreadable.is_empty());
    assert!(!marker_file.exists(), "Marker must be cleaned up");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn scan_removes_dead_pid_marker_and_does_not_report() {
    let dir = make_temp_dir("dead-marker");
    let mut child = std::process::Command::new("/usr/bin/true")
        .spawn()
        .expect("spawn true");
    let dead_pid = child.id();
    child.wait().expect("wait child");

    let marker_file = dir.join(format!("training-{dead_pid}.pid"));
    std::fs::write(&marker_file, dead_pid.to_string()).unwrap();

    let result = scan_orphaned_mlx_processes(&dir).await.unwrap();
    assert!(result.orphans.is_empty());
    assert!(result.unreadable.is_empty());
    assert!(!marker_file.exists(), "Dead PID marker must be cleaned up");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn scan_reports_symlink_as_unreadable_and_does_not_dereference() {
    let dir = make_temp_dir("symlink-marker");
    let target = dir
        .parent()
        .unwrap()
        .join(format!("target-{}.txt", std::process::id()));
    std::fs::write(&target, std::process::id().to_string()).unwrap();

    let link = dir.join(format!("serving-{}.pid", std::process::id()));
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let result = scan_orphaned_mlx_processes(&dir).await.unwrap();
    assert!(result.orphans.is_empty());
    assert_eq!(result.unreadable.len(), 1);
    assert_eq!(result.unreadable[0].path, link.display().to_string());
    assert!(result.unreadable[0].error.contains("Not a regular file"));

    let _ = std::fs::remove_file(&target);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn scan_reports_out_of_range_pid_as_unreadable() {
    let dir = make_temp_dir("range-marker");
    let marker_file = dir.join("training-9999999999.pid");
    std::fs::write(&marker_file, "9999999999").unwrap();

    let result = scan_orphaned_mlx_processes(&dir).await.unwrap();
    assert!(result.orphans.is_empty());
    assert_eq!(result.unreadable.len(), 1);
    assert!(result.unreadable[0].error.contains("outside allowed range"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn scan_reports_corrupt_content_as_unreadable() {
    let dir = make_temp_dir("corrupt-marker");
    let marker_file = dir.join("training-1234.pid");
    std::fs::write(&marker_file, "not-a-pid").unwrap();

    let result = scan_orphaned_mlx_processes(&dir).await.unwrap();
    assert!(result.orphans.is_empty());
    assert_eq!(result.unreadable.len(), 1);
    assert!(result.unreadable[0].error.contains("Failed to parse PID"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn classify_cmdline_identifies_valid_mlx_processes() {
    let lm = "/path/to/venv/bin/python3 -m mlx_lm server --port 8080";
    assert_eq!(
        classify_mlx_cmdline(Some(lm)),
        CmdlineVerification::Mlx(lm.into())
    );
    let vlm = "python3 -m mlx_vlm.server --host 127.0.0.1";
    assert_eq!(
        classify_mlx_cmdline(Some(vlm)),
        CmdlineVerification::Mlx(vlm.into())
    );
    let ft = "python3 scripts/mlx/finetune_wrapper.py --model foo";
    assert_eq!(
        classify_mlx_cmdline(Some(ft)),
        CmdlineVerification::Mlx(ft.into())
    );
}

#[test]
fn classify_cmdline_rejects_false_positive_substrings() {
    // 리뷰의 오탐 예시 1: output 인자에 mlx_lm.log가 포함된 무관한 파이썬 스크립트
    let r1 = "python3 /tmp/report.py --output /tmp/mlx_lm.log";
    assert_eq!(classify_mlx_cmdline(Some(r1)), CmdlineVerification::NotMlx);
    // 리뷰의 오탐 예시 2: finetune_wrapper.py.log를 tail하는 무관한 프로세스
    let r2 = "/usr/bin/tail -f /tmp/finetune_wrapper.py.log";
    assert_eq!(classify_mlx_cmdline(Some(r2)), CmdlineVerification::NotMlx);
    // 일반적인 무관한 프로세스
    let slack = "/Applications/Slack.app/Contents/MacOS/Slack";
    assert_eq!(
        classify_mlx_cmdline(Some(slack)),
        CmdlineVerification::NotMlx
    );
}

#[test]
fn classify_cmdline_unverifiable_on_none_or_empty() {
    assert_eq!(
        classify_mlx_cmdline(None),
        CmdlineVerification::Unverifiable
    );
    assert_eq!(
        classify_mlx_cmdline(Some("   ")),
        CmdlineVerification::Unverifiable
    );
}

#[test]
fn reconciliation_suppresses_when_slot_occupied_by_new_process() {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        let exit = std::process::ExitStatus::from_raw(9);
        // 프로세스 A(pid=100) 종료 전에 프로세스 B(pid=200)가 슬롯을 차지한 상황
        let decision = mlflow_reconciliation_decision(
            "running",
            Some(200),
            100,
            Some("run-B"),
            false,
            Some(&exit),
        );
        assert_eq!(
            decision, None,
            "Must not reconcile when slot disagrees with exited PID"
        );
    }
}

#[test]
fn reconciliation_proceeds_when_slot_matches_exited_pid() {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        let exit = std::process::ExitStatus::from_raw(9);
        let decision = mlflow_reconciliation_decision(
            "killed",
            Some(100),
            100,
            Some("run-A"),
            false,
            Some(&exit),
        );
        assert_eq!(
            decision,
            Some(MlflowRunReconciliation {
                run_id: "run-A".into(),
                status: "KILLED",
            })
        );
    }
}

#[test]
fn reconciliation_suppresses_on_wrapper_terminal_event() {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        let exit = std::process::ExitStatus::from_raw(9);
        let decision = mlflow_reconciliation_decision(
            "done",
            Some(100),
            100,
            Some("run-123"),
            true,
            Some(&exit),
        );
        assert_eq!(decision, None);
    }
}

#[test]
fn reconciliation_suppresses_missing_run_id() {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        let exit = std::process::ExitStatus::from_raw(9);
        let decision =
            mlflow_reconciliation_decision("killed", Some(100), 100, None, false, Some(&exit));
        assert_eq!(decision, None);
    }
}

#[test]
fn reconciliation_suppresses_ordinary_exit_without_signal() {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        let exit = std::process::ExitStatus::from_raw(1 << 8);
        let decision = mlflow_reconciliation_decision(
            "running",
            Some(100),
            100,
            Some("run-123"),
            false,
            Some(&exit),
        );
        assert_eq!(decision, None);
    }
}

#[test]
fn parse_curl_http_response_handles_success_and_error_codes() {
    let (code, body) = parse_curl_http_response("{\"ok\":true}\n200").unwrap();
    assert_eq!(code, 200);
    assert_eq!(body, "{\"ok\":true}");

    let (code, body) =
        parse_curl_http_response("{\"error_code\":\"INTERNAL_ERROR\"}\n500").unwrap();
    assert_eq!(code, 500);
    assert_eq!(body, "{\"error_code\":\"INTERNAL_ERROR\"}");

    assert!(parse_curl_http_response("not-a-code").is_err());
}

#[test]
fn evaluate_reconciliation_result_distinguishes_status_codes() {
    assert!(evaluate_reconciliation_result(true, "{}\n200", "").is_ok());
    assert!(evaluate_reconciliation_result(true, "{}\n204", "").is_ok());

    let err_500 =
        evaluate_reconciliation_result(true, "{\"error_code\":\"INTERNAL_ERROR\"}\n500", "")
            .unwrap_err();
    assert!(err_500.contains("HTTP 500") && err_500.contains("INTERNAL_ERROR"));

    let err_404 =
        evaluate_reconciliation_result(true, "{\"error\":\"not found\"}\n404", "").unwrap_err();
    assert!(err_404.contains("HTTP 404"));

    let err_curl =
        evaluate_reconciliation_result(false, "", "curl: (28) Connection timed out").unwrap_err();
    assert!(err_curl.contains("curl process failed") && err_curl.contains("Connection timed out"));
}

#[test]
fn evaluate_reconciliation_result_truncates_non_ascii_body_without_panicking() {
    // 3바이트 문자로 200바이트 경계가 문자 중간에 걸리게 한다.
    let body = "가".repeat(250);
    let err = evaluate_reconciliation_result(true, &format!("{body}\n502"), "").unwrap_err();
    assert!(err.starts_with("HTTP 502: ") && err.ends_with("..."));
}

#[tokio::test]
async fn scan_errors_instead_of_reporting_empty_when_parent_is_unreadable() {
    // `exists()`는 부모 디렉터리 권한 오류(EACCES)에서도 false를 돌려줘 "고아 없음"이 됐다.
    use std::os::unix::fs::PermissionsExt;
    let parent = make_temp_dir("unreadable-parent");
    let dir = parent.join("mlx-markers");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o000)).unwrap();
    let result = scan_orphaned_mlx_processes(&dir).await;
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::remove_dir_all(&parent).unwrap();
    assert!(result.is_err());
}

/// GitHub #101 — `run_mlx_finetune` 진입 가드가 `status == "running"`만 거부하면,
/// 가드레일이 SIGSTOP한 `paused*` 학습의 슬롯을 새 요청이 덮어쓴다. running과 모든
/// paused* 상태가 비종료로 판정돼야 하고, 종착 상태(done/error/killed)만 통과돼야 한다.
#[test]
fn non_terminal_training_status_rejects_running_and_all_paused_variants() {
    for status in [
        "running",
        "paused",
        "paused_memory_pressure",
        "paused_battery",
        "paused_thermal",
    ] {
        assert!(
            is_non_terminal_training_status(status),
            "{status}는 비종료인데 새 요청이 통과됐다"
        );
    }
    for status in ["done", "error", "killed"] {
        assert!(
            !is_non_terminal_training_status(status),
            "{status}는 종착 상태인데 새 요청이 거부됐다"
        );
    }
}

#[test]
fn in_progress_rejection_message_includes_status_and_pid() {
    let msg = in_progress_rejection_message("running", 4242);
    assert!(msg.contains("running"));
    assert!(msg.contains("4242"));
}

#[test]
fn in_progress_rejection_message_hints_resume_or_stop_when_paused() {
    for status in [
        "paused",
        "paused_memory_pressure",
        "paused_battery",
        "paused_thermal",
    ] {
        let msg = in_progress_rejection_message(status, 1);
        assert!(msg.contains(status));
        assert!(msg.contains('1'));
        assert!(
            msg.to_lowercase().contains("resume") && msg.to_lowercase().contains("stop"),
            "{status} rejection must hint resume/stop: {msg}"
        );
    }
}
