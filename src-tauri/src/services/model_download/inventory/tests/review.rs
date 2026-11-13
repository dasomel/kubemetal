use super::*;

#[test]
fn review_never_created_root_is_explicit() {
    let root = Root::new();
    let report = root.scan();
    assert_eq!(
        serde_json::to_value(&report).unwrap()["state"],
        "not_created"
    );
    assert!(report.entries.is_empty() && report.errors.is_empty());
    assert!(!root.root().exists());
}

#[test]
fn review_missing_parent_is_unknown_with_error() {
    let root = Root::new();
    let report = scan(&root.0.join("missing/models"), &HashMap::new()).unwrap();
    assert!(
        !report.errors.is_empty(),
        "missing parent is not empty staging"
    );
    assert_eq!(serde_json::to_value(&report).unwrap()["state"], "unknown");
}

#[test]
fn review_root_removed_during_scan_is_unknown_with_error() {
    let root = Root::new();
    fs::create_dir(root.root()).unwrap();
    let report = scan_with(
        &root.models(),
        &HashMap::new(),
        || fs::remove_dir(root.root()).unwrap(),
        |dir| dir.next(),
    )
    .unwrap();
    assert!(
        !report.errors.is_empty(),
        "observed root vanished before open"
    );
    assert_eq!(serde_json::to_value(&report).unwrap()["state"], "unknown");
}

#[test]
fn review_enumeration_failure_preserves_entries_and_marks_incomplete() {
    let root = Root::new();
    fs::create_dir(root.root()).unwrap();
    fs::write(root.root().join("first"), b"abc").unwrap();
    let mut count = 0;
    let report = scan_with(
        &root.models(),
        &HashMap::new(),
        || {},
        |dir| {
            count += 1;
            if count == 2 {
                Err(io::Error::other("injected enumeration failure"))
            } else {
                dir.next()
            }
        },
    )
    .unwrap();
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].size_bytes, Some(3));
    assert!(report.errors[0].contains("injected enumeration failure"));
    assert!(report.truncated, "enumeration stopped before EOF");
}

#[test]
fn review_attempt_cap_marks_incomplete() {
    let root = Root::new();
    fs::create_dir(root.root()).unwrap();
    for n in 0..=MAX_ATTEMPTS {
        fs::write(root.root().join(n.to_string()), b"x").unwrap();
    }
    let report = root.scan();
    assert_eq!(report.entries.len(), MAX_ATTEMPTS);
    assert!(report.truncated);
}
