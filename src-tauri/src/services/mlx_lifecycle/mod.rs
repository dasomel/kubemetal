//! MLX 프로세스 수명주기(고아 프로세스 탐지 및 MLflow run 상태 수렴) 서비스 — GitHub #13.

pub mod admission;
pub mod marker;
pub mod reconcile;

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

/// 앱 시작 시 고아 MLX 프로세스 탐지 IPC 커맨드(GitHub #13).
/// marker 디렉터리와 pid 생존 여부를 검사해 살아 있는 프로세스 목록을 반환한다.
#[tauri::command]
pub async fn check_for_orphaned_mlx_processes() -> Result<OrphanScan, String> {
    let home = match std::env::var("HOME").map(PathBuf::from) {
        Ok(h) => h,
        Err(e) => return Err(format!("Failed to determine HOME directory: {e}")),
    };
    let dir = marker_dir(&home);
    scan_orphaned_mlx_processes(&dir).await
}
