//! MLX 어댑터 산출물(학습 결과 디렉터리)에 대한 순수 판정 로직 — 이슈 #33 축소
//! 스코프(체크포인트 상태 판정 + GC 삭제 가드). `commands/mlx.rs`가 `MlxState`의
//! Mutex를 잠그고 그 안의 값을 뽑아 이 모듈의 순수 함수에 넘기는 얇은 호출부를
//! 맡고, 여기서는 `MlxState`/`tauri` 의존 없이 값만으로 판정한다(2026-09-23 리뷰
//! LOW 파인딩 — mlx.rs가 1128→1612줄로 불어난 것 중 일부를 여기로 옮긴다).
//!
//! HOME 조회 실패 시 삭제를 거부하고, 경로 비교 전에 홈 확장과 canonicalize를
//! 수행한다. `tests`는 전역 환경 변경 없이 HOME 실패와 실제 symlink를 검증한다.

use std::path::{Path, PathBuf};

use crate::services::artifact_manifest::verify_manifest;
use crate::services::home_path::expand_home_path;

/// 어댑터 디렉터리의 매니페스트 검증 상태(이슈 #33). `services::artifact_manifest`의
/// `verify_manifest`를 그대로 재사용해 판정만 셋으로 좁힌다 — 별도 파서/sha256
/// 재계산을 두지 않는다(AGENTS.md "같은 사실 두 곳 금지").
///
/// - manifest.json이 없으면 `"missing"`(#22 이전 산출물이거나 실패한 학습).
/// - 있고 `verify_manifest`가 missing/changed/extra 없이 유효하다고 판정하면 `"verified"`.
/// - 있는데 파싱/스키마 실패, 또는 하나라도 missing/changed/extra면 `"corrupt"`.
pub(crate) fn manifest_verification_status(adapter_dir: &Path) -> &'static str {
    if !adapter_dir.join("manifest.json").is_file() {
        return "missing";
    }
    match verify_manifest(adapter_dir) {
        Ok(report) if report.is_valid() => "verified",
        _ => "corrupt",
    }
}

/// mlx 파인튜닝 래퍼(`scripts/mlx/finetune_wrapper.py`)가 어댑터를 쓰는 출력
/// 디렉터리. `Path.home() / ".kubemetal" / "adapters" / <adapter_name>` — 래퍼 쪽과
/// 같은 사실이므로 여기서 새로 지어내지 않고 그 파일의 실제 동작을 그대로 옮긴다.
pub(crate) fn adapter_output_dir(home: &Path, adapter_name: &str) -> PathBuf {
    home.join(".kubemetal").join("adapters").join(adapter_name)
}

/// 호출부가 조회한 HOME과 활성 슬롯으로 삭제 가능 여부를 판정한다.
/// HOME 조회 결과를 값으로 받아 전역 환경 변경 없이 실패 경로도 검증한다.
/// 슬롯의 `None`은 비어 있다는 뜻이다. Mutex poison처럼 상태를 모르는 경우
/// 호출부는 이 함수를 호출하지 않고 즉시 false를 반환해야 한다(D22).
pub(crate) fn is_adapter_safe_to_delete(
    adapter_dir: &Path,
    home: Option<&Path>,
    serving_adapter_path: Option<&str>,
    last_known_good_adapter_path: Option<&str>,
    in_progress_adapter_name: Option<&str>,
) -> bool {
    // D22: HOME을 모르면 학습 경로도 확인 불가다. 조회가 회복될 때까지
    // 관련 없는 경로도 보수적으로 거부하며, 보호 없음으로 대체하지 않는다.
    let Some(home) = home else {
        return false;
    };
    let adapter_dir = expand_home_path(adapter_dir, home);
    // D-a (#33): CWD에 따라 달라지는 상대경로는 거부한다. 절대경로로 다시 요청해야 한다.
    if !adapter_dir.is_absolute() {
        return false;
    }
    // D-b (#33): 대상 정규화 실패는 거부한다. 없는 대상은 지울 것이 없고, 접근 복구 후 재판정한다.
    // 보호 슬롯은 학습 출력 생성 전일 수 있어 canonicalize_or_self 폴백을 유지한다.
    let Ok(adapter_dir) = adapter_dir.canonicalize() else {
        return false;
    };
    let in_progress_adapter_dir =
        in_progress_adapter_name.map(|name| adapter_output_dir(home, name));
    !is_adapter_protected(
        &adapter_dir,
        home,
        serving_adapter_path,
        last_known_good_adapter_path,
        in_progress_adapter_dir.as_deref(),
    )
}

/// 보호 슬롯의 별칭을 정규화한다. 아직 생성되지 않은 출력 경로는 원본으로 비교한다.
fn canonicalize_or_self(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// 이미 canonicalize한 `target`이 삭제로부터 보호돼야 하는가(이슈 #33 GC 가드) — 셋 중
/// 하나라도 canonical 비교로 일치하면 보호 대상이다: 현재 서빙 중인 adapter, 마지막으로
/// 헬스체크를 통과해 last-known-good으로 기록된 adapter, 아직 "done"에 이르지 못해
/// `TrainingStatus.adapter_path`가 비어 있는 **진행 중인 학습**의 출력 디렉터리.
///
fn is_adapter_protected(
    target: &Path,
    home: &Path,
    serving_adapter_path: Option<&str>,
    last_known_good_adapter_path: Option<&str>,
    in_progress_adapter_dir: Option<&Path>,
) -> bool {
    let normalize = |p: &Path| canonicalize_or_self(&expand_home_path(p, home));
    let matches_str = |p: &str| normalize(Path::new(p)) == target;

    if serving_adapter_path.map(matches_str).unwrap_or(false) {
        return true;
    }
    if last_known_good_adapter_path
        .map(matches_str)
        .unwrap_or(false)
    {
        return true;
    }
    if let Some(dir) = in_progress_adapter_dir {
        if normalize(dir) == target {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests;
