//! last-known-good 서빙 구성 판정(이슈 #12 축소 스코프)의 순수 로직. `commands/mlx.rs`가
//! `MlxState`의 Mutex를 잠그고 그 안의 값을 뽑아 여기 넘기는 얇은 호출부를 맡는다
//! (2026-09-23 리뷰 LOW 파인딩 — mlx.rs가 1128→1612줄로 불어난 것 중 일부를 여기로
//! 옮긴다). 이동하면서 동작은 바꾸지 않았다 — `commands::mlx::tests`의 기존
//! `record_serving_success_*`/`revert_config_or_error_*` 통합 테스트가 `MlxState`를
//! 통해 그대로 남아 있고, 이 파일의 테스트는 그 아래 순수 로직만 추가로 좁혀서
//! 검증한다.

/// pid가 여전히 현재 서빙과 일치할 때만 헬스체크 통과 구성을 last-known-good으로
/// 기록해야 한다. 기록 시점 사이에 프로세스가 죽거나 다른 서빙으로 교체됐으면 쓰지
/// 않는다 — 죽은 구성을 "마지막 성공"으로 남기면 되돌리기가 똑같이 죽는 구성으로
/// 돌아간다(D22).
pub(crate) fn should_record_as_last_known_good(
    current_serving_pid: Option<u32>,
    healthcheck_pid: u32,
) -> bool {
    current_serving_pid == Some(healthcheck_pid)
}

/// `revert_to_last_serving`이 되돌릴 대상을 고른다. 저장된 구성이 없으면 지어내지
/// 않고 명확한 에러를 반환한다(D22 — 없는 상태를 지어내지 않는다). 타입 매개변수라
/// `ServingStatus`(commands 계층 타입)를 이 모듈이 알 필요가 없다.
pub(crate) fn pick_revert_target<T>(last_known_good: Option<T>) -> Result<T, String> {
    last_known_good.ok_or_else(|| "되돌릴 이전 구성이 없습니다.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_record_as_last_known_good_when_pid_still_current() {
        assert!(should_record_as_last_known_good(Some(123), 123));
    }

    #[test]
    fn should_not_record_as_last_known_good_when_pid_no_longer_current() {
        assert!(!should_record_as_last_known_good(Some(999), 123));
        assert!(!should_record_as_last_known_good(None, 123));
    }

    #[test]
    fn pick_revert_target_errors_when_nothing_saved() {
        let err = pick_revert_target::<u32>(None).expect_err("되돌릴 이전 구성이 없습니다");
        assert!(err.contains("되돌릴 이전 구성이 없습니다"));
    }

    #[test]
    fn pick_revert_target_returns_saved_value() {
        assert_eq!(pick_revert_target(Some(42)), Ok(42));
    }
}
