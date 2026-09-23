//! MLX 프로세스 수명주기(고아 프로세스 탐지 및 MLflow run 상태 수렴) 서비스 로직 — GitHub #13.
//!
//! `commands/mlx.rs`가 비대해지는 것을 방지하기 위해 프로세스 마커 관리, 고아 탐지,
//! 명령줄 검증, MLflow 리컨실리에이션 판정 등의 순수 로직을 이 모듈로 분리한다.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 고아 MLX 프로세스 정보.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrphanedProcessInfo {
    pub pid: u32,
    pub kind: String,
    pub cmdline: String,
}

/// pid marker 파일 경로를 생성한다.
pub fn pid_marker_path(base_dir: &Path, kind: &str) -> PathBuf {
    base_dir.join(".kubemetal").join(format!("mlx-{kind}.pid"))
}

/// 스폰 성공 직후(실제 child pid를 얻은 시점) marker 파일에 pid를 기록한다.
/// 쓰기 실패는 학습/서빙 자체를 막지 않으나 조용히 삼키지 않고 로그로 남긴다(D22).
pub fn write_pid_marker(marker_path: &Path, pid: u32) -> Result<(), String> {
    if let Some(parent) = marker_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
    }
    std::fs::write(marker_path, pid.to_string())
        .map_err(|e| format!("Failed to write pid marker {}: {e}", marker_path.display()))
}

/// marker 파일이 여전히 이 pid를 가리킬 때만 지운다.
/// 재시작 레이스에서 방금 새로 쓴 marker를 이전 프로세스의 reader가 지우지 않도록 방어한다.
pub fn remove_pid_marker_if_matches(marker_path: &Path, pid: u32) {
    if let Ok(content) = std::fs::read_to_string(marker_path) {
        if content.trim().parse::<u32>() == Ok(pid) {
            let _ = std::fs::remove_file(marker_path);
        }
    }
}

/// marker 파일 안의 pid를 읽고 생존 여부로 고아 여부를 판정한다.
/// - marker 없음/파싱 불가 → `None`.
/// - marker 있고 pid가 죽어있음 → marker를 지우고 `None` (D22: 죽은 프로세스를 고아라고 지어내지 않는다).
/// - marker 있고 pid가 살아있음 → `Some(pid)`.
pub fn detect_orphan_from_marker(marker_path: &Path) -> Option<u32> {
    let content = std::fs::read_to_string(marker_path).ok()?;
    let pid: u32 = match content.trim().parse() {
        Ok(p) => p,
        Err(_) => {
            let _ = std::fs::remove_file(marker_path);
            return None;
        }
    };
    if crate::services::process::pid_is_alive(pid) {
        Some(pid)
    } else {
        let _ = std::fs::remove_file(marker_path);
        None
    }
}

/// 프로세스 명령줄이 실제 MLX 관련(mlx_lm, mlx_vlm, finetune_wrapper.py)인지 검증한다.
/// pid 재사용으로 다른 프로세스가 해당 pid를 가졌거나 명령줄을 읽을 수 없는 경우
/// "확인 불가"로 반환하여 살아 있는 MLX라고 지어내지 않는다(D22).
pub fn verify_mlx_cmdline(raw_cmdline: Option<&str>) -> String {
    match raw_cmdline {
        Some(cmd) => {
            let trimmed = cmd.trim();
            if trimmed.contains("mlx_lm")
                || trimmed.contains("mlx_vlm")
                || trimmed.contains("finetune_wrapper.py")
            {
                trimmed.to_string()
            } else {
                "확인 불가".to_string()
            }
        }
        None => "확인 불가".to_string(),
    }
}

/// 특정 kind("training" 또는 "serving")에 대한 고아 프로세스를 탐지한다.
/// 자동으로 프로세스를 kill하지 않고 사용자 판단을 위해 보고만 수행한다.
pub async fn detect_orphaned_mlx_process(
    marker_path: &Path,
    kind: &str,
) -> Option<OrphanedProcessInfo> {
    let pid = detect_orphan_from_marker(marker_path)?;
    let raw_cmdline = crate::services::process::get_process_cmdline(pid)
        .await
        .ok();
    let cmdline = verify_mlx_cmdline(raw_cmdline.as_deref());
    Some(OrphanedProcessInfo {
        pid,
        kind: kind.to_string(),
        cmdline,
    })
}

/// 앱 시작 시 고아 MLX 프로세스 탐지 IPC 커맨드(GitHub #13).
/// marker 파일과 pid 생존 여부를 검사해 살아 있는 프로세스 목록을 반환한다.
#[tauri::command]
pub async fn check_for_orphaned_mlx_processes() -> Vec<OrphanedProcessInfo> {
    let home = match std::env::var("HOME").map(PathBuf::from) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[mlx] Could not determine HOME: {e}");
            return Vec::new();
        }
    };
    let mut results = Vec::new();
    for kind in ["training", "serving"] {
        let marker = pid_marker_path(&home, kind);
        if let Some(info) = detect_orphaned_mlx_process(&marker, kind).await {
            results.push(info);
        }
    }
    results
}

#[derive(Debug, Clone, PartialEq)]
pub struct MlflowRunReconciliation {
    pub run_id: String,
    pub status: &'static str,
}

/// MLflow run 종료 상태 강제 수렴(GitHub #13)이 필요한지 판정하는 순수 함수.
///
/// `finetune_wrapper.py`는 정상 흐름(성공/실패 모두)에서 항상 자기 `reporter.end_run(...)`을
/// 먼저 부른 뒤에만 종료한다. 따라서 프로세스가 시그널로 죽지 않는 한 wrapper가 이미 종결 상태를
/// 남겼다고 볼 수 있다.
///
/// - `wrapper_reported_terminal`이 true면 항상 `None` (wrapper가 이미 처리함).
/// - `run_id`가 없으면 `None` (대상 없음, D22: 지어내지 않는다).
/// - 시그널로 종료됐으면 `Some(KILLED)`.
/// - 정상 exit code면 `None`.
pub fn mlflow_reconciliation_decision(
    status: &str,
    run_id: Option<&str>,
    wrapper_reported_terminal: bool,
    exit: Option<&std::process::ExitStatus>,
) -> Option<MlflowRunReconciliation> {
    let _ = status;
    if wrapper_reported_terminal {
        return None;
    }
    let run_id = run_id?;
    let signaled = exit.is_some_and(|e| {
        #[cfg(unix)]
        {
            std::os::unix::process::ExitStatusExt::signal(e).is_some()
        }
        #[cfg(not(unix))]
        {
            let _ = e;
            false
        }
    });
    if !signaled {
        return None;
    }
    Some(MlflowRunReconciliation {
        run_id: run_id.to_string(),
        status: "KILLED",
    })
}

/// `MlflowRunReconciliation`을 MLflow REST API로 반영한다.
/// external_command("curl") + ports::local_url("mlflow")(127.0.0.1:5001, D1)를 따른다.
pub async fn reconcile_mlflow_run(reconciliation: MlflowRunReconciliation) {
    let body = serde_json::json!({
        "run_id": reconciliation.run_id,
        "status": reconciliation.status,
    })
    .to_string();

    let cmd = match crate::services::process::external_command("curl") {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "[mlx] Failed to reconcile MLflow run {}: {e}",
                reconciliation.run_id
            );
            return;
        }
    };
    let mut cmd = cmd;
    let url = format!(
        "{}/api/2.0/mlflow/runs/update",
        crate::services::ports::local_url("mlflow")
    );
    let output = cmd
        .args([
            "-s",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-d",
            &body,
            &url,
        ])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => {}
        Ok(out) => eprintln!(
            "[mlx] MLflow run reconciliation for {} returned non-success: {}",
            reconciliation.run_id,
            String::from_utf8_lossy(&out.stderr)
        ),
        Err(e) => eprintln!(
            "[mlx] Failed to reach MLflow to reconcile run {}: {e}",
            reconciliation.run_id
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_temp_dir(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kubemetal-test-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detect_orphan_from_marker_returns_none_without_marker_file() {
        let dir = make_temp_dir("orphan-no-marker");
        let marker = dir.join("mlx-training.pid");
        assert_eq!(detect_orphan_from_marker(&marker), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn detect_orphan_from_marker_returns_some_for_live_pid() {
        let dir = make_temp_dir("orphan-live-pid");
        let marker = dir.join("mlx-training.pid");
        std::fs::write(&marker, std::process::id().to_string()).unwrap();

        assert_eq!(detect_orphan_from_marker(&marker), Some(std::process::id()));
        assert!(marker.is_file());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn detect_orphan_from_marker_returns_none_and_removes_marker_for_dead_pid() {
        let dir = make_temp_dir("orphan-dead-pid");
        let marker = dir.join("mlx-training.pid");
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("failed to spawn /usr/bin/true");
        let dead_pid = child.id();
        child.wait().expect("failed to wait for child");
        std::fs::write(&marker, dead_pid.to_string()).unwrap();

        assert_eq!(detect_orphan_from_marker(&marker), None);
        assert!(!marker.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn detect_orphan_from_marker_returns_none_and_removes_marker_for_corrupt_content() {
        let dir = make_temp_dir("orphan-corrupt");
        let marker = dir.join("mlx-training.pid");
        std::fs::write(&marker, "not-a-pid").unwrap();

        assert_eq!(detect_orphan_from_marker(&marker), None);
        assert!(!marker.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn verify_mlx_cmdline_identifies_mlx_lm() {
        assert_eq!(
            verify_mlx_cmdline(Some(
                "/path/to/venv/bin/python3 -m mlx_lm server --port 8080"
            )),
            "/path/to/venv/bin/python3 -m mlx_lm server --port 8080"
        );
    }

    #[test]
    fn verify_mlx_cmdline_identifies_mlx_vlm() {
        assert_eq!(
            verify_mlx_cmdline(Some("python3 -m mlx_vlm.server --host 127.0.0.1")),
            "python3 -m mlx_vlm.server --host 127.0.0.1"
        );
    }

    #[test]
    fn verify_mlx_cmdline_identifies_finetune_wrapper() {
        assert_eq!(
            verify_mlx_cmdline(Some("python3 scripts/mlx/finetune_wrapper.py --model foo")),
            "python3 scripts/mlx/finetune_wrapper.py --model foo"
        );
    }

    #[test]
    fn verify_mlx_cmdline_reports_unverifiable_for_unrelated_process() {
        assert_eq!(
            verify_mlx_cmdline(Some("/Applications/Slack.app/Contents/MacOS/Slack")),
            "확인 불가"
        );
    }

    #[test]
    fn verify_mlx_cmdline_reports_unverifiable_when_none() {
        assert_eq!(verify_mlx_cmdline(None), "확인 불가");
    }

    #[test]
    fn mlflow_reconciliation_marks_intentional_kill_as_killed() {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            let exit = std::process::ExitStatus::from_raw(9);
            assert_eq!(
                mlflow_reconciliation_decision("killed", Some("run-123"), false, Some(&exit)),
                Some(MlflowRunReconciliation {
                    run_id: "run-123".into(),
                    status: "KILLED",
                })
            );
        }
    }

    #[test]
    fn mlflow_reconciliation_marks_unhandled_signal_as_killed() {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            let exit = std::process::ExitStatus::from_raw(15);
            assert_eq!(
                mlflow_reconciliation_decision("running", Some("run-123"), false, Some(&exit)),
                Some(MlflowRunReconciliation {
                    run_id: "run-123".into(),
                    status: "KILLED",
                })
            );
        }
    }

    #[test]
    fn mlflow_reconciliation_suppresses_wrapper_terminal_event() {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            let exit = std::process::ExitStatus::from_raw(9);
            assert_eq!(
                mlflow_reconciliation_decision("done", Some("run-123"), true, Some(&exit)),
                None
            );
        }
    }

    #[test]
    fn mlflow_reconciliation_suppresses_missing_run_id() {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            let exit = std::process::ExitStatus::from_raw(9);
            assert_eq!(
                mlflow_reconciliation_decision("killed", None, false, Some(&exit)),
                None
            );
        }
    }

    #[test]
    fn mlflow_reconciliation_suppresses_ordinary_abnormal_exit() {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            let exit = std::process::ExitStatus::from_raw(1 << 8);
            assert_eq!(
                mlflow_reconciliation_decision("running", Some("run-123"), false, Some(&exit)),
                None
            );
        }
    }
}
