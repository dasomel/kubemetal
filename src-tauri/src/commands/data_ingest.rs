use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::commands::access::resolve_s3_credentials;
use crate::commands::mlx::{
    validate_home_subpath, venv_python,
};
use crate::commands::rag::default_lancedb_dir;
use crate::services::process::{augmented_path, resolve_bundled_resource};

/// `run_data_ingest`의 IPC 계약(프론트 레인과 합의): 커맨드 인자는 단일 `config` 객체이며,
/// 프론트는 camelCase 필드로 전달한다(`#[serde(rename_all = "camelCase")]`) — 같은 패턴을
/// 쓰는 `mlx.rs::FineTuneConfig`(snake_case 그대로)와는 의도적으로 다르다.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestConfig {
    pub source_type: String,
    pub source_path: String,
    pub collection_name: Option<String>,
    pub embedding_model: Option<String>,
    pub chunk_size: Option<u32>,
    pub chunk_overlap: Option<u32>,
    pub enable_dvc_backup: Option<bool>,
    pub dvc_remote_url: Option<String>,
    pub dvc_bucket: Option<String>,
}

/// D21 SSRF 가드: scheme allowlist(http/https) + 사설/루프백 호스트 거부. `scripts/data/ingest_host.py`
/// 의 `_validate_url`과 동일 규칙을 Rust 측에서도 적용한다(이중 방어 — 스폰 이전에 걸러
/// 프로세스 기동 자체를 막는다). `source_type`이 web/rss일 때만 호출부에서 사용한다.
fn validate_ingest_url(url: &str) -> Result<String, String> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| format!("허용되지 않은 URL 형식입니다: {url}"))?;
    if scheme != "http" && scheme != "https" {
        return Err(format!("허용되지 않은 URL 스킴입니다: {scheme}"));
    }

    let host_port_path = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = host_port_path.rsplit('@').next().unwrap_or(host_port_path);
    let host = if let Some(bracket_end) = host_port.strip_prefix('[').and_then(|s| s.find(']')) {
        host_port[1..=bracket_end].to_string()
    } else {
        host_port.split(':').next().unwrap_or(host_port).to_string()
    };
    let host_lower = host.to_lowercase();

    if host_lower.is_empty()
        || host_lower == "localhost"
        || host_lower.ends_with(".internal")
        || host_lower.ends_with(".local")
    {
        return Err(format!("사설/루프백 네트워크 대상은 허용되지 않습니다: {host}"));
    }

    if let Ok(ip) = host_lower.parse::<IpAddr>() {
        let blocked = ip_blocked(ip);
        if blocked {
            return Err(format!("사설/루프백 네트워크 대상은 허용되지 않습니다: {host}"));
        }
    }

    Ok(host_lower)
}

/// 사설/루프백/링크로컬/ULA 판정 — 리터럴 IP 검사와 DNS 해석 검사가 공유한다.
fn ip_blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7 (unique local)
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10 (link local)
        }
    }
}

/// DNS-name 우회 차단(보안 리뷰 지적): 리터럴 IP가 아닌 호스트는 해석된 **모든** 주소를
/// 검사한다 — `evil.example.com → 127.0.0.1` 류가 리터럴 검사만으로는 통과하기 때문.
/// 해석 실패도 차단으로 취급한다(파이썬 측 `_is_blocked_host`와 동일 규칙, 이중 방어).
async fn ensure_public_resolution(host: &str) -> Result<(), String> {
    if host.parse::<IpAddr>().is_ok() {
        return Ok(()); // 리터럴 IP는 validate_ingest_url에서 이미 검사됨
    }
    let addrs = tokio::net::lookup_host((host, 443u16))
        .await
        .map_err(|e| format!("호스트 해석 실패(차단): {host} ({e})"))?;
    for sa in addrs {
        if ip_blocked(sa.ip()) {
            return Err(format!("차단된 IP로 해석되는 호스트입니다: {host} -> {}", sa.ip()));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagNodeState {
    pub node_id: String,
    pub name: String,
    pub status: String,
    pub duration_sec: f64,
    pub items_processed: u32,
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestFlowResult {
    pub status: String,
    pub dataset_name: String,
    pub source_type: String,
    pub source_path: String,
    pub total_duration_sec: f64,
    pub total_items_extracted: u32,
    pub total_chunks_created: u32,
    pub lancedb_collection: String,
    pub db_path: String,
    pub dvc_backed_up: bool,
    pub dag_nodes: Vec<DagNodeState>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestedDatasetInfo {
    pub collection_name: String,
    pub total_chunks: u64,
    pub db_path: String,
    pub is_lance_table: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct IngestStatusResponse {
    pub env_installed: bool,
    pub default_db_path: String,
    pub active_collections: Vec<IngestedDatasetInfo>,
    pub last_result: Option<IngestFlowResult>,
}

#[derive(Default)]
pub struct DataIngestState {
    pub last_result: Mutex<Option<IngestFlowResult>>,
}

async fn check_python_env_available() -> bool {
    let Ok(venv_py) = venv_python() else {
        return false;
    };
    venv_py.is_file()
}

pub fn list_datasets_in_db() -> Vec<IngestedDatasetInfo> {
    let Ok(db_dir) = default_lancedb_dir() else {
        return Vec::new();
    };
    if !db_dir.is_dir() {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(&db_dir) else {
        return Vec::new();
    };

    let mut datasets = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(ext) = path.extension() {
                if ext == "lance" {
                    if let Some(name) = path.file_stem() {
                        datasets.push(IngestedDatasetInfo {
                            collection_name: name.to_string_lossy().to_string(),
                            total_chunks: 0, // dynamic count if queried
                            db_path: path.to_string_lossy().to_string(),
                            is_lance_table: true,
                        });
                    }
                }
            }
        } else if path.is_file() {
            let name_str = path.file_name().unwrap_or_default().to_string_lossy();
            if name_str.ends_with("_fallback.json") {
                let collection = name_str.trim_end_matches("_fallback.json").to_string();
                datasets.push(IngestedDatasetInfo {
                    collection_name: collection,
                    total_chunks: 0,
                    db_path: path.to_string_lossy().to_string(),
                    is_lance_table: false,
                });
            }
        }
    }
    datasets
}

#[tauri::command]
pub async fn run_data_ingest(
    app: tauri::AppHandle,
    state: State<'_, DataIngestState>,
    config: IngestConfig,
) -> Result<IngestFlowResult, String> {
    let IngestConfig {
        source_type,
        source_path,
        collection_name,
        embedding_model,
        chunk_size,
        chunk_overlap,
        enable_dvc_backup,
        dvc_remote_url,
        dvc_bucket,
    } = config;

    if source_path.trim().is_empty() {
        return Err("소스 경로는 비어 있을 수 없습니다.".into());
    }

    let stype_lower = source_type.to_lowercase();
    let target_source_path = if stype_lower == "local" {
        validate_home_subpath(&source_path)?
            .to_string_lossy()
            .to_string()
    } else {
        if stype_lower == "web" || stype_lower == "rss" {
            let host = validate_ingest_url(&source_path)?;
            ensure_public_resolution(&host).await?;
        }
        source_path.clone()
    };

    let venv_py = venv_python()?;
    let py_cmd = if venv_py.is_file() {
        venv_py
    } else {
        PathBuf::from("python3")
    };

    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let ingest_script = resolve_bundled_resource(&resource_dir, "scripts/data/ingest_host.py");
    if !ingest_script.is_file() {
        return Err(format!(
            "수집 스크립트를 찾을 수 없습니다: {}",
            ingest_script.display()
        ));
    }

    let collection = collection_name.unwrap_or_else(|| "dataset_ingest".to_string());
    let model = embedding_model.unwrap_or_else(|| "sentence-transformers/all-MiniLM-L6-v2".to_string());
    let c_size = chunk_size.unwrap_or(500);
    let c_overlap = chunk_overlap.unwrap_or(50);
    let db_dir = default_lancedb_dir()?;

    let mut cmd = tokio::process::Command::new(&py_cmd);
    cmd.arg(&ingest_script)
        .arg("--source-type")
        .arg(&source_type)
        .arg("--source-path")
        .arg(&target_source_path)
        .arg("--collection")
        .arg(&collection)
        .arg("--db-path")
        .arg(&db_dir)
        .arg("--embedding-model")
        .arg(&model)
        .arg("--chunk-size")
        .arg(c_size.to_string())
        .arg("--chunk-overlap")
        .arg(c_overlap.to_string())
        .env("PATH", augmented_path());

    if enable_dvc_backup.unwrap_or(false) {
        cmd.arg("--dvc-backup");
        if let Some(ref r_url) = dvc_remote_url {
            cmd.arg("--remote-url").arg(r_url);
        }
        if let Some(ref bucket) = dvc_bucket {
            cmd.arg("--bucket").arg(bucket);
        }
        // D13/D21: 크리덴셜은 CLI 인자(ps로 노출됨)가 아니라 env var로 주입한다.
        let (s3_access_key, s3_secret_key) = resolve_s3_credentials().await;
        cmd.env("KUBEMETAL_S3_ACCESS_KEY", &s3_access_key)
            .env("KUBEMETAL_S3_SECRET_KEY", &s3_secret_key);
    }

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("데이터 수집 파이프라인 프로세스 실행 실패: {e}"))?;

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    if stdout_str.trim().is_empty() && !output.status.success() {
        let stderr_str = String::from_utf8_lossy(&output.stderr);
        return Err(format!("수집 파이프라인 오류: {stderr_str}"));
    }

    let result: IngestFlowResult = serde_json::from_str(&stdout_str)
        .map_err(|e| format!("JSON 파싱 실패 ({e}): {stdout_str}"))?;

    if let Ok(mut guard) = state.last_result.lock() {
        *guard = Some(result.clone());
    }

    if result.status != "ok" {
        let err_msg = result.error.as_deref().unwrap_or("데이터 수집 중 오류가 발생했습니다.");
        return Err(err_msg.to_string());
    }

    Ok(result)
}

#[tauri::command]
pub async fn get_ingest_status(
    state: State<'_, DataIngestState>,
) -> Result<IngestStatusResponse, String> {
    let env_installed = check_python_env_available().await;
    let default_db_path = default_lancedb_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let active_collections = list_datasets_in_db();
    let last_result = state.last_result.lock().ok().and_then(|g| g.clone());

    Ok(IngestStatusResponse {
        env_installed,
        default_db_path,
        active_collections,
        last_result,
    })
}

#[tauri::command]
pub async fn list_ingested_datasets() -> Result<Vec<IngestedDatasetInfo>, String> {
    Ok(list_datasets_in_db())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_ingest_url_accepts_public_https() {
        assert!(validate_ingest_url("https://docs.kubemetal.io/feed.xml").is_ok());
    }

    #[test]
    fn validate_ingest_url_rejects_non_http_scheme() {
        assert!(validate_ingest_url("file:///etc/passwd").is_err());
        assert!(validate_ingest_url("ftp://example.com/x").is_err());
    }

    #[test]
    fn validate_ingest_url_rejects_loopback_and_private_hosts() {
        for url in [
            "http://127.0.0.1:4200",
            "http://localhost:8080",
            "http://10.0.0.5/",
            "http://172.16.0.5/",
            "http://192.168.1.1/",
            "http://169.254.169.254/latest/meta-data",
            "http://svc.internal/x",
        ] {
            assert!(validate_ingest_url(url).is_err(), "should reject {url}");
        }
    }

    #[test]
    fn validate_ingest_url_rejects_malformed_url() {
        assert!(validate_ingest_url("not-a-url").is_err());
    }
}
