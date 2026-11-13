use super::directory::open_root;
use super::*;
use crate::commands::modelhub::list_local_models_at;
use crate::services::model_download::{download, Attempt};
use std::collections::HashSet;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "inventory-{}-{}",
            std::process::id(),
            super::super::NEXT_ATTEMPT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn models(&self) -> PathBuf {
        self.0.join("models")
    }
    fn root(&self) -> PathBuf {
        super::super::staging_root(&self.models())
    }
    fn scan(&self) -> StagingInventory {
        scan(&self.models(), &HashMap::new()).unwrap()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn missing_and_empty_root_are_empty_without_writes() {
    let root = Root::new();
    assert!(root.scan().entries.is_empty());
    assert!(!root.root().exists());
    fs::create_dir(root.root()).unwrap();
    let report = root.scan();
    assert!(report.entries.is_empty() && report.errors.is_empty() && !report.truncated);
    assert!(report.observed_at_unix_ms > 0);
}

#[tokio::test]
async fn local_transfer_abort_then_restart_is_unknown_and_never_catalogued() {
    let root = Root::new();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(download(
        root.models(),
        "fixture__model".into(),
        |dir| async move {
            tokio::fs::write(dir.join("weights"), b"partial local fixture")
                .await
                .unwrap();
            tx.send(()).unwrap();
            std::future::pending::<Result<(), String>>().await
        },
    ));
    rx.await.unwrap();
    let report = list(root.models()).await.unwrap();
    assert_eq!(report.entries[0].state, "active");
    assert_eq!(report.entries[0].reason, "registered_attempt");
    assert_eq!(report.entries[0].size_bytes, Some(21));
    println!("A1_ACTIVE={}", serde_json::to_string(&report).unwrap());
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let report = list(root.models()).await.unwrap();
    assert_eq!(report.entries[0].state, "unknown");
    println!("A1_UNKNOWN={}", serde_json::to_string(&report).unwrap());
    // Empty registration models a fresh process. PID and timestamp in ID still
    // identify this living process: neither is durable ownership evidence.
    let restarted = root.scan();
    assert!(restarted.entries[0]
        .id
        .starts_with(&format!("{}-", std::process::id())));
    assert_eq!(restarted.entries[0].state, "unknown");
    assert_eq!(restarted.entries[0].reason, "no_durable_liveness_evidence");
    assert!(list_local_models_at(&root.models()).unwrap().is_empty());
    let attempt = fs::read_dir(root.root())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    symlink(root.0.join("missing"), attempt.join("link")).unwrap();
    println!(
        "A1_PARTIAL={}",
        serde_json::to_string(&root.scan()).unwrap()
    );
    fs::remove_file(attempt.join("link")).unwrap();
    fs::set_permissions(root.root(), fs::Permissions::from_mode(0o0)).unwrap();
    let unreadable = root.scan();
    fs::set_permissions(root.root(), fs::Permissions::from_mode(0o700)).unwrap();
    println!("A1_ERROR={}", serde_json::to_string(&unreadable).unwrap());
}

#[test]
fn symlink_root_and_file_root_are_errors() {
    for link in [true, false] {
        let root = Root::new();
        if link {
            symlink(&root.0, root.root()).unwrap();
        } else {
            fs::write(root.root(), b"not a directory").unwrap();
        }
        let report = root.scan();
        assert!(!report.errors.is_empty());
        assert!(report.entries.is_empty());
    }
}

#[test]
fn nested_symlink_and_permission_failure_are_partial_unknown_sizes() {
    let root = Root::new();
    let attempt = Attempt::begin(&root.models(), "fixture__model").unwrap();
    fs::write(root.0.join("outside"), b"never count me").unwrap();
    symlink(
        root.0.join("outside"),
        attempt.staging.join("limit reached link"),
    )
    .unwrap();
    let denied = root.root().join("denied");
    fs::create_dir(&denied).unwrap();
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o0)).unwrap();
    let report = root.scan();
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(report.entries.len(), 2);
    assert!(
        !report.truncated,
        "filenames must not fabricate limit evidence"
    );
    for entry in report.entries {
        assert_eq!(entry.size_bytes, None);
        assert!(!entry.errors.is_empty());
    }
    fs::set_permissions(root.root(), fs::Permissions::from_mode(0o0)).unwrap();
    let report = root.scan();
    fs::set_permissions(root.root(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(!report.errors.is_empty());
}

#[test]
fn disappearing_child_and_path_replacement_cannot_escape_open_directory() {
    let root = Root::new();
    let attempt = Attempt::begin(&root.models(), "fixture__model").unwrap();
    fs::write(attempt.staging.join("gone"), b"x").unwrap();
    let dir = Directory::new(open_root(&attempt.staging).unwrap()).unwrap();
    let name = dir.next().unwrap().unwrap();
    fs::remove_file(attempt.staging.join(&name)).unwrap();
    assert_eq!(dir.open(&name).unwrap_err().kind(), io::ErrorKind::NotFound);
    let moved = root.0.join("moved");
    fs::rename(&attempt.staging, &moved).unwrap();
    symlink(&root.0, &attempt.staging).unwrap();
    assert!(dir.open(OsStr::new("outside")).is_err());
    assert_eq!(root.scan().entries[0].size_bytes, None);
}

#[test]
fn attempts_entries_and_depth_are_bounded() {
    let root = Root::new();
    let attempt = Attempt::begin(&root.models(), "fixture__model").unwrap();
    fs::write(attempt.staging.join("file"), b"abc").unwrap();
    let file = open_root(&attempt.staging).unwrap();
    assert!(measure(file, &mut 0, 0, &mut false)
        .unwrap_err()
        .contains("entry limit"));
    let file = open_root(&attempt.staging).unwrap();
    assert!(measure(file, &mut 10, MAX_DEPTH, &mut false)
        .unwrap_err()
        .contains("depth limit"));
    for n in 0..MAX_ATTEMPTS {
        fs::create_dir(root.root().join(format!("attempt-{n}"))).unwrap();
    }
    let report = root.scan();
    assert_eq!(report.entries.len(), MAX_ATTEMPTS);
    assert!(report.truncated && !report.errors.is_empty());
}

#[test]
fn large_payload_directory_has_null_size_and_limit_evidence() {
    let root = Root::new();
    let attempt = Attempt::begin(&root.models(), "fixture__model").unwrap();
    for n in 0..MAX_ENTRIES {
        fs::write(attempt.staging.join(n.to_string()), b"x").unwrap();
    }
    let report = root.scan();
    assert!(report.truncated);
    assert_eq!(report.entries[0].size_bytes, None);
    assert!(report.entries[0].errors[0].contains("limit"));
}

#[test]
fn non_utf8_names_have_distinct_ids_and_special_files_have_unknown_size() {
    let root = Root::new();
    fs::create_dir(root.root()).unwrap();
    let ids: HashSet<_> = [vec![0xff], vec![0xfe], b"%FF".to_vec()]
        .into_iter()
        .map(|name| entry_id(&OsString::from_vec(name)))
        .collect();
    assert_eq!(ids.len(), 3);
    assert!(ids.contains("%FF") && ids.contains("%25FF"));
    let error = measure(File::open("/dev/null").unwrap(), &mut 1, 0, &mut false).unwrap_err();
    assert!(error.contains("Unsupported"));
}

#[tokio::test]
async fn replacing_a_registered_path_is_not_an_active_attempt() {
    let root = Root::new();
    let attempt = Attempt::begin(&root.models(), "fixture__model").unwrap();
    fs::rename(&attempt.staging, root.0.join("moved-active")).unwrap();
    fs::create_dir(&attempt.staging).unwrap();
    let report = list(root.models()).await.unwrap();
    assert_eq!(report.entries[0].state, "unknown");
    assert_eq!(report.entries[0].size_bytes, Some(0));
}

mod review;
