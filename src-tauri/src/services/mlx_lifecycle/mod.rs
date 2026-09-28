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
use tauri::State;

use crate::commands::mlx::MlxState;

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
        !crate::services::process::pid_is_alive(pid)
    })
    .await
    .map_err(|e| format!("Failed to wait for orphaned process termination: {e}"))?;

    if !exited {
        return Err(format!(
            "Orphaned MLX process {pid} did not exit after termination attempt."
        ));
    }

    remove_pid_marker(&home, &orphan.kind, pid).await;

    let fresh_tracked_pids = session::tracked_mlx_pids(&state)?;
    scan_orphaned_mlx_processes(&dir, &fresh_tracked_pids).await
}
