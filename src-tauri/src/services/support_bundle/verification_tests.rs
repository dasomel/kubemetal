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
