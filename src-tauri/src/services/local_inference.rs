use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::time::timeout;

use crate::services::process::{augmented_path, resolve_cli_path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocalInferenceRuntimeKind {
    Omlx,
    MlxLm,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeCapabilities {
    pub openai_chat: bool,
    pub openai_responses: bool,
    pub anthropic_messages: bool,
    pub embeddings: bool,
    pub rerank: bool,
    pub multi_model: bool,
    pub model_load_unload: bool,
    pub model_pinning: bool,
    pub model_ttl: bool,
    pub continuous_batching: bool,
    pub tiered_kv_cache: bool,
    pub mcp: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeProbe {
    pub runtime: LocalInferenceRuntimeKind,
    pub installed: bool,
    pub executable: Option<String>,
    pub version: Option<String>,
    pub endpoint: String,
    pub managed_by_kubemetal: bool,
    pub capabilities: RuntimeCapabilities,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeLaunchConfig {
    pub runtime: LocalInferenceRuntimeKind,
    pub port: u16,
    pub model_dir: Option<String>,
    #[serde(default = "default_true")]
    pub cache_enabled: bool,
    pub paged_ssd_cache_dir: Option<String>,
    pub paged_ssd_cache_max_size: Option<String>,
    pub hot_cache_max_size: Option<String>,
    pub max_concurrent_requests: Option<u32>,
    /// `omlx serve --memory-guard` takes a required value (measured 2026-09-06: bare
    /// `--memory-guard` fails argparse). `None` omits the flag; `Some` must be one of
    /// `MEMORY_GUARD_TIERS`, validated in `build_omlx_command`. Pinning is a post-start
    /// admin-API concern (`set_omlx_model_settings_sparse`), not a launch flag — `omlx serve`
    /// has no `--pin` (measured); there is no launch-config field for it.
    pub memory_guard_tier: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Valid values for `omlx serve --memory-guard <tier>` (measured against 0.6.4's argparse).
pub const MEMORY_GUARD_TIERS: [&str; 4] = ["off", "safe", "balanced", "aggressive"];

impl Default for RuntimeLaunchConfig {
    fn default() -> Self {
        Self {
            runtime: LocalInferenceRuntimeKind::Omlx,
            port: 8000,
            model_dir: None,
            cache_enabled: true,
            paged_ssd_cache_dir: None,
            paged_ssd_cache_max_size: None,
            hot_cache_max_size: None,
            max_concurrent_requests: None,
            memory_guard_tier: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeLiveStatus {
    pub runtime: LocalInferenceRuntimeKind,
    pub endpoint: String,
    pub reachable: bool,
    pub healthy: bool,
    pub health_status_code: Option<u16>,
    pub health_detail: Option<String>,
    pub models: Vec<RuntimeModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeModel {
    pub id: String,
    pub display_name: Option<String>,
    pub model_path: Option<String>,
    pub loaded: bool,
    pub loading: bool,
    pub pinned: Option<bool>,
    pub is_default: Option<bool>,
    pub model_type: Option<String>,
    pub engine_type: Option<String>,
    pub estimated_size: Option<u64>,
    pub actual_size: Option<u64>,
    pub alias: Option<String>,
    pub ttl_seconds: Option<u64>,
}

#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    /// Header name/value pairs in wire order, name un-lowercased (HTTP header names are
    /// case-insensitive — callers that need to find one should compare case-insensitively,
    /// see `extract_admin_session_cookie`). A repeated header (e.g. multiple `Set-Cookie`)
    /// appears as multiple entries.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn omlx_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(path) = resolve_cli_path("omlx") {
        candidates.push(path);
    }
    if let Some(home) = home_dir() {
        candidates.push(home.join(".omlx/bin/omlx"));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/omlx"));
    candidates.push(PathBuf::from("/usr/local/bin/omlx"));
    candidates
}

fn mlx_lm_python_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = home_dir() {
        candidates.push(home.join(".kubemetal/venv/bin/python"));
    }
    if let Ok(path) = resolve_cli_path("python3") {
        candidates.push(path);
    }
    candidates
}

async fn command_version(program: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .env("PATH", augmented_path())
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let text = if stdout.is_empty() { stderr } else { stdout };
    (!text.is_empty()).then_some(text.lines().next().unwrap_or_default().to_string())
}

fn capabilities(runtime: LocalInferenceRuntimeKind) -> RuntimeCapabilities {
    match runtime {
        LocalInferenceRuntimeKind::Omlx => RuntimeCapabilities {
            openai_chat: true,
            openai_responses: true,
            anthropic_messages: true,
            embeddings: true,
            rerank: true,
            multi_model: true,
            model_load_unload: true,
            model_pinning: true,
            model_ttl: true,
            continuous_batching: true,
            tiered_kv_cache: true,
            mcp: true,
        },
        LocalInferenceRuntimeKind::MlxLm => RuntimeCapabilities {
            openai_chat: true,
            openai_responses: false,
            anthropic_messages: false,
            embeddings: false,
            rerank: false,
            multi_model: false,
            model_load_unload: false,
            model_pinning: false,
            model_ttl: false,
            continuous_batching: false,
            tiered_kv_cache: false,
            mcp: false,
        },
    }
}

pub async fn probe_runtime(runtime: LocalInferenceRuntimeKind) -> RuntimeProbe {
    match runtime {
        LocalInferenceRuntimeKind::Omlx => {
            let executable = omlx_candidates().into_iter().find(|p| p.is_file());
            let version = match executable.as_ref() {
                Some(path) => command_version(path, &["--version"]).await,
                None => None,
            };
            RuntimeProbe {
                runtime,
                installed: executable.is_some(),
                executable: executable.as_ref().map(|p| p.display().to_string()),
                version,
                endpoint: "http://127.0.0.1:8000".into(),
                managed_by_kubemetal: false,
                capabilities: capabilities(runtime),
                detail: executable.is_none().then_some(
                    "oMLX not detected. Install via the official package/Homebrew; KubeMetal never installs it implicitly.".into(),
                ),
            }
        }
        LocalInferenceRuntimeKind::MlxLm => {
            let python = mlx_lm_python_candidates().into_iter().find(|p| p.is_file());
            let version = match python.as_ref() {
                Some(path) => {
                    command_version(
                        path,
                        &[
                            "-c",
                            "import importlib.metadata as m; print(m.version('mlx-lm'))",
                        ],
                    )
                    .await
                }
                None => None,
            };
            RuntimeProbe {
                runtime,
                installed: version.is_some(),
                executable: python.as_ref().map(|p| p.display().to_string()),
                version,
                endpoint: "http://127.0.0.1:8080".into(),
                managed_by_kubemetal: true,
                capabilities: capabilities(runtime),
                detail: None,
            }
        }
    }
}

pub async fn probe_all_runtimes() -> Vec<RuntimeProbe> {
    let (omlx, mlx_lm) = tokio::join!(
        probe_runtime(LocalInferenceRuntimeKind::Omlx),
        probe_runtime(LocalInferenceRuntimeKind::MlxLm)
    );
    vec![omlx, mlx_lm]
}

fn validate_home_path(value: &str, must_exist: bool) -> Result<PathBuf, String> {
    let home = home_dir().ok_or_else(|| "HOME is unavailable".to_string())?;
    let expanded = if let Some(rest) = value.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(value)
    };
    let path = if must_exist {
        expanded
            .canonicalize()
            .map_err(|e| format!("Path not found: {value} ({e})"))?
    } else {
        let parent = expanded
            .parent()
            .ok_or_else(|| format!("Invalid path: {value}"))?;
        let canonical_parent = parent
            .canonicalize()
            .map_err(|e| format!("Parent path not found: {} ({e})", parent.display()))?;
        canonical_parent.join(
            expanded
                .file_name()
                .ok_or_else(|| format!("Invalid path: {value}"))?,
        )
    };
    let canonical_home = home
        .canonicalize()
        .map_err(|e| format!("Failed to resolve HOME: {e}"))?;
    if !path.starts_with(canonical_home) {
        return Err(format!(
            "Local inference paths must stay under the user's home directory: {value}"
        ));
    }
    Ok(path)
}

/// `validate_home_path` canonicalizes paths (blocking `std::fs`). `build_omlx_command` runs on
/// the async runtime, so this offloads the check to a blocking-pool thread instead of stalling
/// the runtime while resolving the filesystem.
async fn validate_home_path_blocking(value: &str, must_exist: bool) -> Result<PathBuf, String> {
    let owned = value.to_string();
    tokio::task::spawn_blocking(move || validate_home_path(&owned, must_exist))
        .await
        .map_err(|e| format!("Path validation task failed: {e}"))?
}

fn validate_size(value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || !trimmed
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'K' | 'M' | 'G' | 'T' | 'B'))
    {
        return Err(format!("Invalid cache size: {value}"));
    }
    Ok(())
}

fn validate_memory_guard_tier(tier: &str) -> Result<(), String> {
    if MEMORY_GUARD_TIERS.contains(&tier) {
        Ok(())
    } else {
        Err(format!(
            "Invalid memory guard tier: {tier} (expected one of {})",
            MEMORY_GUARD_TIERS.join(", ")
        ))
    }
}

pub async fn build_omlx_command(config: &RuntimeLaunchConfig) -> Result<Command, String> {
    if config.runtime != LocalInferenceRuntimeKind::Omlx {
        return Err("The shared runtime lifecycle currently owns only oMLX. mlx-lm remains managed by the existing MLX Studio serving path.".into());
    }
    let executable = omlx_candidates()
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| "oMLX executable not found".to_string())?;

    let mut command = Command::new(executable);
    command
        .arg("serve")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(config.port.to_string())
        .env("PATH", augmented_path());

    if let Some(model_dir) = config.model_dir.as_deref() {
        let owned = model_dir.to_string();
        let path = tokio::task::spawn_blocking(move || -> Result<PathBuf, String> {
            let path = validate_home_path(&owned, true)?;
            if !path.is_dir() {
                return Err(format!("Model directory is not a directory: {owned}"));
            }
            Ok(path)
        })
        .await
        .map_err(|e| format!("Path validation task failed: {e}"))??;
        command.arg("--model-dir").arg(path);
    }
    if !config.cache_enabled {
        command.arg("--no-cache");
    }
    if let Some(dir) = config.paged_ssd_cache_dir.as_deref() {
        let path = validate_home_path_blocking(dir, false).await?;
        command.arg("--paged-ssd-cache-dir").arg(path);
    }
    if let Some(size) = config.paged_ssd_cache_max_size.as_deref() {
        validate_size(size)?;
        command.arg("--paged-ssd-cache-max-size").arg(size);
    }
    if let Some(size) = config.hot_cache_max_size.as_deref() {
        validate_size(size)?;
        command.arg("--hot-cache-max-size").arg(size);
    }
    if let Some(max) = config.max_concurrent_requests {
        if max == 0 || max > 1024 {
            return Err("max_concurrent_requests must be between 1 and 1024".into());
        }
        command.arg("--max-concurrent-requests").arg(max.to_string());
    }
    if let Some(tier) = config.memory_guard_tier.as_deref() {
        validate_memory_guard_tier(tier)?;
        command.arg("--memory-guard").arg(tier);
    }

    command.stdin(Stdio::null());
    Ok(command)
}

pub fn endpoint_for(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn parse_loopback_endpoint(endpoint: &str) -> Result<(String, u16), String> {
    let authority = endpoint
        .strip_prefix("http://")
        .ok_or_else(|| "Only loopback http:// endpoints are supported".to_string())?
        .trim_end_matches('/');
    let (host, port) = authority
        .rsplit_once(':')
        .ok_or_else(|| format!("Endpoint must include a port: {endpoint}"))?;
    if host != "127.0.0.1" && host != "localhost" {
        return Err(format!(
            "Refusing non-loopback local inference endpoint: {endpoint}"
        ));
    }
    let port = port
        .parse::<u16>()
        .map_err(|_| format!("Invalid endpoint port: {endpoint}"))?;
    Ok((host.to_string(), port))
}

pub async fn loopback_http_request(
    endpoint: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
    bearer_token: Option<&str>,
) -> Result<HttpResponse, String> {
    loopback_http_request_inner(endpoint, method, path, body, bearer_token, None).await
}

/// Sibling of [`loopback_http_request`] for oMLX's `/admin/api/*` routes, which authenticate
/// via a signed session cookie (`omlx_admin_session`) rather than the bearer token `/v1/*`
/// accepts — measured 2026-09-06: a valid bearer key against `/admin/api/models` still 401s.
/// Get the cookie pair from [`omlx_admin_session`] first.
pub async fn loopback_http_request_with_cookie(
    endpoint: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
    cookie: Option<&str>,
) -> Result<HttpResponse, String> {
    loopback_http_request_inner(endpoint, method, path, body, None, cookie).await
}

async fn loopback_http_request_inner(
    endpoint: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
    bearer_token: Option<&str>,
    cookie: Option<&str>,
) -> Result<HttpResponse, String> {
    let (host, port) = parse_loopback_endpoint(endpoint)?;
    if !path.starts_with('/') || path.contains('\r') || path.contains('\n') {
        return Err("Invalid HTTP path".into());
    }
    if !matches!(method, "GET" | "POST" | "PUT") {
        return Err(format!("Unsupported HTTP method: {method}"));
    }
    let address = format!("{host}:{port}");
    let mut stream = timeout(Duration::from_secs(2), TcpStream::connect(&address))
        .await
        .map_err(|_| format!("Timed out connecting to {address}"))?
        .map_err(|e| format!("Failed to connect to {address}: {e}"))?;

    let payload = body.unwrap_or("");
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nAccept: application/json\r\nConnection: close\r\n"
    );
    if !payload.is_empty() {
        request.push_str("Content-Type: application/json\r\n");
        request.push_str(&format!("Content-Length: {}\r\n", payload.len()));
    }
    if let Some(token) = bearer_token.filter(|t| !t.is_empty()) {
        if token.contains('\r') || token.contains('\n') {
            return Err("Invalid API key".into());
        }
        request.push_str("Authorization: Bearer ");
        request.push_str(token);
        request.push_str("\r\n");
    }
    if let Some(cookie_value) = cookie.filter(|c| !c.is_empty()) {
        if cookie_value.contains('\r') || cookie_value.contains('\n') {
            return Err("Invalid session cookie".into());
        }
        request.push_str("Cookie: ");
        request.push_str(cookie_value);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    request.push_str(payload);

    timeout(Duration::from_secs(2), stream.write_all(request.as_bytes()))
        .await
        .map_err(|_| "Timed out writing local inference request".to_string())?
        .map_err(|e| format!("Failed to write local inference request: {e}"))?;

    let mut bytes = Vec::new();
    timeout(Duration::from_secs(5), stream.read_to_end(&mut bytes))
        .await
        .map_err(|_| "Timed out reading local inference response".to_string())?
        .map_err(|e| format!("Failed to read local inference response: {e}"))?;
    let response = String::from_utf8_lossy(&bytes);
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| "Malformed HTTP response".to_string())?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| "Malformed HTTP status line".to_string())?;
    let headers = head
        .lines()
        .skip(1)
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_string(), value.trim().to_string()))
        })
        .collect();
    Ok(HttpResponse {
        status,
        headers,
        body: body.to_string(),
    })
}

/// Extracts the `omlx_admin_session=<token>` pair from `Set-Cookie` response headers
/// (case-insensitive header name; the cookie value ends at the first `;` — attributes like
/// `HttpOnly`/`SameSite` follow). oMLX may emit multiple `Set-Cookie` headers, so every
/// occurrence is scanned rather than assuming the admin session cookie is first or only.
fn extract_admin_session_cookie(headers: &[(String, String)]) -> Option<String> {
    headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("set-cookie"))
        .find_map(|(_, value)| {
            let pair = value.split(';').next()?.trim();
            let (name, val) = pair.split_once('=')?;
            let (name, val) = (name.trim(), val.trim());
            // Defense in depth: `loopback_http_request_with_cookie` also rejects an embedded
            // CR/LF before it would go out on the wire, but a tampered/synthesized header
            // value must never survive extraction as a usable cookie pair in the first place.
            if val.contains('\r') || val.contains('\n') {
                return None;
            }
            (name == "omlx_admin_session" && !val.is_empty()).then(|| format!("{name}={val}"))
        })
}

/// Exchanges the configured oMLX server API key for an admin session cookie via
/// `/admin/api/login`. `/admin/api/*` (model load/unload/settings, the model list used for
/// live probing) is guarded by a signed session cookie, never the bearer token `/v1/*` takes
/// (measured 2026-09-06). Every admin-API caller must go through this first.
pub async fn omlx_admin_session(endpoint: &str, api_key: &str) -> Result<String, String> {
    if api_key.trim().is_empty() {
        return Err(
            "oMLX admin API requires the server API key configured in oMLX settings; enter it in the runtime card"
                .into(),
        );
    }
    let body = serde_json::json!({ "api_key": api_key, "remember": false }).to_string();
    let response =
        loopback_http_request_with_cookie(endpoint, "POST", "/admin/api/login", Some(&body), None)
            .await?;
    if !(200..300).contains(&response.status) {
        // oMLX returns {"detail": "..."} for both failure modes: 400 "No API key configured.
        // Please set up an API key first." and 401 "Invalid API key" — surface that text
        // verbatim rather than inventing our own guidance for a case we didn't reproduce.
        let detail = serde_json::from_str::<serde_json::Value>(&response.body)
            .ok()
            .and_then(|v| v.get("detail").and_then(|d| d.as_str()).map(str::to_string))
            .filter(|d| !d.trim().is_empty())
            .unwrap_or_else(|| format!("oMLX admin login failed (HTTP {})", response.status));
        return Err(detail);
    }
    extract_admin_session_cookie(&response.headers).ok_or_else(|| {
        "oMLX admin login succeeded but did not return a session cookie".to_string()
    })
}

fn json_bool(value: &serde_json::Value, key: &str) -> Option<bool> {
    value.get(key).and_then(serde_json::Value::as_bool)
}

fn json_u64(value: &serde_json::Value, key: &str) -> Option<u64> {
    value.get(key).and_then(serde_json::Value::as_u64)
}

fn json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
}

fn parse_admin_models(body: &str) -> Vec<RuntimeModel> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let Some(models) = value.get("models").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    models
        .iter()
        .filter_map(|model| {
            let id = json_string(model, "id")?;
            let settings = model.get("settings").unwrap_or(&serde_json::Value::Null);
            Some(RuntimeModel {
                id,
                display_name: json_string(model, "display_name"),
                model_path: json_string(model, "model_path"),
                loaded: json_bool(model, "loaded").unwrap_or(false),
                loading: json_bool(model, "is_loading").unwrap_or(false),
                pinned: json_bool(model, "pinned").or_else(|| json_bool(settings, "is_pinned")),
                is_default: json_bool(model, "is_default")
                    .or_else(|| json_bool(settings, "is_default")),
                model_type: json_string(model, "model_type"),
                engine_type: json_string(model, "engine_type"),
                estimated_size: json_u64(model, "estimated_size"),
                actual_size: json_u64(model, "actual_size"),
                alias: json_string(settings, "model_alias"),
                ttl_seconds: json_u64(settings, "ttl_seconds"),
            })
        })
        .collect()
}

fn parse_openai_models(body: &str) -> Vec<RuntimeModel> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    value
        .get("data")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|model| {
            Some(RuntimeModel {
                id: json_string(model, "id")?,
                display_name: None,
                model_path: None,
                loaded: true,
                loading: false,
                pinned: None,
                is_default: None,
                model_type: None,
                engine_type: None,
                estimated_size: None,
                actual_size: None,
                alias: None,
                ttl_seconds: None,
            })
        })
        .collect()
}

pub async fn probe_live_runtime(
    runtime: LocalInferenceRuntimeKind,
    endpoint: &str,
    api_key: Option<&str>,
) -> RuntimeLiveStatus {
    let health = loopback_http_request(endpoint, "GET", "/health", None, api_key).await;
    let (reachable, healthy, health_status_code, health_detail) = match health {
        Ok(response) => (
            true,
            (200..300).contains(&response.status),
            Some(response.status),
            (!response.body.trim().is_empty()).then_some(response.body),
        ),
        Err(error) => (false, false, None, Some(error)),
    };

    let models = if reachable {
        match runtime {
            LocalInferenceRuntimeKind::Omlx => {
                // `/admin/api/models` needs a session cookie, not the bearer key (measured
                // 2026-09-06). Best-effort: no key, a login failure, or a non-2xx admin
                // response all fall back to the unauthenticated `/v1/models` view rather than
                // failing the probe outright — this is discovery, not a guarded mutation.
                let admin_models = match api_key.filter(|key| !key.is_empty()) {
                    Some(key) => match omlx_admin_session(endpoint, key).await {
                        Ok(cookie) => loopback_http_request_with_cookie(
                            endpoint,
                            "GET",
                            "/admin/api/models",
                            None,
                            Some(&cookie),
                        )
                        .await
                        .ok()
                        .filter(|r| (200..300).contains(&r.status))
                        .map(|r| parse_admin_models(&r.body)),
                        Err(_) => None,
                    },
                    None => None,
                };
                match admin_models {
                    Some(models) => models,
                    None => loopback_http_request(endpoint, "GET", "/v1/models", None, api_key)
                        .await
                        .ok()
                        .filter(|r| (200..300).contains(&r.status))
                        .map(|r| parse_openai_models(&r.body))
                        .unwrap_or_default(),
                }
            }
            LocalInferenceRuntimeKind::MlxLm => {
                loopback_http_request(endpoint, "GET", "/v1/models", None, api_key)
                    .await
                    .ok()
                    .filter(|r| (200..300).contains(&r.status))
                    .map(|r| parse_openai_models(&r.body))
                    .unwrap_or_default()
            }
        }
    } else {
        Vec::new()
    };

    RuntimeLiveStatus {
        runtime,
        endpoint: endpoint.to_string(),
        reachable,
        healthy,
        health_status_code,
        health_detail,
        models,
    }
}

pub async fn omlx_model_action(
    endpoint: &str,
    model_id: &str,
    action: &str,
    api_key: Option<&str>,
) -> Result<HttpResponse, String> {
    if model_id.trim().is_empty()
        || model_id.contains('/')
        || model_id.contains('?')
        || model_id.contains('#')
        || model_id.contains('\r')
        || model_id.contains('\n')
    {
        return Err("Invalid oMLX model id".into());
    }
    if !matches!(action, "load" | "unload") {
        return Err(format!("Unsupported model action: {action}"));
    }
    // Unlike the probe, load/unload is a guarded mutation: no key or a bad login must fail
    // loudly with the reason, never silently no-op or hit an unauthenticated route.
    let cookie = omlx_admin_session(endpoint, api_key.unwrap_or("")).await?;
    loopback_http_request_with_cookie(
        endpoint,
        "POST",
        &format!("/admin/api/models/{model_id}/{action}"),
        None,
        Some(&cookie),
    )
    .await
}

pub fn pid_is_running(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let result = unsafe { libc::kill(pid as i32, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

pub fn terminate_pid(pid: u32) -> Result<(), String> {
    if pid == 0 {
        return Err("Invalid process id".into());
    }
    let result = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    if result == 0 {
        Ok(())
    } else {
        Err(format!(
            "Failed to terminate process {pid}: {}",
            std::io::Error::last_os_error()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omlx_exposes_multi_model_capabilities() {
        let caps = capabilities(LocalInferenceRuntimeKind::Omlx);
        assert!(caps.multi_model);
        assert!(caps.continuous_batching);
        assert!(caps.tiered_kv_cache);
        assert!(caps.openai_responses);
        assert!(caps.anthropic_messages);
    }

    #[test]
    fn mlx_lm_remains_basic_fallback_runtime() {
        let caps = capabilities(LocalInferenceRuntimeKind::MlxLm);
        assert!(caps.openai_chat);
        assert!(!caps.multi_model);
        assert!(!caps.tiered_kv_cache);
    }

    #[test]
    fn endpoint_is_loopback_only() {
        assert!(parse_loopback_endpoint("http://127.0.0.1:8000").is_ok());
        assert!(parse_loopback_endpoint("http://localhost:8000").is_ok());
        assert!(parse_loopback_endpoint("http://0.0.0.0:8000").is_err());
        assert!(parse_loopback_endpoint("https://127.0.0.1:8000").is_err());
    }

    #[test]
    fn parses_omlx_admin_model_shape() {
        let models = parse_admin_models(
            r#"{"models":[{"id":"qwen","display_name":"Qwen","loaded":true,"is_loading":false,"pinned":true,"model_type":"llm","settings":{"model_alias":"reasoning","ttl_seconds":600}}]}"#,
        );
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "qwen");
        assert!(models[0].loaded);
        assert_eq!(models[0].alias.as_deref(), Some("reasoning"));
        assert_eq!(models[0].ttl_seconds, Some(600));
    }

    #[test]
    fn rejects_unsafe_model_ids() {
        for id in ["", "../foo", "foo/bar", "foo?x=1", "foo\nbar"] {
            assert!(id.trim().is_empty() || id.contains('/') || id.contains('?') || id.contains('\n'));
        }
    }

    #[test]
    fn accepts_documented_memory_guard_tiers() {
        for tier in MEMORY_GUARD_TIERS {
            assert!(validate_memory_guard_tier(tier).is_ok());
        }
    }

    #[test]
    fn rejects_unknown_memory_guard_tier() {
        assert!(validate_memory_guard_tier("bare").is_err());
        assert!(validate_memory_guard_tier("").is_err());
    }

    #[test]
    fn extracts_admin_session_cookie_from_normal_set_cookie() {
        let headers = vec![(
            "Set-Cookie".to_string(),
            "omlx_admin_session=abc.def.ghi; HttpOnly; SameSite=lax".to_string(),
        )];
        assert_eq!(
            extract_admin_session_cookie(&headers).as_deref(),
            Some("omlx_admin_session=abc.def.ghi")
        );
    }

    #[test]
    fn extracts_admin_session_cookie_case_insensitively() {
        let headers = vec![(
            "set-cookie".to_string(),
            "omlx_admin_session=tok; Path=/".to_string(),
        )];
        assert_eq!(
            extract_admin_session_cookie(&headers).as_deref(),
            Some("omlx_admin_session=tok")
        );
    }

    #[test]
    fn returns_none_when_admin_session_cookie_missing() {
        let headers = vec![("Set-Cookie".to_string(), "other=value; Path=/".to_string())];
        assert_eq!(extract_admin_session_cookie(&headers), None);
        assert_eq!(extract_admin_session_cookie(&[]), None);
    }

    #[test]
    fn finds_admin_session_cookie_among_multiple_set_cookie_headers() {
        let headers = vec![
            ("Set-Cookie".to_string(), "tracking=xyz; Path=/".to_string()),
            (
                "Set-Cookie".to_string(),
                "omlx_admin_session=tok2; HttpOnly".to_string(),
            ),
        ];
        assert_eq!(
            extract_admin_session_cookie(&headers).as_deref(),
            Some("omlx_admin_session=tok2")
        );
    }

    #[test]
    fn rejects_crlf_tampered_cookie_value() {
        // A value carrying a real embedded CR/LF (e.g. a synthesized/tampered header, since
        // the wire parser itself splits on line boundaries before this function ever runs)
        // must be rejected rather than handed back as a cookie pair ready for the next request.
        let tampered = vec![(
            "Set-Cookie".to_string(),
            "omlx_admin_session=tok\r\nX-Injected: 1".to_string(),
        )];
        assert_eq!(extract_admin_session_cookie(&tampered), None);

        // The tampered entry must not block a legitimate cookie found elsewhere in the list.
        let mixed = vec![
            (
                "Set-Cookie".to_string(),
                "omlx_admin_session=tok\r\nX-Injected: 1".to_string(),
            ),
            ("Set-Cookie".to_string(), "omlx_admin_session=real; Path=/".to_string()),
        ];
        assert_eq!(
            extract_admin_session_cookie(&mixed).as_deref(),
            Some("omlx_admin_session=real")
        );

        // A genuinely malformed header (no '=' before ';') must not be mistaken for the cookie.
        let malformed = vec![("Set-Cookie".to_string(), "; Path=/".to_string())];
        assert_eq!(extract_admin_session_cookie(&malformed), None);
    }
}
