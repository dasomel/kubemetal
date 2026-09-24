/**
 * 학습 상태 집합의 프런트 단일 출처.
 *
 * 이 파일이 있는 이유: 같은 판정이 네 곳에 복사돼 있었고(`App.tsx`, `useMlx.ts`,
 * `PipelineView.tsx`, `MlxFineTuneCard.tsx`), 전부 `status !== 'done' && status !== 'error'`
 * 라는 **배제** 형태였다. `killed`가 그 집합에 없어서 중지된 학습이 어디서는 배지로,
 * 어디서는 "학습 진행 중"으로 남았고, `useMlx`에서는 3초 폴링이 영원히 멈추지 않았다
 * (실측 2026-08-21~22). 값이 하나 늘어난 순간 네 곳이 함께 조용히 틀린 것이다.
 *
 * 알려진 상태는 두 배열에 열거한다. 백엔드와 어긋나면 Rust 쪽
 * `training_status_sets_match_frontend` 테스트가 종료/비종료 집합을 각각 대조해 실패한다.
 *
 * 백엔드 출처: `src-tauri/src/services/mlx_lifecycle/admission.rs`의 `TRAINING_STATUSES`.
 */

/** 아직 결말이 나지 않은 상태 — 진행 표시·폴링·배지가 살아 있어야 한다. */
export const ACTIVE_TRAINING_STATUSES = [
  'running',
  'paused',
  'paused_memory_pressure',
  'paused_battery',
  'paused_thermal',
] as const;

/** 결말이 난 상태 — 덮어쓰지 않고, 진행 표시를 멈춘다. */
export const TERMINAL_TRAINING_STATUSES = ['done', 'error', 'killed'] as const;

/**
 * 학습이 아직 진행 중인가(일시정지 포함).
 *
 * D22: 모르는 문자열의 완료를 단정하지 않는다. 백엔드가 새 학습을 막는 동안 중지와
 * 폴링을 유지한다(비용: 알 수 없는 상태도 계속 폴링, 탈출: 사용자 중지).
 * null/undefined는 학습 슬롯 자체가 없는 경우다.
 */
export function isTrainingActive(status: string | undefined | null): boolean {
  return status != null && !(TERMINAL_TRAINING_STATUSES as readonly string[]).includes(status);
}
