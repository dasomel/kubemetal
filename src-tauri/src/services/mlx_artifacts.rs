//! MLX 어댑터 산출물(학습 결과 디렉터리)에 대한 순수 판정 로직 — 이슈 #33 축소
//! 스코프(체크포인트 상태 판정 + GC 삭제 가드). `commands/mlx.rs`가 `MlxState`의
//! Mutex를 잠그고 그 안의 값을 뽑아 이 모듈의 순수 함수에 넘기는 얇은 호출부를
//! 맡고, 여기서는 `MlxState`/`tauri` 의존 없이 값만으로 판정한다(2026-09-23 리뷰
//! LOW 파인딩 — mlx.rs가 1128→1612줄로 불어난 것 중 일부를 여기로 옮긴다).
//!
//! 이동하면서 동작은 바꾸지 않았다 — `commands::mlx::tests`의 기존
//! `is_adapter_safe_to_delete_*`/`manifest_verification_status_*` 통합 테스트가
//! `MlxState`를 통해 그대로 남아 있고, 이 파일의 테스트는 그 아래 순수 로직만
//! 추가로 좁혀서 검증한다.

use std::path::{Path, PathBuf};

use crate::services::artifact_manifest::verify_manifest;

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

/// 심볼릭 링크·`.`/`..` 컴포넌트가 섞인 별칭 경로가 canonical 비교를 피해가지 못하게
/// 정규화한다. canonicalize가 실패하면(경로가 아직 없는 경우 등) 원본을 그대로 쓴다 —
/// 존재하지 않는 경로를 있는 그대로 보고하는 건 괜찮지만, 존재하는 서빙 중 어댑터의
/// 별칭이 정규화를 피해 통과해서는 안 된다(2026-09-23 리뷰).
fn canonicalize_or_self(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// `adapter_dir`이 삭제로부터 보호돼야 하는가(이슈 #33 GC 가드의 순수 판정부) — 셋 중
/// 하나라도 canonical 비교로 일치하면 보호 대상이다: 현재 서빙 중인 adapter, 마지막으로
/// 헬스체크를 통과해 last-known-good으로 기록된 adapter, 아직 "done"에 이르지 못해
/// `TrainingStatus.adapter_path`가 비어 있는 **진행 중인 학습**의 출력 디렉터리.
///
/// **호출부(mlx.rs)가 반드시 지켜야 할 계약**: 세 슬롯 중 어느 것을 뒷받침하는
/// Mutex가 poison됐다면(다른 스레드가 그 락을 쥔 채 panic) 그 슬롯 값을 `None`으로
/// 여기 넘기지 말고 **호출 자체를 생략하고 즉시 false(=삭제 불가)를 반환**해야 한다
/// — `None`은 "그 슬롯은 비어 있다"는 뜻이지 "모른다"는 뜻이 아니라서, poison을
/// `None`으로 뭉개 넘기면 이 함수가 "보호 대상 아님"으로 오판한다(D22, fail-closed는
/// 이 함수가 아니라 그 계약을 지키는 호출부의 책임이다).
pub(crate) fn is_adapter_protected(
    adapter_dir: &Path,
    serving_adapter_path: Option<&str>,
    last_known_good_adapter_path: Option<&str>,
    in_progress_adapter_dir: Option<&Path>,
) -> bool {
    let target = canonicalize_or_self(adapter_dir);
    let matches_str = |p: &str| canonicalize_or_self(Path::new(p)) == target;

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
        if canonicalize_or_self(dir) == target {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::artifact_manifest::ManifestContext;

    fn make_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kubemetal-mlx-artifacts-test-{name}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("failed to create temp dir");
        dir
    }

    fn manifest_context() -> ManifestContext {
        ManifestContext {
            runtime: "mlx-lm".into(),
            base_model: "/base".into(),
        }
    }

    #[test]
    fn manifest_verification_status_returns_missing_without_manifest() {
        let dir = make_temp_dir("manifest-missing");
        assert_eq!(manifest_verification_status(&dir), "missing");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn manifest_verification_status_returns_verified_when_hashes_match() {
        let dir = make_temp_dir("manifest-verified");
        std::fs::write(dir.join("adapters.safetensors"), b"weights").unwrap();
        crate::services::artifact_manifest::write_manifest(&dir, manifest_context())
            .expect("manifest write should succeed");
        assert_eq!(manifest_verification_status(&dir), "verified");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn manifest_verification_status_returns_corrupt_when_hash_mismatches() {
        let dir = make_temp_dir("manifest-corrupt");
        std::fs::write(dir.join("adapters.safetensors"), b"weights").unwrap();
        crate::services::artifact_manifest::write_manifest(&dir, manifest_context())
            .expect("manifest write should succeed");
        std::fs::write(dir.join("adapters.safetensors"), b"tampered").unwrap();
        assert_eq!(manifest_verification_status(&dir), "corrupt");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn adapter_output_dir_joins_home_dotkubemetal_adapters_and_name() {
        let home = Path::new("/Users/example");
        assert_eq!(
            adapter_output_dir(home, "my-adapter"),
            Path::new("/Users/example/.kubemetal/adapters/my-adapter")
        );
    }

    #[test]
    fn is_adapter_protected_matches_serving_path_exactly() {
        let dir = make_temp_dir("protected-serving");
        let path_str = dir.to_string_lossy().to_string();
        assert!(is_adapter_protected(&dir, Some(&path_str), None, None));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn is_adapter_protected_matches_last_known_good_path() {
        let dir = make_temp_dir("protected-lkg");
        let path_str = dir.to_string_lossy().to_string();
        assert!(is_adapter_protected(&dir, None, Some(&path_str), None));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn is_adapter_protected_matches_non_canonical_alias_of_serving_path() {
        // `<dir>/.`은 canonicalize하면 dir 자신과 완전히 같은 경로가 된다 — 문자열
        // 비교였다면 놓쳤을 별칭이다(2026-09-23 리뷰).
        let dir = make_temp_dir("protected-alias");
        let alias = dir.join(".").to_string_lossy().to_string();
        assert!(is_adapter_protected(&dir, Some(&alias), None, None));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn is_adapter_protected_matches_in_progress_training_dir() {
        let dir = make_temp_dir("protected-in-progress");
        assert!(is_adapter_protected(&dir, None, None, Some(&dir)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn is_adapter_protected_allows_unrelated_path() {
        let dir = make_temp_dir("unprotected");
        let other = make_temp_dir("unprotected-other");
        let other_str = other.to_string_lossy().to_string();
        assert!(!is_adapter_protected(
            &dir,
            Some(&other_str),
            Some(&other_str),
            Some(&other)
        ));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&other).ok();
    }
}
