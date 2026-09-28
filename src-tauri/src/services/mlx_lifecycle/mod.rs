//! MLX 프로세스 수명주기(고아 프로세스 탐지 및 MLflow run 상태 수렴) 서비스 — GitHub #13.

pub mod admission;
pub mod marker;
pub mod reconcile;
mod session;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use admission::{in_progress_rejection_message, is_non_terminal_training_status};
#[allow(unused_imports)]
pub use marker::{
    classify_mlx_cmdline, marker_dir, pid_marker_path, remove_pid_marker,
    scan_orphaned_mlx_processes, write_pid_marker, CmdlineVerification, OrphanScan,
    OrphanedProcessInfo, UnreadableMarker,
};
#[allow(unused_imports)]
pub use reconcile::{
    evaluate_reconciliation_result, mlflow_reconciliation_decision, parse_curl_http_response,
    reconcile_mlflow_run, MlflowRunReconciliation,
};

use std::path::PathBuf;
use std::time::Duration;
use tauri::State;

use crate::commands::mlx::MlxState;

/// `terminate_orphaned_mlx_process`가 종료 확인을 위해 생존 여부를 다시 검사하는 최대 횟수.
const ORPHAN_EXIT_POLL_ATTEMPTS: u32 = 15;
/// 폴링 간 대기 간격. attempts와 곱하면 약 3초의 상한이 된다.
const ORPHAN_EXIT_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// SIGTERM/SIGKILL 직후에는 Metal teardown 등으로 프로세스가 잠시 더 살아있을 수 있어
/// 단일 `pid_is_alive` 체크는 방금 종료된 프로세스를 "종료 실패"로 오탐한다. `is_alive`를
/// 짧은 간격으로 최대 `attempts`번 재확인해 실제 종료를 기다린다.
/// 순수 함수로 분리해 실제 프로세스 없이 주입된 클로저로 단위 테스트할 수 있게 한다.
fn wait_for_process_exit(
    mut is_alive: impl FnMut() -> bool,
    attempts: u32,
    interval: Duration,
) -> bool {
    for attempt in 0..attempts.max(1) {
        if !is_alive() {
            return true;
        }
        if attempt + 1 < attempts {
            std::thread::sleep(interval);
        }
    }
    false
}

/// 앱 시작 시 고아 MLX 프로세스 탐지 IPC 커맨드(GitHub #13).
/// marker 디렉터리와 pid 생존 여부를 검사해 살아 있는 프로세스 목록을 반환한다.
#[tauri::command]
pub async fn check_for_orphaned_mlx_processes(
    state: State<'_, MlxState>,
) -> Result<OrphanScan, String> {
    let tracked_pids = session::tracked_mlx_pids(&state)?;
    let home = match std::env::var("HOME").map(PathBuf::from) {
        Ok(h) => h,
        Err(e) => return Err(format!("Failed to determine HOME directory: {e}")),
    };
    let dir = marker_dir(&home);
    scan_orphaned_mlx_processes(&dir, &tracked_pids).await
}

/// 현재 스캔에서만 종료를 허용한다. 세션이 소유한 PID는 스캔 결과에 있더라도
/// fail-closed로 거부해, 범용 `kill_mlx_process` 경로로 고아가 아닌 프로세스가
/// 종료되는 우회 경로를 만들지 않는다.
pub(crate) fn orphan_termination_candidate(
    scan: &OrphanScan,
    tracked_pids: &[u32],
    pid: u32,
) -> Option<OrphanedProcessInfo> {
    if session::is_tracked_pid(pid, tracked_pids) {
        return None;
    }
    scan.orphans
        .iter()
        .find(|orphan| orphan.pid == pid)
        .cloned()
}

/// 고아로 재검증된 MLX 프로세스만 종료한다. 재시작 전 UI가 표시했던 결과를 신뢰하지 않고
/// 여기서 marker, 생존 여부, 명령줄, 현재 세션 소유권을 다시 스캔해 PID 재사용을 막는다.
#[tauri::command]
pub async fn terminate_orphaned_mlx_process(
    state: State<'_, MlxState>,
    pid: u32,
) -> Result<OrphanScan, String> {
    let tracked_pids = session::tracked_mlx_pids(&state)?;
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|e| format!("Failed to determine HOME directory: {e}"))?;
    let dir = marker_dir(&home);
    let scan = scan_orphaned_mlx_processes(&dir, &tracked_pids).await?;
    let orphan = orphan_termination_candidate(&scan, &tracked_pids, pid).ok_or_else(|| {
        format!("Process {pid} is no longer an orphaned MLX process and was not terminated.")
    })?;
    let cmdline = crate::services::process::get_process_cmdline(pid)
        .await
        .map_err(|e| format!("Process {pid} is no longer an orphaned MLX process: {e}"))?;
    if !matches!(
        classify_mlx_cmdline(Some(&cmdline)),
        CmdlineVerification::Mlx(_)
    ) {
        return Err(format!(
            "Process {pid} is no longer an orphaned MLX process and was not terminated."
        ));
    }
    let use_process_group = orphan.kind == "training";

    let exited = tokio::task::spawn_blocking(move || {
        crate::commands::mlx::terminate_pid(pid, use_process_group);
        wait_for_process_exit(
            || crate::services::process::pid_is_alive(pid),
            ORPHAN_EXIT_POLL_ATTEMPTS,
            ORPHAN_EXIT_POLL_INTERVAL,
        )
    })
    .await
    .map_err(|e| format!("Failed to wait for orphaned process termination: {e}"))?;

    if !exited {
        return Err(format!(
            "Orphaned MLX process {pid} did not exit within {:.1}s after termination attempt.",
            ORPHAN_EXIT_POLL_ATTEMPTS.saturating_sub(1) as f64
                * ORPHAN_EXIT_POLL_INTERVAL.as_secs_f64()
        ));
    }

    remove_pid_marker(&home, &orphan.kind, pid).await;

    let fresh_tracked_pids = session::tracked_mlx_pids(&state)?;
    scan_orphaned_mlx_processes(&dir, &fresh_tracked_pids).await
}
