use super::*;
use std::os::unix::net::UnixListener;

struct TestDir(PathBuf);
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixture() -> (TestDir, PathBuf) {
    let temp = TestDir(PathBuf::from("/tmp").join(format!(
            "sv-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    fs::create_dir_all(&temp.0).unwrap();
    let result = generate_support_bundle(
        &temp.0,
        None,
        None,
        collect_app_os_version_info(None),
        vec![],
    )
    .unwrap();
    (temp, PathBuf::from(result.bundle_dir))
}

#[test]
fn verifier_rejects_unlisted_socket() {
    let (_temp, root) = fixture();
    let _socket = UnixListener::bind(root.join("unexpected.sock")).unwrap();
    let report = verify_support_bundle_dir(&root, None).unwrap();
    assert!(!report.verified, "special entries must fail closed");
}

fn fifo(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
}

#[test]
fn verifier_rejects_fifo_manifest_and_payload_and_extra_entry() {
    for name in [MANIFEST_FILE, "app_info.json", "unexpected.fifo"] {
        let (_temp, root) = fixture();
        let target = root.join(name);
        if target.exists() {
            fs::remove_file(&target).unwrap();
        }
        fifo(&target);
        let report = verify_support_bundle_dir(&root, None).unwrap();
        assert!(!report.verified, "FIFO {name} must fail closed");
    }
}

#[test]
fn verifier_read_enforces_actual_byte_limit_and_rejects_symlinks() {
    let (_temp, root) = fixture();
    let file = root.join("growing.txt");
    fs::write(&file, b"12345").unwrap();
    assert!(read_verification_file(&file, 4).is_err());
    assert_eq!(read_verification_file(&file, 5).unwrap(), b"12345");
    let link = root.join("link");
    std::os::unix::fs::symlink(&file, &link).unwrap();
    assert!(read_verification_file(&link, 5).is_err());
    let pipe = root.join("pipe");
    fifo(&pipe);
    assert!(read_verification_file(&pipe, 5).is_err());
}

#[test]
fn correlation_manifest_and_verification_contract() {
    let (temp, _) = fixture();
    let id = "aabbccddeeff00112233445566778899";
    let observed_at = "2026-11-16T01:02:03Z";
    let health = serde_json::json!({"correlation_id": id, "observed_at": observed_at,
        "token": "sentinel-secret", "path": "/Users/test/private"});
    let result = generate_support_bundle(
        &temp.0,
        Some(Path::new("/Users/test")),
        Some(health.to_string()),
        collect_app_os_version_info(None),
        vec![],
    )
    .unwrap();
    let root = Path::new(&result.bundle_dir);
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&result.manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["health_snapshot"]["correlation_id"], id);
    assert_eq!(manifest["health_snapshot"]["observed_at"], observed_at);
    let redacted = fs::read_to_string(root.join("health_summary.json")).unwrap();
    assert!(redacted.contains(id));
    assert!(!redacted.contains("sentinel-secret"));
    assert!(!redacted.contains("/Users/test"));
    let report = serde_json::to_value(verify_support_bundle_dir(root, None).unwrap()).unwrap();
    assert_eq!(report["health_link_status"], "linked");
    assert_eq!(report["health_snapshot"]["correlation_id"], id);
    for field in ["correlation_id", "observed_at"] {
        let original = manifest["health_snapshot"][field].clone();
        manifest["health_snapshot"][field] = serde_json::json!("different");
        fs::write(&result.manifest_path, manifest.to_string()).unwrap();
        let report = verify_support_bundle_dir(root, None).unwrap();
        assert!(!report.verified);
        assert!(report
            .problems
            .iter()
            .any(|p| p.category == "health-link-mismatch"));
        manifest["health_snapshot"][field] = original;
    }
    manifest.as_object_mut().unwrap().remove("health_snapshot");
    fs::write(&result.manifest_path, manifest.to_string()).unwrap();
    let report = verify_support_bundle_dir(root, None).unwrap();
    assert!(report.verified, "old manifests remain valid");
    assert_eq!(
        serde_json::to_value(report).unwrap()["health_link_status"],
        "not_linked"
    );
}

#[test]
fn correlation_claim_without_health_identity_is_invalid() {
    let (_temp, root) = fixture();
    let path = root.join(MANIFEST_FILE);
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["health_snapshot"] = serde_json::json!({
        "correlation_id": "aabbccddeeff00112233445566778899", "observed_at": "2026-11-16T01:02:03Z"
    });
    fs::write(path, manifest.to_string()).unwrap();
    let report = verify_support_bundle_dir(&root, None).unwrap();
    assert!(!report.verified);
    assert_eq!(report.health_link_status, HealthLinkStatus::InvalidLink);
    assert!(report.health_snapshot.is_none());
}

#[test]
fn correlation_broken_evidence_is_invalid_link() {
    for damage in [
        "missing",
        "hash",
        "size",
        "invalid-json",
        "unreadable",
        "unlisted",
    ] {
        let (temp, _) = fixture();
        let health = serde_json::json!({"correlation_id": "a".repeat(32), "observed_at": "2026-11-16T01:02:03Z"});
        let result = generate_support_bundle(
            &temp.0,
            None,
            Some(health.to_string()),
            collect_app_os_version_info(None),
            vec![],
        )
        .unwrap();
        let root = Path::new(&result.bundle_dir);
        let path = root.join("health_summary.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&result.manifest_path).unwrap()).unwrap();
        match damage {
            "missing" => fs::remove_file(&path).unwrap(),
            "unreadable" => {
                fs::remove_file(&path).unwrap();
                fs::create_dir(&path).unwrap();
            }
            "unlisted" => manifest["files"]
                .as_array_mut()
                .unwrap()
                .retain(|e| e["path"] != "health_summary.json"),
            "size" => {
                let entry = manifest["files"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|e| e["path"] == "health_summary.json")
                    .unwrap();
                entry["bytes"] = serde_json::json!(1);
            }
            "hash" => {
                let bytes = fs::read_to_string(&path)
                    .unwrap()
                    .replace(&"a".repeat(32), &"b".repeat(32));
                fs::write(&path, bytes).unwrap();
            }
            _ => {
                fs::write(&path, b"not json").unwrap();
                if damage == "invalid-json" {
                    let entry = manifest["files"]
                        .as_array_mut()
                        .unwrap()
                        .iter_mut()
                        .find(|e| e["path"] == "health_summary.json")
                        .unwrap();
                    entry["bytes"] = serde_json::json!(8);
                    entry["sha256"] = serde_json::json!(sha256_digest(b"not json"));
                }
            }
        }
        fs::write(&result.manifest_path, manifest.to_string()).unwrap();
        let report = verify_support_bundle_dir(root, None).unwrap();
        assert_eq!(
            report.health_link_status,
            HealthLinkStatus::InvalidLink,
            "{damage}"
        );
        assert!(!report.verified, "{damage}");
        if damage == "hash" {
            assert!(report
                .problems
                .iter()
                .any(|p| p.category == "hash-mismatch"));
        }
        assert!(report.health_snapshot.is_none(), "{damage}");
    }
}
