//! #17 축소 스코프: 서포트 번들 생성 서비스
//!
//! 시스템 헬스 요약(`get_system_health_summary`), 앱/OS/버전 정보, 최근 로그(`~/.kubemetal/logs/`)를
//! 수집해 앱 데이터 디렉터리 하위 `support-bundles/support-bundle-<timestamp>`에 저장한다.
//! 민감정보(토큰, kubeconfig 키/인증서 데이터, Authorization 헤더, API 키, 사용자 홈 경로 `~` 치환)를
//! 자동 redaction하며, 바이너리/비-UTF-8 등 검사 불가능한 파일은 fail-closed로 제외하고
//! `manifest.json`에 `omitted: <reason>`으로 기록한다.

use std::fs::{self, File, OpenOptions};
#[cfg(test)]
use std::io::BufReader;
use std::io::{Read, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sysinfo::System;

pub const SCHEMA_VERSION: u32 = 1;
pub const MANIFEST_FILE: &str = "manifest.json";
pub const MAX_SCANNABLE_BYTES: usize = 10 * 1024 * 1024; // 10 MiB limit

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestFileEntry {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestOmittedEntry {
    pub path: String,
    pub omitted: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupportBundleManifest {
    pub schema_version: u32,
    pub created_at: String,
    pub files: Vec<ManifestFileEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omitted: Vec<ManifestOmittedEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupportBundleResult {
    pub bundle_dir: String,
    pub manifest_path: String,
    pub files_count: usize,
    pub omitted_count: usize,
    pub manifest: SupportBundleManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppOsVersionInfo {
    pub app_name: String,
    pub app_version: String,
    pub os: String,
    pub os_version: Option<String>,
    pub kernel_version: Option<String>,
    pub arch: String,
}

/// 파일 바이트가 텍스트 검사 가능한지 확인 (fail-closed)
pub fn is_scannable_text(bytes: &[u8]) -> Result<&str, String> {
    if bytes.len() > MAX_SCANNABLE_BYTES {
        return Err(format!(
            "File exceeds maximum scannable size of {} bytes",
            MAX_SCANNABLE_BYTES
        ));
    }
    if bytes.contains(&0) {
        return Err("Binary content containing null bytes cannot be safely scanned".to_string());
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|e| format!("Non-UTF-8 content cannot be safely scanned: {e}"))?;
    Ok(text)
}

/// SHA-256 해시 16진수 문자열 계산
pub fn sha256_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut hex, "{byte:02x}").expect("hex formatting");
    }
    hex
}

/// 파일 단위 SHA-256 해시 계산
#[cfg(test)]
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open {}: {e}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|e| format!("Failed to hash {}: {e}", path.display()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut hex, "{byte:02x}").expect("hex formatting");
    }
    Ok(hex)
}

mod redact;
pub use redact::redact_text;

fn write_bundle_file(path: &Path, bytes: &[u8]) -> Result<ManifestFileEntry, String> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("Failed to create {}: {e}", path.display()))?;
    file.write_all(bytes)
        .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|e| format!("Failed to seek {}: {e}", path.display()))?;
    let mut written = Vec::new();
    file.read_to_end(&mut written)
        .map_err(|e| format!("Failed to verify {}: {e}", path.display()))?;
    Ok(ManifestFileEntry {
        path: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        sha256: sha256_digest(&written),
        bytes: written.len() as u64,
    })
}

/// 앱 및 OS 환경 정보 수집
pub fn collect_app_os_version_info(app: Option<&tauri::AppHandle>) -> AppOsVersionInfo {
    let app_name = app
        .map(|a| a.package_info().name.to_string())
        .unwrap_or_else(|| "KubeMetal".to_string());
    let app_version = app
        .map(|a| a.package_info().version.to_string())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());

    AppOsVersionInfo {
        app_name,
        app_version,
        os: std::env::consts::OS.to_string(),
        os_version: System::os_version(),
        kernel_version: System::kernel_version(),
        arch: std::env::consts::ARCH.to_string(),
    }
}

/// 서포트 번들 생성 (동기/스레드 블로킹 구현)
#[cfg(test)]
pub fn generate_support_bundle(
    target_base_dir: &Path,
    home: Option<&Path>,
    health_summary_json: Option<String>,
    app_info: AppOsVersionInfo,
    log_sources: Vec<(String, PathBuf)>,
) -> Result<SupportBundleResult, String> {
    generate_support_bundle_with_omissions(
        target_base_dir,
        home,
        health_summary_json,
        app_info,
        log_sources,
        Vec::new(),
    )
}

fn generate_support_bundle_with_omissions(
    target_base_dir: &Path,
    home: Option<&Path>,
    health_summary_json: Option<String>,
    app_info: AppOsVersionInfo,
    log_sources: Vec<(String, PathBuf)>,
    mut omitted: Vec<ManifestOmittedEntry>,
) -> Result<SupportBundleResult, String> {
    let (created_at, dir_timestamp) = format_bundle_timestamps()?;
    // D1: Refuse a linked root outside app data. This aborts the bundle; callers can retry
    // after removing the link, rather than accepting a write outside the configured root.
    if let Ok(metadata) = fs::symlink_metadata(target_base_dir) {
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "Omitted support bundle: bundle root {} is a symlink",
                target_base_dir.display()
            ));
        }
    }
    fs::create_dir_all(target_base_dir).map_err(|e| {
        format!(
            "Failed to create bundle base {}: {e}",
            target_base_dir.display()
        )
    })?;
    let bundle_dir = (0..1000)
        .find_map(|suffix| {
            let name = if suffix == 0 {
                format!("support-bundle-{dir_timestamp}")
            } else {
                format!("support-bundle-{dir_timestamp}-{suffix}")
            };
            let path = target_base_dir.join(name);
            match fs::create_dir(&path) {
                Ok(()) => Some(Ok(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(format!(
                    "Failed to create bundle dir {}: {e}",
                    path.display()
                ))),
            }
        })
        .unwrap_or_else(|| Err("No unused support bundle directory name".to_string()))?;

    let mut files = Vec::new();

    // 1. app_info.json 작성
    let app_info_str = serde_json::to_string_pretty(&app_info)
        .map_err(|e| format!("Failed to serialize app info: {e}"))?;
    let app_info_redacted = redact_text(&app_info_str, home);
    let app_info_path = bundle_dir.join("app_info.json");
    files.push(write_bundle_file(
        &app_info_path,
        app_info_redacted.as_bytes(),
    )?);

    // 2. health_summary.json 작성 (실패 시에도 가짜 상태 대신 에러 기록)
    let health_str = health_summary_json.unwrap_or_else(|| {
        serde_json::json!({
            "status": "failed",
            "error": "Health summary unavailable"
        })
        .to_string()
    });
    let health_redacted = redact_text(&health_str, home);
    let health_path = bundle_dir.join("health_summary.json");
    files.push(write_bundle_file(&health_path, health_redacted.as_bytes())?);

    // 3. 로그 소스 수집 및 redaction (fail-closed 검증)
    for (dest_rel_path, src_path) in log_sources {
        if !src_path.exists() {
            omitted.push(ManifestOmittedEntry {
                path: dest_rel_path,
                omitted: "File does not exist".to_string(),
            });
            continue;
        }
        // D2: Bound source reads even when files grow after metadata. Large logs are omitted;
        // the manifest gives users a route to inspect them separately.
        let raw = File::open(&src_path).and_then(|file| {
            if file.metadata()?.len() > MAX_SCANNABLE_BYTES as u64 {
                return Err(std::io::Error::other("File exceeds maximum scannable size"));
            }
            let mut bytes = Vec::new();
            file.take((MAX_SCANNABLE_BYTES + 1) as u64)
                .read_to_end(&mut bytes)?;
            Ok(bytes)
        });
        match raw {
            Err(e) => {
                omitted.push(ManifestOmittedEntry {
                    path: dest_rel_path,
                    omitted: format!("Failed to read file: {e}"),
                });
            }
            Ok(raw_bytes) => match is_scannable_text(&raw_bytes) {
                Err(reason) => {
                    omitted.push(ManifestOmittedEntry {
                        path: dest_rel_path,
                        omitted: reason,
                    });
                }
                Ok(text) => {
                    let redacted = redact_text(text, home);
                    if !Path::new(&dest_rel_path)
                        .components()
                        .all(|part| matches!(part, std::path::Component::Normal(_)))
                    {
                        return Err(format!("Invalid bundle path: {dest_rel_path}"));
                    }
                    let dest_path = bundle_dir.join(&dest_rel_path);
                    if let Some(parent) = dest_path.parent() {
                        if parent != bundle_dir {
                            match fs::create_dir(parent) {
                                Ok(()) => {}
                                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                                    let metadata = fs::symlink_metadata(parent).map_err(|e| {
                                        format!(
                                            "Failed to inspect directory {}: {e}",
                                            parent.display()
                                        )
                                    })?;
                                    if !metadata.is_dir() || metadata.file_type().is_symlink() {
                                        return Err(format!(
                                            "Unsafe directory {}",
                                            parent.display()
                                        ));
                                    }
                                }
                                Err(e) => {
                                    return Err(format!(
                                        "Failed to create directory {}: {e}",
                                        parent.display()
                                    ))
                                }
                            }
                        }
                    }
                    let mut entry = write_bundle_file(&dest_path, redacted.as_bytes())?;
                    entry.path = dest_rel_path;
                    files.push(entry);
                }
            },
        }
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    omitted.sort_by(|a, b| a.path.cmp(&b.path));

    // 4. manifest.json 작성
    let manifest = SupportBundleManifest {
        schema_version: SCHEMA_VERSION,
        created_at,
        files,
        omitted,
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| format!("Failed to serialize manifest: {e}"))?;
    let manifest_path = bundle_dir.join(MANIFEST_FILE);
    write_bundle_file(&manifest_path, &manifest_bytes)?;

    Ok(SupportBundleResult {
        bundle_dir: bundle_dir.to_string_lossy().to_string(),
        manifest_path: manifest_path.to_string_lossy().to_string(),
        files_count: manifest.files.len(),
        omitted_count: manifest.omitted.len(),
        manifest,
    })
}

fn collect_log_sources(logs_dir: &Path) -> (Vec<(String, PathBuf)>, Vec<ManifestOmittedEntry>) {
    let mut sources = Vec::new();
    let mut omitted = Vec::new();
    match fs::read_dir(logs_dir) {
        Err(e) => omitted.push(ManifestOmittedEntry {
            path: "logs/".to_string(),
            omitted: format!("Failed to open log directory: {e}"),
        }),
        Ok(entries) => {
            for entry in entries {
                match entry {
                    Err(e) => omitted.push(ManifestOmittedEntry {
                        path: "logs/".to_string(),
                        omitted: format!("Failed to read log directory entry: {e}"),
                    }),
                    Ok(entry) => {
                        let path = entry.path();
                        let name = entry.file_name().to_string_lossy().to_string();
                        match entry.file_type() {
                            Ok(kind) if kind.is_file() => {
                                sources.push((format!("logs/{name}"), path))
                            }
                            Ok(kind) if kind.is_symlink() => omitted.push(ManifestOmittedEntry {
                                path: format!("logs/{name}"),
                                omitted: "Symlink log entry cannot be safely scanned".to_string(),
                            }),
                            Ok(_) => {}
                            Err(e) => omitted.push(ManifestOmittedEntry {
                                path: format!("logs/{name}"),
                                omitted: format!("Failed to inspect log entry: {e}"),
                            }),
                        }
                    }
                }
            }
        }
    }
    (sources, omitted)
}

fn collect_home_logs(home: Option<&Path>) -> (Vec<(String, PathBuf)>, Vec<ManifestOmittedEntry>) {
    match home {
        Some(home) => collect_log_sources(&home.join(".kubemetal").join("logs")),
        None => (
            Vec::new(),
            vec![ManifestOmittedEntry {
                path: "logs/".to_string(),
                omitted: "HOME is unavailable; log directory cannot be located".to_string(),
            }],
        ),
    }
}

/// Tauri 커맨드용 비동기 래퍼
pub async fn create_support_bundle_impl(
    app: &tauri::AppHandle,
) -> Result<SupportBundleResult, String> {
    use tauri::Manager;

    let health_summary_json =
        match crate::commands::health::get_system_health_summary(app.clone()).await {
            Ok(summary) => match serde_json::to_string_pretty(&summary) {
                Ok(json) => Some(json),
                Err(e) => Some(
                    serde_json::json!({
                        "status": "failed",
                        "error": format!("Failed to serialize health summary: {e}")
                    })
                    .to_string(),
                ),
            },
            Err(err) => Some(
                serde_json::json!({
                    "status": "failed",
                    "error": err
                })
                .to_string(),
            ),
        };

    let app_info = collect_app_os_version_info(Some(app));

    let app_data_dir = app.path().app_data_dir().unwrap_or_else(|_| {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(".kubemetal"))
            .unwrap_or_else(|| std::env::temp_dir().join("kubemetal"))
    });
    let target_base_dir = app_data_dir.join("support-bundles");

    let home = std::env::var_os("HOME").map(PathBuf::from);

    tokio::task::spawn_blocking(move || {
        let (log_sources, omitted) = collect_home_logs(home.as_deref());
        generate_support_bundle_with_omissions(
            &target_base_dir,
            home.as_deref(),
            health_summary_json,
            app_info,
            log_sources,
            omitted,
        )
    })
    .await
    .map_err(|e| format!("Support bundle background task failed: {e}"))?
}

fn format_bundle_timestamps() -> Result<(String, String), String> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("System clock error: {e}"))?
        .as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day % 3600) / 60;
    let second = seconds_of_day % 60;

    let rfc3339 = format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z");
    let dir_tag = format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}");
    Ok((rfc3339, dir_tag))
}

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
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn create_test_temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let dir = std::env::temp_dir().join(format!("kubemetal-test-{name}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_existing_bundle_name_and_symlink_are_not_followed() {
        let base = create_test_temp_dir("bundle-boundary");
        let outside = base.join("outside.json");
        fs::write(&outside, "untouched").unwrap();
        let (_, stamp) = format_bundle_timestamps().unwrap();
        let planted = base.join(format!("support-bundle-{stamp}"));
        fs::create_dir(&planted).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, planted.join("app_info.json")).unwrap();
        let info = AppOsVersionInfo {
            app_name: "KubeMetal".into(),
            app_version: "0.2.0".into(),
            os: "macos".into(),
            os_version: None,
            kernel_version: None,
            arch: "aarch64".into(),
        };
        let result = generate_support_bundle(&base, None, None, info, vec![]).unwrap();
        assert_ne!(Path::new(&result.bundle_dir), planted);
        assert_eq!(fs::read_to_string(outside).unwrap(), "untouched");
        fs::remove_dir_all(base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_output_file_creation_rejects_symlink() {
        let base = create_test_temp_dir("output-symlink");
        let outside = base.join("outside.json");
        fs::write(&outside, "untouched").unwrap();
        let planted = base.join("app_info.json");
        std::os::unix::fs::symlink(&outside, &planted).unwrap();
        assert!(write_bundle_file(&planted, b"replacement").is_err());
        assert_eq!(fs::read_to_string(outside).unwrap(), "untouched");
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn test_log_directory_open_failure_is_recorded() {
        let base = create_test_temp_dir("log-open-failure");
        let logs = base.join("logs");
        fs::write(&logs, "not a directory").unwrap();
        let (sources, omitted) = collect_log_sources(&logs);
        assert!(sources.is_empty());
        assert_eq!(omitted.len(), 1);
        assert!(omitted[0].omitted.contains("Failed to open log directory"));
        let info = AppOsVersionInfo {
            app_name: "KubeMetal".into(),
            app_version: "0.2.0".into(),
            os: "macos".into(),
            os_version: None,
            kernel_version: None,
            arch: "aarch64".into(),
        };
        let result =
            generate_support_bundle_with_omissions(&base, None, None, info, sources, omitted)
                .unwrap();
        assert_eq!(result.omitted_count, 1);
        let manifest: SupportBundleManifest =
            serde_json::from_slice(&fs::read(result.manifest_path).unwrap()).unwrap();
        assert!(manifest.omitted[0]
            .omitted
            .contains("Failed to open log directory"));
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn test_tampered_binary_file_detection() {
        let binary_data = vec![0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x00, 0xff, 0xfe];
        let result = is_scannable_text(&binary_data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Binary content"));

        let invalid_utf8 = vec![0xff, 0xfe, 0xfd];
        let result_utf8 = is_scannable_text(&invalid_utf8);
        assert!(result_utf8.is_err());
        assert!(result_utf8.unwrap_err().contains("Non-UTF-8"));
    }

    #[test]
    fn test_manifest_hash_matches_file_contents() {
        let temp_dir = create_test_temp_dir("manifest-hash");
        let home_dir = create_test_temp_dir("home-hash");

        let log_file = home_dir.join("test.log");
        fs::write(&log_file, "sample log content line 1\nBearer token123").unwrap();

        let app_info = AppOsVersionInfo {
            app_name: "KubeMetal".into(),
            app_version: "0.2.0".into(),
            os: "macos".into(),
            os_version: Some("15.0".into()),
            kernel_version: Some("24.0.0".into()),
            arch: "aarch64".into(),
        };

        let result = generate_support_bundle(
            &temp_dir,
            Some(&home_dir),
            Some(r#"{"status":"healthy"}"#.into()),
            app_info,
            vec![("logs/test.log".into(), log_file)],
        )
        .unwrap();

        assert_eq!(result.manifest.files.len(), 3); // app_info.json, health_summary.json, logs/test.log

        let bundle_path = Path::new(&result.bundle_dir);
        for entry in &result.manifest.files {
            let file_path = bundle_path.join(&entry.path);
            assert!(file_path.is_file(), "File missing: {}", file_path.display());
            let computed_hash = sha256_file(&file_path).unwrap();
            assert_eq!(
                computed_hash, entry.sha256,
                "Hash mismatch for {}",
                entry.path
            );
            let metadata = fs::metadata(&file_path).unwrap();
            assert_eq!(
                metadata.len(),
                entry.bytes,
                "Byte mismatch for {}",
                entry.path
            );
        }

        fs::remove_dir_all(&temp_dir).ok();
        fs::remove_dir_all(&home_dir).ok();
    }

    #[test]
    fn test_omitted_file_recorded_in_manifest() {
        let temp_dir = create_test_temp_dir("manifest-omitted");
        let source_dir = create_test_temp_dir("source-omitted");

        let binary_file = source_dir.join("corrupt.bin");
        fs::write(&binary_file, [0x00, 0x99, 0xff, 0xfe]).unwrap();

        let normal_file = source_dir.join("normal.log");
        fs::write(&normal_file, "normal log content").unwrap();

        let app_info = AppOsVersionInfo {
            app_name: "KubeMetal".into(),
            app_version: "0.2.0".into(),
            os: "macos".into(),
            os_version: None,
            kernel_version: None,
            arch: "aarch64".into(),
        };

        let result = generate_support_bundle(
            &temp_dir,
            None,
            None,
            app_info,
            vec![
                ("logs/normal.log".into(), normal_file),
                ("logs/corrupt.bin".into(), binary_file),
            ],
        )
        .unwrap();

        assert_eq!(result.omitted_count, 1);
        assert_eq!(result.manifest.omitted.len(), 1);
        let omitted_entry = &result.manifest.omitted[0];
        assert_eq!(omitted_entry.path, "logs/corrupt.bin");
        assert!(omitted_entry.omitted.contains("Binary content"));

        let bundle_path = Path::new(&result.bundle_dir);
        assert!(bundle_path.join("logs/normal.log").exists());
        assert!(!bundle_path.join("logs/corrupt.bin").exists());

        fs::remove_dir_all(&temp_dir).ok();
        fs::remove_dir_all(&source_dir).ok();
    }

    #[test]
    fn test_oversized_source_is_omitted_before_reading() {
        let base = create_test_temp_dir("oversized");
        let source = base.join("oversized.log");
        let file = File::create(&source).unwrap();
        file.set_len((MAX_SCANNABLE_BYTES + 1) as u64).unwrap();
        let result = generate_support_bundle(
            &base,
            None,
            None,
            AppOsVersionInfo {
                app_name: "KubeMetal".into(),
                app_version: "0.2.0".into(),
                os: "macos".into(),
                os_version: None,
                kernel_version: None,
                arch: "aarch64".into(),
            },
            vec![("logs/oversized.log".into(), source)],
        )
        .unwrap();
        assert!(result.manifest.omitted[0]
            .omitted
            .contains("maximum scannable size"));
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn test_missing_home_is_recorded() {
        let (sources, omitted) = collect_home_logs(None);
        assert!(sources.is_empty());
        assert_eq!(omitted[0].path, "logs/");
        assert!(omitted[0].omitted.contains("HOME is unavailable"));
    }

    #[cfg(unix)]
    #[test]
    fn test_bundle_root_symlink_is_refused() {
        let base = create_test_temp_dir("root-link");
        let outside = base.join("outside");
        fs::create_dir(&outside).unwrap();
        let root = base.join("support-bundles");
        std::os::unix::fs::symlink(&outside, &root).unwrap();
        let error = generate_support_bundle(
            &root,
            None,
            None,
            AppOsVersionInfo {
                app_name: "KubeMetal".into(),
                app_version: "0.2.0".into(),
                os: "macos".into(),
                os_version: None,
                kernel_version: None,
                arch: "aarch64".into(),
            },
            vec![],
        )
        .unwrap_err();
        assert!(error.contains("Omitted support bundle"));
        assert!(error.contains("symlink"));
        assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
        fs::remove_dir_all(base).unwrap();
    }
}
