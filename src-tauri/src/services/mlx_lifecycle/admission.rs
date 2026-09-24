//! `run_mlx_finetune` 재요청 거부 판정 (GitHub #101).
//!
//! 진입 가드는 예전에 `status == "running"`만 거부했다. 가드레일(guardrails.rs, D16/D28)이
//! SIGSTOP해 만드는 `paused` | `paused_memory_pressure` | `paused_battery` | `paused_thermal`
//! (`TrainingStatus` 주석 참조)은 걸러지지 않아, 일시정지된 학습이 있어도 새 요청이 슬롯을
//! 덮어썼다 — 멈춘 프로세스는 추적 밖으로 빠지고 그 `spawn_guardrail_loop`는 pid 불일치로
//! 종료된다. 종착 상태(done/error/killed) 판정은 `commands::mlx::should_record_exit`와 같은
//! 기준이어야 하므로, 그 기준을 여기 한 곳에만 두고 양쪽이 재사용한다.

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrainingStatusClass {
    NonTerminal,
    Terminal,
}

/// 백엔드가 생성하는 학습 상태 전체. TS의 두 배열과 정확히 같은 집합인지 테스트한다.
pub const TRAINING_STATUSES: &[(&str, TrainingStatusClass)] = &[
    ("running", TrainingStatusClass::NonTerminal),
    ("paused", TrainingStatusClass::NonTerminal),
    ("paused_memory_pressure", TrainingStatusClass::NonTerminal),
    ("paused_battery", TrainingStatusClass::NonTerminal),
    ("paused_thermal", TrainingStatusClass::NonTerminal),
    ("done", TrainingStatusClass::Terminal),
    ("error", TrainingStatusClass::Terminal),
    ("killed", TrainingStatusClass::Terminal),
];

/// 학습 상태가 아직 결말이 나지 않았는가(= 진행 중으로 간주해 새 요청을 거부해야 하는가).
///
/// 종착 상태는 `done`/`error`/`killed` 뿐이다. `running`과 모든 `paused*`는 비종료다.
pub fn is_non_terminal_training_status(status: &str) -> bool {
    !TRAINING_STATUSES
        .iter()
        .any(|(known, class)| *known == status && *class == TrainingStatusClass::Terminal)
}

/// 비종료 학습이 있을 때 새 `run_mlx_finetune` 요청에 반환할 거부 메시지.
///
/// 상태와 pid를 항상 담는다. `paused*` 상태면 사용자가 취해야 할 다음 행동(재개 또는 중지
/// 후 재시작)을 안내한다 — 그냥 거부만 하면 슬롯이 고아처럼 보여 사용자가 막힌 이유를 알 수 없다.
pub fn in_progress_rejection_message(status: &str, pid: u32) -> String {
    if status.starts_with("paused") {
        format!(
            "Training is already in progress (status: {status}, PID {pid}). \
             Resume or stop it before starting a new one."
        )
    } else {
        format!("Training is already in progress (status: {status}, PID {pid}).")
    }
}
