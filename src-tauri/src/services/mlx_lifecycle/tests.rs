use super::*;
use std::path::PathBuf;

fn make_temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kubemetal-test-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// macOS의 python/python3는 spawn 직후 자신을 Python.framework 경로로 재실행(re-exec)한다
/// (실측: CommandLineTools `/usr/bin/python3`, Homebrew venv 파이썬 모두 재실행 후
/// `.../Python.app/Contents/MacOS/Python`, 2026-09-28, GitHub #13 HIGH-2). 스폰 직후 곧바로
/// 스캔하면 재실행 전 argv로 경합해 통과하므로, classify_mlx_cmdline의 재실행 후 basename
/// 판정 회귀(HIGH-1)를 이 테스트가 잡지 못했다. argv가 스폰 시점 값과 달라지거나(재실행
/// 완료) 최대 1.5초가 지날 때까지 폴링한 뒤 스캔한다.
async fn wait_for_argv_settled(pid: u32) {
    let initial = crate::services::process::get_process_cmdline(pid)
        .await
        .unwrap_or_default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
    loop {
        if std::time::Instant::now() >= deadline {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if let Ok(current) = crate::services::process::get_process_cmdline(pid).await {
            if current != initial {
                return;
            }
        }
    }
}

#[tokio::test]
async fn scan_returns_empty_when_dir_does_not_exist() {
    let dir = make_temp_dir("nonexistent-dir").join("sub");
    let result = scan_orphaned_mlx_processes(&dir, &[]).await.unwrap();
    assert!(result.orphans.is_empty());
    assert!(result.unreadable.is_empty());
}

#[tokio::test]
async fn scan_detects_live_pid_marker() {
    let dir = make_temp_dir("live-marker");
    let script_dir = make_temp_dir("dummy-script");
    let script_path = script_dir.join("scripts/mlx/finetune_wrapper.py");
    std::fs::create_dir_all(script_path.parent().unwrap()).unwrap();
    std::fs::write(&script_path, "import time\ntime.sleep(5)\n").unwrap();
    let mut child = std::process::Command::new("python3")
        .arg(&script_path)
        .spawn()
        .expect("failed to spawn dummy finetune_wrapper");
    let pid = child.id();
    let marker_file = dir.join(format!("training-{pid}.pid"));
    std::fs::write(&marker_file, pid.to_string()).unwrap();

    wait_for_argv_settled(pid).await;
    let result = scan_orphaned_mlx_processes(&dir, &[]).await.unwrap();
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

    let result = scan_orphaned_mlx_processes(&dir, &[]).await.unwrap();
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

    let result = scan_orphaned_mlx_processes(&dir, &[]).await.unwrap();
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

    let result = scan_orphaned_mlx_processes(&dir, &[]).await.unwrap();
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

    let result = scan_orphaned_mlx_processes(&dir, &[]).await.unwrap();
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

    let result = scan_orphaned_mlx_processes(&dir, &[]).await.unwrap();
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

/// 실측 argv 형태(2026-09-28, GitHub #13 HIGH-1): `~/.kubemetal/venv/bin/python3` 및
/// macOS CommandLineTools `/usr/bin/python3` 모두 spawn ~1.5초 후 `ps -o command=`에서
/// 대문자 `Python` basename의 프레임워크 재실행(re-exec) 경로로 관측됐다. 이 정확한 경로에
/// `-m mlx_lm`을 붙인 형태가 Mlx로 분류되지 않으면 실행 중인 프로세스가 NotMlx로 오판되어
/// Stop이 SIGKILL을 못 보내고 orphan 스캔이 진짜 고아의 marker를 지운다.
#[test]
fn classify_cmdline_accepts_measured_framework_python_reexec_paths() {
    let venv_reexec = "/opt/homebrew/Cellar/python@3.14/3.14.7/Frameworks/Python.framework/Versions/3.14/Resources/Python.app/Contents/MacOS/Python -m mlx_lm server --port 8080";
    assert_eq!(
        classify_mlx_cmdline(Some(venv_reexec)),
        CmdlineVerification::Mlx(venv_reexec.into())
    );

    let cli_tools_reexec = "/Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework/Versions/3.9/Resources/Python.app/Contents/MacOS/Python -m mlx_vlm.server --host 127.0.0.1";
    assert_eq!(
        classify_mlx_cmdline(Some(cli_tools_reexec)),
        CmdlineVerification::Mlx(cli_tools_reexec.into())
    );

    let library_frameworks_reexec = "/Library/Frameworks/Python.framework/Versions/3.10/Resources/Python.app/Contents/MacOS/Python scripts/mlx/finetune_wrapper.py --model foo";
    assert_eq!(
        classify_mlx_cmdline(Some(library_frameworks_reexec)),
        CmdlineVerification::Mlx(library_frameworks_reexec.into())
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
    let r3 = "vim finetune_wrapper.py";
    assert_eq!(classify_mlx_cmdline(Some(r3)), CmdlineVerification::NotMlx);
    let r4 = "less mlx_lm.log";
    assert_eq!(classify_mlx_cmdline(Some(r4)), CmdlineVerification::NotMlx);
    let r5 = "python3 /tmp/finetune_wrapper.py";
    assert_eq!(classify_mlx_cmdline(Some(r5)), CmdlineVerification::NotMlx);
    let r6 = "python3.evil -m mlx_lm server";
    assert_eq!(classify_mlx_cmdline(Some(r6)), CmdlineVerification::NotMlx);
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
fn orphan_termination_allows_pid_in_fresh_orphan_scan() {
    let scan = OrphanScan {
        orphans: vec![OrphanedProcessInfo {
            pid: 42,
            kind: "training".into(),
            cmdline: "python -m mlx_lm.lora".into(),
            verified: true,
        }],
        unreadable: vec![],
    };

    assert_eq!(
        orphan_termination_candidate(&scan, &[], 42).map(|orphan| orphan.pid),
        Some(42)
    );
}

#[test]
fn orphan_termination_refuses_pid_missing_from_fresh_scan() {
    let scan = OrphanScan {
        orphans: vec![],
        unreadable: vec![],
    };

    assert_eq!(orphan_termination_candidate(&scan, &[], 42), None);
}

#[test]
fn orphan_termination_refuses_session_tracked_pid() {
    let scan = OrphanScan {
        orphans: vec![OrphanedProcessInfo {
            pid: 42,
            kind: "serving".into(),
            cmdline: "python -m mlx_lm server".into(),
            verified: true,
        }],
        unreadable: vec![],
    };

    assert_eq!(orphan_termination_candidate(&scan, &[42], 42), None);
}

#[test]
fn wait_for_process_exit_returns_true_once_process_dies_within_attempts() {
    use std::cell::Cell;
    // 처음 2번은 살아있다고 보고하고, 3번째 확인부터 종료된 것으로 본다(Metal teardown 지연 모사).
    let calls = Cell::new(0u32);
    let exited = wait_for_process_exit(
        || {
            let n = calls.get();
            calls.set(n + 1);
            n < 2
        },
        5,
        Duration::ZERO,
    );
    assert!(exited);
    assert_eq!(
        calls.get(),
        3,
        "3번째 확인에서 종료를 감지하고 즉시 멈춰야 한다"
    );
}

#[test]
fn wait_for_process_exit_returns_false_when_process_never_dies() {
    let exited = wait_for_process_exit(|| true, 3, Duration::ZERO);
    assert!(!exited);
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
    let result = scan_orphaned_mlx_processes(&dir, &[]).await;
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
