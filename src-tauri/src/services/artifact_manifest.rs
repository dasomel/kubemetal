use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MANIFEST_FILE: &str = "manifest.json";
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct ManifestContext {
    pub runtime: String,
    pub base_model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ArtifactManifest {
    schema_version: u32,
    created_at: String,
    runtime: String,
    base_model: String,
    files: Vec<ManifestEntry>,
}

// Verification is intentionally a service API rather than an IPC command in this single-user slice.
#[allow(dead_code)]
#[derive(Debug, Default, PartialEq, Eq)]
pub struct VerifyReport {
    pub missing: Vec<String>,
    pub changed: Vec<String>,
    pub extra: Vec<String>,
}

#[allow(dead_code)]
impl VerifyReport {
    pub fn is_valid(&self) -> bool {
        self.missing.is_empty() && self.changed.is_empty() && self.extra.is_empty()
    }
}

pub fn write_manifest(dir: &Path, extra: ManifestContext) -> Result<PathBuf, String> {
    let files = collect_files(dir)?;
    let manifest = ArtifactManifest {
        schema_version: SCHEMA_VERSION,
        created_at: utc_rfc3339_now()?,
        runtime: extra.runtime,
        base_model: extra.base_model,
        files,
    };
    let path = dir.join(MANIFEST_FILE);
    let content = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| format!("Failed to serialize artifact manifest: {e}"))?;
    fs::write(&path, content)
        .map_err(|e| format!("Failed to write artifact manifest {}: {e}", path.display()))?;
    Ok(path)
}

#[allow(dead_code)]
pub fn verify_manifest(dir: &Path) -> Result<VerifyReport, String> {
    let manifest_path = dir.join(MANIFEST_FILE);
    let content = fs::read(&manifest_path).map_err(|e| {
        format!(
            "Artifact manifest not found at {}: {e}",
            manifest_path.display()
        )
    })?;
    let manifest: ArtifactManifest = serde_json::from_slice(&content).map_err(|e| {
        format!(
            "Failed to parse artifact manifest {}: {e}",
            manifest_path.display()
        )
    })?;
    if manifest.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "Unsupported artifact manifest schema version: {}",
            manifest.schema_version
        ));
    }

    let actual = collect_files(dir)?;
    let mut expected = manifest.files;
    expected.sort_by(|a, b| a.path.cmp(&b.path));
    validate_manifest_paths(&expected)?;

    let mut report = VerifyReport::default();
    let mut expected_index = 0;
    let mut actual_index = 0;
    while expected_index < expected.len() || actual_index < actual.len() {
        match (expected.get(expected_index), actual.get(actual_index)) {
            (Some(expected), Some(actual)) if expected.path == actual.path => {
                if expected.sha256 != actual.sha256 || expected.bytes != actual.bytes {
                    report.changed.push(expected.path.clone());
                }
                expected_index += 1;
                actual_index += 1;
            }
            (Some(expected), Some(actual)) if expected.path < actual.path => {
                report.missing.push(expected.path.clone());
                expected_index += 1;
            }
            (Some(_), Some(actual)) => {
                report.extra.push(actual.path.clone());
                actual_index += 1;
            }
            (Some(expected), None) => {
                report.missing.push(expected.path.clone());
                expected_index += 1;
            }
            (None, Some(actual)) => {
                report.extra.push(actual.path.clone());
                actual_index += 1;
            }
            (None, None) => break,
        }
    }
    Ok(report)
}

fn collect_files(dir: &Path) -> Result<Vec<ManifestEntry>, String> {
    let entries = fs::read_dir(dir)
        .map_err(|e| format!("Failed to read artifact directory {}: {e}", dir.display()))?;
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read artifact directory entry: {e}"))?;
        let file_type = entry.file_type().map_err(|e| {
            format!(
                "Failed to inspect artifact entry {}: {e}",
                entry.path().display()
            )
        })?;
        if !file_type.is_file() || file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| format!("Artifact filename is not valid UTF-8: {}", path.display()))?;
        if name == MANIFEST_FILE {
            continue;
        }
        let bytes = fs::metadata(&path)
            .map_err(|e| format!("Failed to read artifact metadata {}: {e}", path.display()))?
            .len();
        files.push(ManifestEntry {
            path: name,
            sha256: sha256_file(&path)?,
            bytes,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let file =
        File::open(path).map_err(|e| format!("Failed to open artifact {}: {e}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|e| format!("Failed to hash artifact {}: {e}", path.display()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[allow(dead_code)]
fn validate_manifest_paths(entries: &[ManifestEntry]) -> Result<(), String> {
    let mut previous: Option<&str> = None;
    for entry in entries {
        if entry.path == MANIFEST_FILE
            || Path::new(&entry.path).components().count() != 1
            || !matches!(
                Path::new(&entry.path).components().next(),
                Some(Component::Normal(_))
            )
        {
            return Err(format!("Invalid artifact manifest path: {}", entry.path));
        }
        if previous == Some(entry.path.as_str()) {
            return Err(format!("Duplicate artifact manifest path: {}", entry.path));
        }
        previous = Some(&entry.path);
    }
    Ok(())
}

fn utc_rfc3339_now() -> Result<String, String> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("System clock is before Unix epoch: {e}"))?
        .as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds_of_day / 3600,
        (seconds_of_day % 3600) / 60,
        seconds_of_day % 60
    ))
}

// Gregorian civil date from days since 1970-01-01, adapted from Howard Hinnant's public-domain algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    (year + i64::from(month <= 2), month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{verify_manifest, write_manifest, ManifestContext};

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("kubemetal-{name}-{nonce}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn context() -> ManifestContext {
        ManifestContext {
            runtime: "mlx-lm".into(),
            base_model: "mlx-community/test-model".into(),
        }
    }

    #[test]
    fn writes_sorted_manifest_with_sha256_and_excludes_itself() {
        let dir = temp_dir("manifest-write");
        fs::write(dir.join("z.txt"), b"z").unwrap();
        fs::write(dir.join("a.txt"), b"hello").unwrap();

        write_manifest(&dir, context()).unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("manifest.json")).unwrap()).unwrap();
        let files = manifest["files"].as_array().unwrap();
        assert_eq!(manifest["schema_version"], 1);
        assert_eq!(manifest["runtime"], "mlx-lm");
        assert_eq!(files[0]["path"], "a.txt");
        assert_eq!(
            files[0]["sha256"],
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(files[0]["bytes"], 5);
        assert_eq!(files[1]["path"], "z.txt");
        assert!(files.iter().all(|entry| entry["path"] != "manifest.json"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn ignores_symlinks_and_subdirectories() {
        use std::os::unix::fs::symlink;

        let dir = temp_dir("manifest-ignore");
        fs::write(dir.join("kept.bin"), b"kept").unwrap();
        fs::create_dir(dir.join("nested")).unwrap();
        fs::write(dir.join("nested/inside.bin"), b"nested").unwrap();
        symlink(dir.join("kept.bin"), dir.join("linked.bin")).unwrap();

        write_manifest(&dir, context()).unwrap();
        let report = verify_manifest(&dir).unwrap();
        assert!(report.is_valid());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn verify_reports_changed_missing_and_extra_files() {
        let dir = temp_dir("manifest-verify");
        fs::write(dir.join("changed.bin"), b"original").unwrap();
        fs::write(dir.join("missing.bin"), b"missing").unwrap();
        write_manifest(&dir, context()).unwrap();
        fs::write(dir.join("changed.bin"), b"changed").unwrap();
        fs::remove_file(dir.join("missing.bin")).unwrap();
        fs::write(dir.join("extra.bin"), b"extra").unwrap();

        let report = verify_manifest(&dir).unwrap();
        assert_eq!(report.changed, ["changed.bin"]);
        assert_eq!(report.missing, ["missing.bin"]);
        assert_eq!(report.extra, ["extra.bin"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn verify_without_manifest_returns_explicit_error() {
        let dir = temp_dir("manifest-missing");
        let error = verify_manifest(Path::new(&dir)).unwrap_err();
        assert!(error.contains("Artifact manifest not found"));
        fs::remove_dir_all(dir).unwrap();
    }
}
