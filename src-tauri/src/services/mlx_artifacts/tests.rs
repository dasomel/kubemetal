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
fn deletion_fails_closed_when_home_is_unavailable() {
    let adapter = Path::new("/unresolved-home/.kubemetal/adapters/training");
    assert!(!is_adapter_safe_to_delete(
        adapter,
        None,
        None,
        None,
        Some("training")
    ));
    assert!(!is_adapter_safe_to_delete(adapter, None, None, None, None));
}

#[test]
fn is_adapter_protected_matches_serving_path_exactly() {
    let dir = make_temp_dir("protected-serving");
    let path_str = dir.to_string_lossy().to_string();
    assert!(is_adapter_protected(
        &dir,
        Path::new("/"),
        Some(&path_str),
        None,
        None
    ));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn is_adapter_protected_matches_last_known_good_path() {
    let dir = make_temp_dir("protected-lkg");
    let path_str = dir.to_string_lossy().to_string();
    assert!(is_adapter_protected(
        &dir,
        Path::new("/"),
        None,
        Some(&path_str),
        None
    ));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn deletion_forbids_symlink_to_serving_and_last_known_good_adapter() {
    let home = make_temp_dir("symlink-serving");
    let dir = home.join("adapter");
    let alias = home.join("alias");
    std::fs::create_dir(&dir).unwrap();
    std::os::unix::fs::symlink(&dir, &alias).unwrap();
    let real_path = dir.to_str().unwrap();
    let alias_path = alias.to_str().unwrap();
    let results = [
        is_adapter_safe_to_delete(&alias, Some(&home), Some(real_path), None, None),
        is_adapter_safe_to_delete(&dir, Some(&home), Some(alias_path), None, None),
        is_adapter_safe_to_delete(&alias, Some(&home), None, Some(real_path), None),
        is_adapter_safe_to_delete(&dir, Some(&home), None, Some(alias_path), None),
    ];
    std::fs::remove_dir_all(&home).unwrap();
    assert_eq!(results, [false; 4]);
}

#[test]
fn deletion_forbids_symlink_to_in_progress_training_adapter() {
    let home = make_temp_dir("symlink-training");
    let dir = adapter_output_dir(&home, "training");
    let alias = home.join("alias");
    std::fs::create_dir_all(&dir).unwrap();
    std::os::unix::fs::symlink(&dir, &alias).unwrap();
    let safe = is_adapter_safe_to_delete(&alias, Some(&home), None, None, Some("training"));
    std::fs::remove_dir_all(&home).unwrap();
    assert!(!safe);
}

#[test]
fn is_adapter_protected_matches_in_progress_training_dir() {
    let dir = make_temp_dir("protected-in-progress");
    assert!(is_adapter_protected(
        &dir,
        Path::new("/"),
        None,
        None,
        Some(&dir)
    ));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn is_adapter_protected_allows_unrelated_path() {
    let dir = make_temp_dir("unprotected");
    let other = make_temp_dir("unprotected-other");
    let other_str = other.to_string_lossy().to_string();
    assert!(!is_adapter_protected(
        &dir,
        Path::new("/"),
        Some(&other_str),
        Some(&other_str),
        Some(&other)
    ));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&other).ok();
}

#[test]
fn deletion_forbids_tilde_paths_for_serving_and_last_known_good() {
    let home = make_temp_dir("tilde-serving");
    let dir = home.join("adapter");
    let alias = home.join("alias");
    std::fs::create_dir(&dir).unwrap();
    std::os::unix::fs::symlink(&dir, &alias).unwrap();
    let real_path = dir.to_str().unwrap();
    let results = [
        is_adapter_safe_to_delete(
            Path::new("~/adapter"),
            Some(&home),
            Some(real_path),
            None,
            None,
        ),
        is_adapter_safe_to_delete(&dir, Some(&home), Some("~/adapter"), None, None),
        is_adapter_safe_to_delete(
            Path::new("~/adapter"),
            Some(&home),
            None,
            Some(real_path),
            None,
        ),
        is_adapter_safe_to_delete(&dir, Some(&home), None, Some("~/adapter"), None),
        is_adapter_safe_to_delete(
            Path::new("~/alias"),
            Some(&home),
            Some(real_path),
            None,
            None,
        ),
        is_adapter_safe_to_delete(&dir, Some(&home), Some("~/alias"), None, None),
    ];
    std::fs::remove_dir_all(&home).unwrap();
    assert_eq!(results, [false; 6]);
}

#[test]
fn deletion_forbids_tilde_path_for_training_before_output_exists() {
    let home = make_temp_dir("tilde-training");
    let target = Path::new("~/.kubemetal/adapters/training");
    let before = is_adapter_safe_to_delete(target, Some(&home), None, None, Some("training"));
    std::fs::create_dir_all(adapter_output_dir(&home, "training")).unwrap();
    let after = is_adapter_safe_to_delete(target, Some(&home), None, None, Some("training"));
    let unrelated = is_adapter_safe_to_delete(
        Path::new("~/.kubemetal/adapters/other"),
        Some(&home),
        None,
        None,
        Some("training"),
    );
    std::fs::remove_dir_all(&home).unwrap();
    assert!(!before);
    assert!(!after);
    assert!(unrelated);
}

#[test]
fn deletion_fails_closed_for_tilde_paths_without_home() {
    let target = Path::new("~/adapter");
    assert!(!is_adapter_safe_to_delete(target, None, None, None, None));
    assert!(!is_adapter_safe_to_delete(
        Path::new("/adapter"),
        None,
        Some("~/adapter"),
        None,
        None,
    ));
}
