//! GPU benchmark workload admission (#11).

use super::mlx_lifecycle::is_non_terminal_training_status;

/// Training is (status, pid); serving is (pid, port), including a claimed pid-0 slot.
pub(crate) fn check_workloads(
    training: Option<(&str, u32)>,
    serving: Option<(u32, u16)>,
) -> Result<(), String> {
    // D22: busy/paused workloads invalidate GPU measurements. Refuse until their slots
    // clear; terminal training history alone does not require the user to clear it.
    if let Some((status, pid)) = training {
        if is_non_terminal_training_status(status) {
            return Err(format!(
                "Cannot run GPU benchmark while training is in progress (status: {status}, PID {pid})."
            ));
        }
    }
    if let Some((pid, port)) = serving {
        return Err(format!(
            "Cannot run GPU benchmark while model serving occupies the slot (PID {pid}, port {port})."
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
