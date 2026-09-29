//! #17 축소 스코프: 서포트 번들 생성 서비스
//!
//! 시스템 헬스 요약(`get_system_health_summary`), 앱/OS/버전 정보, 최근 로그(`~/.kubemetal/logs/`)를
//! 수집해 앱 데이터 디렉터리 하위 `support-bundles/support-bundle-<timestamp>`에 저장한다.
//! 민감정보(토큰, kubeconfig 키/인증서 데이터, Authorization 헤더, API 키, 사용자 홈 경로 `~` 치환)를
//! 자동 redaction하며, 바이너리/비-UTF-8 등 검사 불가능한 파일은 fail-closed로 제외하고
//! `manifest.json`에 `omitted: <reason>`으로 기록한다.

use std::fs;
#[cfg(test)]
use std::fs::File;
#[cfg(test)]
use std::io::{BufReader, Read};
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

/// kubeconfig 관련 민감 데이터(client-key-data 등) 마스킹
fn redact_kubeconfig_fields(line: &str) -> String {
    const KUBE_KEYS: &[&str] = &[
        "client-key-data",
        "client-certificate-data",
        "certificate-authority-data",
    ];
    let mut out = line.to_string();
    for key in KUBE_KEYS {
        let colon_pattern = format!("{key}:");
        if let Some(pos) = out.find(&colon_pattern) {
            let after_colon = &out[pos + colon_pattern.len()..];
            let trimmed = after_colon.trim_start();
            let leading_spaces = &after_colon[..after_colon.len() - trimmed.len()];
            if !trimmed.is_empty() && !trimmed.starts_with("[REDACTED]") {
                let val_len = trimmed
                    .find(|c: char| c.is_whitespace() || c == ',' || c == ';')
                    .unwrap_or(trimmed.len());
                let rest = &trimmed[val_len..];
                out = format!(
                    "{}{}[REDACTED]{}",
                    &out[..pos + colon_pattern.len()],
                    leading_spaces,
                    rest
                );
            }
        }
        let json_pattern = format!("\"{key}\"");
        if let Some(pos) = out.find(&json_pattern) {
            if let Some(colon_pos) = out[pos..].find(':') {
                let abs_colon = pos + colon_pos;
                let rest = &out[abs_colon + 1..];
                if let Some(first_quote) = rest.find('"') {
                    let after_first_quote = &rest[first_quote + 1..];
                    if let Some(second_quote) = after_first_quote.find('"') {
                        let before = &out[..abs_colon + 1 + first_quote + 1];
                        let after = &after_first_quote[second_quote..];
                        out = format!("{before}[REDACTED]{after}");
                    }
                }
            }
        }
    }
    out
}

/// Authorization 헤더 마스킹
fn redact_authorization_headers(line: &str) -> String {
    let lower = line.to_lowercase();
    if !lower.contains("authorization") {
        return line.to_string();
    }
    let mut out = line.to_string();
    for json_key in &["\"authorization\"", "\"Authorization\""] {
        if let Some(pos) = out.find(json_key) {
            if let Some(colon_pos) = out[pos..].find(':') {
                let abs_colon = pos + colon_pos;
                let rest = &out[abs_colon + 1..];
                if let Some(first_quote) = rest.find('"') {
                    let after_first_quote = &rest[first_quote + 1..];
                    if let Some(second_quote) = after_first_quote.find('"') {
                        let before = &out[..abs_colon + 1 + first_quote + 1];
                        let after = &after_first_quote[second_quote..];
                        out = format!("{before}[REDACTED]{after}");
                    }
                }
            }
        }
    }
    for header in &["authorization:", "Authorization:"] {
        if let Some(pos) = out.find(header) {
            let after = &out[pos + header.len()..];
            let trimmed = after.trim_start();
            let leading = &after[..after.len() - trimmed.len()];
            if let Some(rest) = trimmed
                .strip_prefix("Bearer ")
                .or_else(|| trimmed.strip_prefix("bearer "))
            {
                let trimmed_token = rest.trim_start();
                let end = trimmed_token
                    .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                    .unwrap_or(trimmed_token.len());
                let remaining = &trimmed_token[end..];
                out = format!(
                    "{}{}Bearer [REDACTED]{}",
                    &out[..pos + header.len()],
                    leading,
                    remaining
                );
            } else if let Some(rest) = trimmed
                .strip_prefix("Basic ")
                .or_else(|| trimmed.strip_prefix("basic "))
            {
                let trimmed_token = rest.trim_start();
                let end = trimmed_token
                    .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                    .unwrap_or(trimmed_token.len());
                let remaining = &trimmed_token[end..];
                out = format!(
                    "{}{}Basic [REDACTED]{}",
                    &out[..pos + header.len()],
                    leading,
                    remaining
                );
            } else if !trimmed.is_empty() && !trimmed.starts_with("[REDACTED]") {
                let end = trimmed
                    .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                    .unwrap_or(trimmed.len());
                let remaining = &trimmed[end..];
                out = format!(
                    "{}{}[REDACTED]{}",
                    &out[..pos + header.len()],
                    leading,
                    remaining
                );
            }
        }
    }
    out
}

/// Bearer 토큰 마스킹
fn redact_bearer_tokens(line: &str) -> String {
    let mut out = line.to_string();
    let mut start_idx = 0;
    while let Some(found) = out[start_idx..]
        .find("Bearer ")
        .or_else(|| out[start_idx..].find("bearer "))
    {
        let abs_pos = start_idx + found;
        let token_start = abs_pos + "Bearer ".len();
        let rest = &out[token_start..];
        let trimmed = rest.trim_start();
        let leading_len = rest.len() - trimmed.len();
        if trimmed.starts_with("[REDACTED]") {
            start_idx = token_start + "[REDACTED]".len();
            continue;
        }
        let token_len = trimmed
            .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',' || c == ';')
            .unwrap_or(trimmed.len());
        if token_len > 0 {
            let before = &out[..token_start];
            let leading = &rest[..leading_len];
            let after = &trimmed[token_len..];
            let new_out = format!("{before}{leading}[REDACTED]{after}");
            start_idx = token_start + leading_len + "[REDACTED]".len();
            out = new_out;
        } else {
            start_idx = token_start;
        }
    }
    out
}

/// 민감 키-값(API 키, 패스워드, 토큰 등) 마스킹
fn redact_sensitive_key_values(line: &str) -> String {
    const KEYS: &[&str] = &[
        "api_key",
        "apiKey",
        "apikey",
        "api-key",
        "x-api-key",
        "password",
        "passwd",
        "pwd",
        "client_secret",
        "secret_key",
        "secret",
        "access_token",
        "refresh_token",
        "auth_token",
        "id_token",
        "token",
        "private_key",
    ];

    let mut out = line.to_string();
    for key in KEYS {
        // 1. JSON 형식: "key": "..."
        let json_key = format!("\"{key}\"");
        if let Some(pos) = out.find(&json_key) {
            if let Some(colon_pos) = out[pos..].find(':') {
                let abs_colon = pos + colon_pos;
                let rest = &out[abs_colon + 1..];
                let trimmed = rest.trim_start();
                let leading = &rest[..rest.len() - trimmed.len()];
                if let Some(inside) = trimmed.strip_prefix('"') {
                    if let Some(end_quote) = inside.find('"') {
                        let val = &inside[..end_quote];
                        if val != "[REDACTED]" && !val.is_empty() {
                            let before = &out[..abs_colon + 1];
                            let after = &inside[end_quote..];
                            out = format!("{before}{leading}\"[REDACTED]\"{after}");
                        }
                    }
                }
            }
        }

        // 2. YAML 형식: key: ...
        let yaml_key = format!("{key}:");
        let lower = out.to_lowercase();
        if let Some(pos) = lower.find(&yaml_key) {
            let is_boundary = pos == 0 || out.as_bytes()[pos - 1].is_ascii_whitespace();
            if is_boundary {
                let after = &out[pos + yaml_key.len()..];
                let trimmed = after.trim_start();
                let leading = &after[..after.len() - trimmed.len()];
                if !trimmed.is_empty() && !trimmed.starts_with("[REDACTED]") {
                    let end = trimmed
                        .find(|c: char| c.is_whitespace() || c == ',' || c == ';')
                        .unwrap_or(trimmed.len());
                    let remaining = &trimmed[end..];
                    out = format!(
                        "{}{}[REDACTED]{}",
                        &out[..pos + yaml_key.len()],
                        leading,
                        remaining
                    );
                }
            }
        }

        // 3. ENV / Assignment 형식: KEY=...
        let env_key = format!("{key}=");
        let lower = out.to_lowercase();
        if let Some(pos) = lower.find(&env_key) {
            let is_boundary = pos == 0
                || out.as_bytes()[pos - 1].is_ascii_whitespace()
                || out.as_bytes()[pos - 1] == b'&';
            if is_boundary {
                let after = &out[pos + env_key.len()..];
                let trimmed = after.trim_start();
                let leading = &after[..after.len() - trimmed.len()];
                if !trimmed.is_empty() && !trimmed.starts_with("[REDACTED]") {
                    let end = trimmed
                        .find(|c: char| c.is_whitespace() || c == '&' || c == ';')
                        .unwrap_or(trimmed.len());
                    let remaining = &trimmed[end..];
                    out = format!(
                        "{}{}[REDACTED]{}",
                        &out[..pos + env_key.len()],
                        leading,
                        remaining
                    );
                }
            }
        }
    }
    out
}

/// 접두사 기반 독립 토큰 마스킹 (sk-, ghp_, glpat-, hf_)
fn redact_prefixed_tokens(line: &str) -> String {
    const PREFIXES: &[&str] = &["sk-ant-", "sk-", "ghp_", "glpat-", "hf_"];
    let mut out = line.to_string();
    for prefix in PREFIXES {
        let mut start_idx = 0;
        while let Some(found) = out[start_idx..].find(prefix) {
            let abs_pos = start_idx + found;
            let token_chars = &out[abs_pos..];
            let token_len = token_chars
                .find(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
                .unwrap_or(token_chars.len());
            if token_len >= 10 && &token_chars[..token_len] != "[REDACTED]" {
                let before = &out[..abs_pos];
                let after = &out[abs_pos + token_len..];
                let new_out = format!("{before}[REDACTED]{after}");
                start_idx = abs_pos + "[REDACTED]".len();
                out = new_out;
            } else {
                start_idx = abs_pos + prefix.len();
            }
        }
    }
    out
}

/// 단일 라인 문자열에 대한 모든 redaction 적용
pub fn redact_line(line: &str) -> String {
    let s = redact_kubeconfig_fields(line);
    let s = redact_authorization_headers(&s);
    let s = redact_bearer_tokens(&s);
    let s = redact_sensitive_key_values(&s);
    redact_prefixed_tokens(&s)
}

/// 전체 텍스트 redaction 및 사용자 홈 경로 `~` 치환
pub fn redact_text(text: &str, home: Option<&Path>) -> String {
    let mut result = String::with_capacity(text.len());
    for line in text.lines() {
        let redacted = redact_line(line);
        result.push_str(&redacted);
        result.push('\n');
    }
    if !text.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    if let Some(home) = home {
        let home_str = home.to_string_lossy();
        if !home_str.is_empty() && home_str != "/" {
            let escaped_home = home_str.replace('/', "\\/");
            let mut replaced = result.replace(&escaped_home, "~");
            replaced = replaced.replace(home_str.as_ref(), "~");
            return replaced;
        }
    }
    result
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
pub fn generate_support_bundle(
    target_base_dir: &Path,
    home: Option<&Path>,
    health_summary_json: Option<String>,
    app_info: AppOsVersionInfo,
    log_sources: Vec<(String, PathBuf)>,
) -> Result<SupportBundleResult, String> {
    let (created_at, dir_timestamp) = format_bundle_timestamps()?;
    let bundle_dir_name = format!("support-bundle-{dir_timestamp}");
    let bundle_dir = target_base_dir.join(&bundle_dir_name);
    fs::create_dir_all(&bundle_dir)
        .map_err(|e| format!("Failed to create bundle dir {}: {e}", bundle_dir.display()))?;

    let mut files = Vec::new();
    let mut omitted = Vec::new();

    // 1. app_info.json 작성
    let app_info_str = serde_json::to_string_pretty(&app_info)
        .map_err(|e| format!("Failed to serialize app info: {e}"))?;
    let app_info_redacted = redact_text(&app_info_str, home);
    let app_info_path = bundle_dir.join("app_info.json");
    fs::write(&app_info_path, app_info_redacted.as_bytes())
        .map_err(|e| format!("Failed to write app_info.json: {e}"))?;
    files.push(ManifestFileEntry {
        path: "app_info.json".to_string(),
        sha256: sha256_digest(app_info_redacted.as_bytes()),
        bytes: app_info_redacted.len() as u64,
    });

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
    fs::write(&health_path, health_redacted.as_bytes())
        .map_err(|e| format!("Failed to write health_summary.json: {e}"))?;
    files.push(ManifestFileEntry {
        path: "health_summary.json".to_string(),
        sha256: sha256_digest(health_redacted.as_bytes()),
        bytes: health_redacted.len() as u64,
    });

    // 3. 로그 소스 수집 및 redaction (fail-closed 검증)
    for (dest_rel_path, src_path) in log_sources {
        if !src_path.exists() {
            omitted.push(ManifestOmittedEntry {
                path: dest_rel_path,
                omitted: "File does not exist".to_string(),
            });
            continue;
        }
        match fs::read(&src_path) {
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
                    let dest_path = bundle_dir.join(&dest_rel_path);
                    if let Some(parent) = dest_path.parent() {
                        fs::create_dir_all(parent).map_err(|e| {
                            format!("Failed to create directory {}: {e}", parent.display())
                        })?;
                    }
                    fs::write(&dest_path, redacted.as_bytes())
                        .map_err(|e| format!("Failed to write {}: {e}", dest_path.display()))?;
                    files.push(ManifestFileEntry {
                        path: dest_rel_path,
                        sha256: sha256_digest(redacted.as_bytes()),
                        bytes: redacted.len() as u64,
                    });
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
    fs::write(&manifest_path, manifest_bytes)
        .map_err(|e| format!("Failed to write manifest.json: {e}"))?;

    Ok(SupportBundleResult {
        bundle_dir: bundle_dir.to_string_lossy().to_string(),
        manifest_path: manifest_path.to_string_lossy().to_string(),
        files_count: manifest.files.len(),
        omitted_count: manifest.omitted.len(),
        manifest,
    })
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

    let mut log_sources = Vec::new();
    if let Some(ref h) = home {
        let logs_dir = h.join(".kubemetal").join("logs");
        if logs_dir.is_dir() {
            if let Ok(entries) = fs::read_dir(&logs_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                            log_sources.push((format!("logs/{file_name}"), path));
                        }
                    }
                }
            }
        }
    }

    tokio::task::spawn_blocking(move || {
        generate_support_bundle(
            &target_base_dir,
            home.as_deref(),
            health_summary_json,
            app_info,
            log_sources,
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
    fn test_redact_bearer_token() {
        let input = "2026-09-29 12:00:00 Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.secret-token-123\ncurl -H 'Bearer ghp_1234567890abcdef' http://127.0.0.1:8080";
        let redacted = redact_text(input, None);
        assert!(!redacted.contains("eyJhbGciOiJIUzI1NiJ9.secret-token-123"));
        assert!(!redacted.contains("ghp_1234567890abcdef"));
        assert!(redacted.contains("Bearer [REDACTED]"));
    }

    #[test]
    fn test_redact_kubeconfig_key_data() {
        let yaml_input = r#"apiVersion: v1
clusters:
- cluster:
    certificate-authority-data: LS0tLS1CRUdJTiBDRVJUSUZJQ0FURS0tLS0tCg==
  name: colima
users:
- name: colima
  user:
    client-certificate-data: LS0tLS1CRUdJTiBDRVJUSUZJQ0FURS0tLS0tCg==
    client-key-data: LS0tLS1CRUdJTiBSU0EgUFJJVkFURSBLRVktLS0tLQo=
"#;
        let redacted_yaml = redact_text(yaml_input, None);
        assert!(!redacted_yaml.contains("LS0tLS1CRUdJTiBDRVJUSUZJQ0FURS0tLS0tCg=="));
        assert!(!redacted_yaml.contains("LS0tLS1CRUdJTiBSU0EgUFJJVkFURSBLRVktLS0tLQo="));
        assert!(redacted_yaml.contains("certificate-authority-data: [REDACTED]"));
        assert!(redacted_yaml.contains("client-certificate-data: [REDACTED]"));
        assert!(redacted_yaml.contains("client-key-data: [REDACTED]"));

        let json_input = r#"{"client-key-data": "LS0tLS1CRUdJTiBSU0EgUFJJVkFURSBLRVktLS0tLQo="}"#;
        let redacted_json = redact_text(json_input, None);
        assert!(!redacted_json.contains("LS0tLS1CRUdJTiBSU0EgUFJJVkFURSBLRVktLS0tLQo="));
        assert!(redacted_json.contains("\"client-key-data\": \"[REDACTED]\""));
    }

    #[test]
    fn test_redact_home_path() {
        let home = Path::new("/Users/developer");
        let input = "Model loaded from /Users/developer/.kubemetal/models/qwen2.5\nEscaped: \\/Users\\/developer\\/adapters";
        let redacted = redact_text(input, Some(home));
        assert!(!redacted.contains("/Users/developer"));
        assert!(redacted.contains("~/.kubemetal/models/qwen2.5"));
        assert!(redacted.contains("~\\/adapters"));
    }

    #[test]
    fn test_redact_api_keys_and_tokens() {
        let input = r#"
api_key: sk-1234567890abcdef12345
export HF_TOKEN=hf_abcdef1234567890
{"password": "super-secret-pass", "normal_field": "keep-me"}
"#;
        let redacted = redact_text(input, None);
        assert!(!redacted.contains("sk-1234567890abcdef12345"));
        assert!(!redacted.contains("hf_abcdef1234567890"));
        assert!(!redacted.contains("super-secret-pass"));
        assert!(redacted.contains("keep-me"));
        assert!(redacted.contains("[REDACTED]"));
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
}
