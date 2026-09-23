//! 프로세스 스폰 admission 게이트(D40, GitHub #32) 순수 보조 로직.
//!
//! `run_mlx_finetune`과 `start_model_serving`에서 프로세스 스폰 직전 admission 검사 결과에 따라
//! 이미 점유해 둔 상태(학습 진행 상태, 서빙 슬롯)를 안전하게 원복(rollback)하는 순수 로직을 담당한다.

/// admission 검사가 거부된 경우 슬롯을 이전 상태(`prev_state`)로 원복한다.
pub(crate) fn rollback_admission_slot<T>(slot: &mut Option<T>, prev_state: Option<T>) {
    *slot = prev_state;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollback_admission_slot_restores_previous_state() {
        let mut slot = Some("running_new".to_string());
        let prev = Some("running_old".to_string());

        rollback_admission_slot(&mut slot, prev);

        assert_eq!(slot, Some("running_old".to_string()));
    }

    #[test]
    fn rollback_admission_slot_clears_slot_when_prev_is_none() {
        let mut slot = Some("serving_slot".to_string());

        rollback_admission_slot(&mut slot, None);

        assert_eq!(slot, None);
    }
}
