use crate::commands::mlx::MlxState;

#[cfg(test)]
mod tests;

/// I/O 전에 슬롯의 PID만 복사한다. 잠금은 비동기 스캔 동안 유지하지 않는다.
pub(super) fn tracked_mlx_pids(state: &MlxState) -> Result<Vec<u32>, String> {
    // D22: 소유권을 모르면 고아를 단정하지 않는다. 비용은 스캔 실패이며 앱 재시작으로 복구한다.
    let training_pid = state
        .training
        .lock()
        .map_err(|e| format!("Cannot determine tracked training PID: {e}"))?
        .as_ref()
        .map(|training| training.pid);
    let serving_pid = state
        .serving
        .lock()
        .map_err(|e| format!("Cannot determine tracked serving PID: {e}"))?
        .as_ref()
        .map(|serving| serving.pid);
    Ok([training_pid, serving_pid]
        .into_iter()
        .flatten()
        .filter(|pid| *pid != 0)
        .collect())
}

/// pid=0은 스폰 전 슬롯 예약용이므로 소유 프로세스가 아니다.
pub(super) fn is_tracked_pid(pid: u32, tracked_pids: &[u32]) -> bool {
    pid != 0 && tracked_pids.contains(&pid)
}
