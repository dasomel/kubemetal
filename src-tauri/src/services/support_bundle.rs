//! #17 축소 스코프: 서포트 번들 생성 서비스
//!
//! 시스템 헬스 요약(`get_system_health_summary`), 앱/OS/버전 정보, 최근 로그(`~/.kubemetal/logs/`)를
//! 수집해 앱 데이터 디렉터리 하위 `support-bundles/support-bundle-<timestamp>`에 저장한다.
//! 민감정보(토큰, kubeconfig 키/인증서 데이터, Authorization 헤더, API 키, 사용자 홈 경로 `~` 치환)를
//! 자동 redaction하며, 바이너리/비-UTF-8 등 검사 불가능한 파일은 fail-closed로 제외하고
//! `manifest.json`에 `omitted: <reason>`으로 기록한다.

#[cfg(test)]
use std::fs::File;
use std::fs::{self, OpenOptions};
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
        "aws_secret_access_key",
        "aws_access_key_id",
        "github_token",
    ];
    let mut out = line.to_string();
    for key in KEYS {
        let json_key = format!("\"{key}\"");
        let mut cursor = 0;
        while let Some(offset) = out[cursor..].find(&json_key) {
            let pos = cursor + offset + json_key.len();
            let rest = out[pos..].trim_start();
            if let Some(after_colon) = rest.strip_prefix(':') {
                let value = after_colon.trim_start();
                if let Some(inside) = value.strip_prefix('"') {
                    let mut escaped = false;
                    let end = inside.char_indices().find_map(|(i, ch)| {
                        if escaped {
                            escaped = false;
                            None
                        } else if ch == '\\' {
                            escaped = true;
                            None
                        } else if ch == '"' {
                            Some(i)
                        } else {
                            None
                        }
                    });
                    if let Some(end) = end {
                        let start = out.len() - inside.len();
                        if end > 0 && &inside[..end] != "[REDACTED]" {
                            out.replace_range(start..start + end, "[REDACTED]");
                        }
                    }
                }
            }
            cursor = pos;
        }

        for delimiter in [':', '='] {
            let pattern = format!("{key}{delimiter}");
            let mut cursor = 0;
            while let Some(offset) = out[cursor..]
                .to_ascii_lowercase()
                .find(&pattern.to_ascii_lowercase())
            {
                let pos = cursor + offset;
                let end_key = pos + pattern.len();
                let boundary = pos == 0
                    || out.as_bytes()[pos - 1].is_ascii_whitespace()
                    || out.as_bytes()[pos - 1] == b'&';
                if !boundary {
                    cursor = end_key;
                    continue;
                }
                let value = out[end_key..].trim_start();
                let start = out.len() - value.len();
                let end = value
                    .find(|c: char| c.is_whitespace() || c == ',' || c == ';' || c == '&')
                    .unwrap_or(value.len());
                if end > 0 && &value[..end] != "[REDACTED]" {
                    out.replace_range(start..start + end, "[REDACTED]");
                }
                cursor = (start + if end > 0 { "[REDACTED]".len() } else { 0 }).min(out.len());
            }
        }
    }
    out
}

/// 접두사 기반 독립 토큰 마스킹 (sk-, ghp_, glpat-, hf_)
fn redact_prefixed_tokens(line: &str) -> String {
    const PREFIXES: &[&str] = &[
        "sk-ant-",
        "sk-",
        "github_pat_",
        "ghp_",
        "gho_",
        "glpat-",
        "hf_",
        "AKIA",
    ];
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
    redact_url_userinfo(&redact_prefixed_tokens(&s))
}

fn redact_url_userinfo(line: &str) -> String {
    let mut out = line.to_string();
    for scheme in ["https://", "http://"] {
        let mut cursor = 0;
        while let Some(offset) = out[cursor..].find(scheme) {
            let start = cursor + offset + scheme.len();
            let authority = &out[start..];
            let end = authority
                .find(|c: char| c.is_whitespace() || c == '/' || c == '?' || c == '#')
                .unwrap_or(authority.len());
            if let Some(at) = authority[..end].rfind('@') {
                out.replace_range(start..start + at, "[REDACTED]");
                cursor = start + "[REDACTED]@".len();
            } else {
                cursor = start + end;
            }
        }
    }
    out
}

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

/// 전체 텍스트 redaction 및 사용자 홈 경로 `~` 치환
pub fn redact_text(text: &str, home: Option<&Path>) -> String {
    let mut result = String::with_capacity(text.len());
    let mut yaml_indent = None;
    let mut in_pem = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if in_pem {
            if trimmed.starts_with("-----END ") {
                in_pem = false;
            }
            result.push_str("[REDACTED]\n");
            continue;
        }
        if trimmed.starts_with("-----BEGIN ") {
            in_pem = !trimmed.contains("-----END ");
            result.push_str("[REDACTED]\n");
            continue;
        }
        if let Some(block_indent) = yaml_indent {
            if trimmed.is_empty() || indent > block_indent {
                result.push_str("[REDACTED]\n");
                continue;
            }
            yaml_indent = None;
        }
        if [
            "client-key-data",
            "client-certificate-data",
            "certificate-authority-data",
        ]
        .iter()
        .any(|key| {
            trimmed.starts_with(&format!("{key}:"))
                && trimmed.split_once(':').is_some_and(|(_, value)| {
                    matches!(value.trim(), "|" | "|-" | "|+" | ">" | ">-" | ">+")
                })
        }) {
            yaml_indent = Some(indent);
        }
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
        let (log_sources, omitted) = home
            .as_ref()
            .map(|h| collect_log_sources(&h.join(".kubemetal").join("logs")))
            .unwrap_or_default();
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
    fn test_redact_multiline_yaml_pem_and_url_userinfo() {
        let input = "client-key-data: |\n  first-secret-line\n  second-secret-line\nname: visible\n-----BEGIN PRIVATE KEY-----\npem-secret-line\n-----END PRIVATE KEY-----\nurl=https://user:password@host/path";
        let redacted = redact_text(input, None);
        for secret in [
            "first-secret-line",
            "second-secret-line",
            "pem-secret-line",
            "user:password",
        ] {
            assert!(!redacted.contains(secret), "leaked {secret}");
        }
        assert!(redacted.contains("name: visible"));
        assert!(redacted.contains("host/path"));
    }

    #[test]
    fn test_redact_multiple_env_secrets_on_one_line() {
        let input = "AWS_SECRET_ACCESS_KEY=alpha123 AWS_ACCESS_KEY_ID=AKIA1234567890123456 GITHUB_TOKEN=github_pat_1234567890 ghp_1234567890 gho_1234567890 hf_1234567890 password=first password=second";
        let redacted = redact_text(input, None);
        for secret in [
            "alpha123",
            "AKIA1234567890123456",
            "github_pat_1234567890",
            "ghp_1234567890",
            "gho_1234567890",
            "hf_1234567890",
            "first",
            "second",
        ] {
            assert!(!redacted.contains(secret), "leaked {secret}");
        }
    }

    #[test]
    fn test_redacted_json_remains_valid() {
        let redacted = redact_text(
            r#"{"password":"secret","password":"second","normal":"keep"}"#,
            None,
        );
        let value: serde_json::Value = serde_json::from_str(&redacted).unwrap();
        assert_eq!(value["normal"], "keep");
        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("second"));
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
}
