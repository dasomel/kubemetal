use super::*;
use crate::commands::modelhub::list_local_models_at;
use crate::services::process::external_command;
use std::os::unix::fs::symlink;

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "kubemetal-model-download-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_ATTEMPT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn models(&self) -> PathBuf {
        self.0.join("models")
    }
    fn catalog(&self) -> Vec<String> {
        list_local_models_at(&self.models())
            .unwrap()
            .into_iter()
            .map(|m| m.repo_id)
            .collect()
    }
    fn attempts(&self) -> Vec<PathBuf> {
        fs::read_dir(self.0.join("model-downloads"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[tokio::test]
async fn second_file_failure_is_invisible_and_retry_promotes() {
    let root = Root::new();
    let catalog = &root;
    let orphan = Attempt::begin(&root.models(), "other__model").unwrap();
    fs::write(orphan.staging.join("old"), b"unrelated").unwrap();
    let error = download(root.models(), "owner__model".into(), |dir| async move {
        fs::write(dir.join("config.json"), b"{}").unwrap();
        assert!(catalog.catalog().is_empty(), "partial first file leaked");
        fs::write(dir.join("weights"), b"partial").unwrap();
        Err("second file transfer failed".into())
    })
    .await
    .unwrap_err();
    assert!(error.contains("second file transfer failed"));
    assert!(root.catalog().is_empty());
    assert_eq!(root.attempts(), vec![orphan.staging.clone()]);
    download(root.models(), "owner__model".into(), |dir| async move {
        fs::write(dir.join("config.json"), b"{}").unwrap();
        fs::create_dir(dir.join("nested")).unwrap();
        fs::write(dir.join("nested/weights"), b"complete").unwrap();
        assert!(catalog.catalog().is_empty());
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(root.catalog(), vec!["owner/model"]);
    assert_eq!(
        fs::read(root.models().join("owner__model/nested/weights")).unwrap(),
        b"complete"
    );
    assert_eq!(fs::read(orphan.staging.join("old")).unwrap(), b"unrelated");
}

#[test]
fn exdev_fails_without_copying_and_cleans_attempt() {
    let root = Root::new();
    let attempt = Attempt::begin(&root.models(), "owner__model").unwrap();
    fs::write(attempt.staging.join("weights"), b"complete").unwrap();
    let error = attempt
        .finish(
            Ok(()),
            |_, _| Err(io::Error::from_raw_os_error(libc::EXDEV)),
            |p| fs::remove_dir_all(p),
        )
        .unwrap_err();
    assert!(error.contains(&io::Error::from_raw_os_error(libc::EXDEV).to_string()));
    assert!(root.catalog().is_empty());
    assert!(root.attempts().is_empty());
}

#[tokio::test]
async fn existing_directory_file_and_symlinks_refuse_before_transfer() {
    for kind in ["directory", "file", "symlink", "dangling"] {
        let root = Root::new();
        fs::create_dir(root.models()).unwrap();
        let destination = root.models().join("owner__model");
        let outside = root.0.join("external");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"preserve").unwrap();
        match kind {
            "directory" => {
                fs::create_dir(&destination).unwrap();
            }
            "file" => fs::write(&destination, b"existing").unwrap(),
            "symlink" => symlink(&outside, &destination).unwrap(),
            _ => symlink(root.0.join("absent"), &destination).unwrap(),
        }
        let error = download(root.models(), "owner__model".into(), |_| async {
            panic!("existing destination must refuse before transfer")
        })
        .await
        .unwrap_err();
        assert!(error.contains("already exists"));
        assert!(fs::symlink_metadata(&destination).is_ok());
        assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"preserve");
        if kind == "file" {
            assert_eq!(fs::read(&destination).unwrap(), b"existing");
        }
        if kind == "directory" {
            assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
        }
    }
}

#[test]
fn destination_created_during_transfer_is_preserved() {
    for symlink_target in [false, true] {
        let root = Root::new();
        let attempt = Attempt::begin(&root.models(), "owner__model").unwrap();
        fs::write(attempt.staging.join("weights"), b"new").unwrap();
        let destination = attempt.destination.clone();
        if symlink_target {
            symlink(root.0.join("missing-external"), &destination).unwrap();
        } else {
            // Plain rename would replace this empty directory.
            fs::create_dir(&destination).unwrap();
        }
        let error = attempt
            .finish(Ok(()), rename_exclusive, |p| fs::remove_dir_all(p))
            .unwrap_err();
        assert!(error.contains("promotion failed"));
        assert!(fs::symlink_metadata(&destination).is_ok());
        assert!(root.attempts().is_empty());
        if symlink_target {
            assert_eq!(
                fs::read_link(destination).unwrap(),
                root.0.join("missing-external")
            );
        } else {
            assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
        }
    }
}

#[test]
fn cleanup_failure_keeps_original_transfer_or_promotion_error() {
    for transfer in [Err("transfer interrupted".into()), Ok(())] {
        let root = Root::new();
        let attempt = Attempt::begin(&root.models(), "owner__model").unwrap();
        let original = if transfer.is_err() {
            "transfer interrupted"
        } else {
            "promotion failed"
        };
        let error = attempt
            .finish(
                transfer,
                |_, _| Err(io::Error::from_raw_os_error(libc::EXDEV)),
                |_| {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "cleanup fixture",
                    ))
                },
            )
            .unwrap_err();
        assert!(error.contains(original));
        assert!(error.contains("staging cleanup failed"));
        assert!(error.contains("cleanup fixture"));
        assert_eq!(root.attempts().len(), 1);
        assert!(root.catalog().is_empty());
    }
}

#[test]
fn staging_root_file_or_external_symlink_is_refused() {
    for link in [false, true] {
        let root = Root::new();
        let staging = root.0.join("model-downloads");
        if link {
            symlink(&root.0, &staging).unwrap();
        } else {
            fs::write(staging, b"preserve").unwrap();
        }
        assert!(Attempt::begin(&root.models(), "owner__model")
            .err()
            .unwrap()
            .contains("real directory"));
        assert!(root.catalog().is_empty());
    }
}

#[tokio::test]
async fn aborted_task_leaves_only_private_orphan_then_local_curl_retry_promotes() {
    let root = Root::new();
    let fixture = root.0.join("fixture");
    fs::write(&fixture, b"local transfer fixture").unwrap();
    let (ready, waiting) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(download(
        root.models(),
        "owner__model".into(),
        |dir| async move {
            tokio::fs::write(dir.join("weights"), b"partial")
                .await
                .unwrap();
            ready.send(()).unwrap();
            std::future::pending::<Result<(), String>>().await
        },
    ));
    waiting.await.unwrap();
    assert!(root.catalog().is_empty());
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(root.catalog().is_empty());
    assert_eq!(root.attempts().len(), 1);
    download(root.models(), "owner__model".into(), |dir| async move {
        let output = external_command("curl")?
            .kill_on_drop(true)
            .args(["-sfL", "-o"])
            .arg(dir.join("weights"))
            .arg(format!("file://{}", fixture.display()))
            .output()
            .await
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into());
        }
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(root.catalog(), vec!["owner/model"]);
    assert_eq!(
        fs::read(root.models().join("owner__model/weights")).unwrap(),
        b"local transfer fixture"
    );
    assert_eq!(root.attempts().len(), 1);
    println!("macOS local fixture: interrupted -> catalog empty -> curl file retry promoted; orphan remains outside catalog");
}
