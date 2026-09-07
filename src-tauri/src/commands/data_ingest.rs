use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{Manager, State};

use crate::commands::access::resolve_s3_credentials;
use crate::commands::mlx::{
    validate_home_subpath, venv_python,
};
use crate::commands::rag::default_lancedb_dir;
use crate::services::ports;
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
        .ok_or_else(|| format!("Invalid URL format: {url}"))?;
    if scheme != "http" && scheme != "https" {
        return Err(format!("Disallowed URL scheme: {scheme}"));
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
        return Err(format!("Private/loopback network targets are not allowed: {host}"));
    }

    if let Ok(ip) = host_lower.parse::<IpAddr>() {
        let blocked = ip_blocked(ip);
        if blocked {
            return Err(format!("Private/loopback network targets are not allowed: {host}"));
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
        .map_err(|e| format!("Host resolution failed (blocked): {host} ({e})"))?;
    for sa in addrs {
        if ip_blocked(sa.ip()) {
            return Err(format!("Host resolves to a blocked IP: {host} -> {}", sa.ip()));
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

/// D38과 같은 사상(성공한 산출물에 sha256 증빙을 남긴다, `mlx.rs::write_training_manifest`
/// #22)을 데이터셋 ingest에도 적용한다(#14 축소 스코프) — dedup/DVC 연결 검증/버전 관리는
/// 이 스코프에 없다, 판정 함수만 제공한다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetProvenance {
    pub collection_name: String,
    pub source_type: String,
    pub source_path: String,
    pub source_hash: Option<String>,
    pub source_hash_reason: Option<String>,
    pub embedding_model: String,
    pub chunk_size: u32,
    pub chunk_overlap: u32,
    pub ingested_at_unix: u64,
}

/// 컬렉션 이름을 파일명으로 안전하게 만든다 — `scripts/airgap/download_airgap_bundle.sh`의
/// `tr '/:' '_'` 관례를 그대로 따른다(`colima.rs`가 같은 관례를 이미지 파일명에 적용).
fn sanitize_collection_name(name: &str) -> String {
    name.chars()
        .map(|c| if c == '/' || c == ':' { '_' } else { c })
        .collect()
}

/// 컬렉션별 provenance 파일 경로 — 같은 LanceDB 디렉터리에 여러 컬렉션이 있을 수 있으므로
/// 컬렉션 이름별로 별도 파일에 기록해 서로 덮어쓰지 않게 한다.
fn provenance_file_path(db_dir: &Path, collection_name: &str) -> PathBuf {
    db_dir.join(format!("{}.provenance.json", sanitize_collection_name(collection_name)))
}

/// `path`가 파일이면 그 파일 하나, 디렉터리면 재귀적으로 파일들을 정렬된 순서로 모아
/// 반환한다(재현 가능한 해시를 위해 — `read_dir`은 순서를 보장하지 않는다). 심볼릭
/// 링크는 건너뛴다(`write_training_manifest`와 동일 원칙).
fn collect_files_sorted(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if path.is_symlink() {
        return Ok(());
    }
    if path.is_file() {
        out.push(path.to_path_buf());
        return Ok(());
    }
    if path.is_dir() {
        let entries = std::fs::read_dir(path)
            .map_err(|e| format!("Failed to read directory {}: {e}", path.display()))?;
        let mut children: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        children.sort();
        for child in children {
            collect_files_sorted(&child, out)?;
        }
        return Ok(());
    }
    Err(format!("Source path does not exist: {}", path.display()))
}

/// 파일 또는 디렉터리(재귀)의 sha256 — `write_training_manifest`(#22)가 어댑터 디렉터리를
/// 다루는 방식과 같은 사상: 정렬된 파일 순서로 이어붙여 재현 가능한 해시를 만든다.
fn hash_source_path(path: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    collect_files_sorted(path, &mut files)?;
    let mut hasher = Sha256::new();
    for f in &files {
        let bytes = std::fs::read(f).map_err(|e| format!("Failed to read {}: {e}", f.display()))?;
        hasher.update(&bytes);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// 순수 함수: 이미 계산된 값들로 provenance JSON을 만든다(파일시스템 부작용 없음).
fn build_provenance_json(provenance: &DatasetProvenance) -> Result<String, String> {
    serde_json::to_string_pretty(provenance)
        .map_err(|e| format!("Failed to serialize dataset provenance: {e}"))
}

/// ingest 성공 경로에서만 호출한다(D38과 동일 원칙 — 실패 시에는 provenance.json을 남기지
/// 않는다). `source_type`이 local이면 `resolved_source_path`(검증·확장된 실제 경로)의
/// sha256을 기록하고, web/rss/hf처럼 로컬 파일이 없는 소스는 해시를 지어내지 않고(D22)
/// `source_hash: null` + 이유를 남긴다. 프런트 소비자가 아직 없어 IPC로는 노출하지
/// 않는다(`manifest_verification_status`와 같은 이유) — `run_data_ingest` 성공 경로에서
/// 내부적으로만 호출한다.
///
/// 인자 8개는 clippy 기본 한도(7)를 넘지만, 호출부가 `run_data_ingest` 하나뿐인 내부
/// 전용 함수를 위해 별도 파라미터 구조체를 두는 건 이 스코프에 과하다고 판단했다.
#[allow(clippy::too_many_arguments)]
pub(crate) fn write_dataset_provenance(
    db_dir: &Path,
    collection_name: &str,
    source_type: &str,
    source_path: &str,
    resolved_source_path: &str,
    embedding_model: &str,
    chunk_size: u32,
    chunk_overlap: u32,
) -> Result<(), String> {
    let (source_hash, source_hash_reason) = if source_type.to_lowercase() == "local" {
        (Some(hash_source_path(Path::new(resolved_source_path))?), None)
    } else {
        (
            None,
            Some(format!(
                "{source_type} source has no local file to hash"
            )),
        )
    };

    let ingested_at_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let provenance = DatasetProvenance {
        collection_name: collection_name.to_string(),
        source_type: source_type.to_string(),
        source_path: source_path.to_string(),
        source_hash,
        source_hash_reason,
        embedding_model: embedding_model.to_string(),
        chunk_size,
        chunk_overlap,
        ingested_at_unix,
    };

    let json = build_provenance_json(&provenance)?;
    std::fs::create_dir_all(db_dir)
        .map_err(|e| format!("Failed to create LanceDB directory: {e}"))?;
    std::fs::write(provenance_file_path(db_dir, collection_name), json)
        .map_err(|e| format!("Failed to write provenance.json: {e}"))
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
        return Err("Source path must not be empty.".into());
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
            "Ingest script not found: {}",
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
        // 호출자가 지정하지 않으면 포워딩이 실제로 잡은 S3 포트를 명시로 넘긴다.
        // 파이썬 쪽 기본값(8333 고정)에 맡기면 대체 포트로 밀렸을 때 조용히 어긋난다.
        match dvc_remote_url {
            Some(ref r_url) => {
                cmd.arg("--remote-url").arg(r_url);
            }
            None => {
                cmd.arg("--remote-url").arg(ports::local_url("seaweedfs-s3"));
            }
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
        .map_err(|e| format!("Failed to execute data ingest pipeline process: {e}"))?;

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    if stdout_str.trim().is_empty() && !output.status.success() {
        let stderr_str = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Ingest pipeline error: {stderr_str}"));
    }

    let result: IngestFlowResult = serde_json::from_str(&stdout_str)
        .map_err(|e| format!("JSON parsing failed ({e}): {stdout_str}"))?;

    if let Ok(mut guard) = state.last_result.lock() {
        *guard = Some(result.clone());
    }

    if result.status != "ok" {
        let err_msg = result.error.as_deref().unwrap_or("An error occurred during data ingest.");
        return Err(err_msg.to_string());
    }

    // #14 축소 스코프(D38과 동일 사상): 성공한 ingest 산출물에만 provenance 매니페스트를
    // 남긴다. 매니페스트 기록 실패가 이미 성공한 ingest 결과 반환을 막아서는 안 되므로
    // 에러는 전파하지 않고 로그로만 남긴다(D22 — 조용히 삼키지 않되, ingest 자체의
    // 성공/실패 판정과는 분리한다).
    if let Err(e) = write_dataset_provenance(
        &db_dir,
        &collection,
        &source_type,
        &source_path,
        &target_source_path,
        &model,
        c_size,
        c_overlap,
    ) {
        eprintln!("Failed to write dataset provenance manifest: {e}");
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

    /// 테스트 전용 임시 디렉터리 — tempfile 크레이트 없이 `mlx.rs`(`make_temp_model_dir`)와
    /// 같은 관례(`std::env::temp_dir()` + 고유 접미사)를 따른다.
    fn make_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kubemetal-data-ingest-test-{name}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("failed to create temp dir");
        dir
    }

    #[test]
    fn sanitize_collection_name_replaces_slash_and_colon() {
        assert_eq!(sanitize_collection_name("my/dataset:v1"), "my_dataset_v1");
        assert_eq!(sanitize_collection_name("plain_name"), "plain_name");
    }

    #[test]
    fn build_provenance_json_is_pure_and_round_trips() {
        let provenance = DatasetProvenance {
            collection_name: "docs".to_string(),
            source_type: "local".to_string(),
            source_path: "/tmp/docs".to_string(),
            source_hash: Some("abc123".to_string()),
            source_hash_reason: None,
            embedding_model: "sentence-transformers/all-MiniLM-L6-v2".to_string(),
            chunk_size: 500,
            chunk_overlap: 50,
            ingested_at_unix: 1_700_000_000,
        };
        let json = build_provenance_json(&provenance).expect("serialization should succeed");
        let parsed: DatasetProvenance =
            serde_json::from_str(&json).expect("provenance JSON should parse back");
        assert_eq!(parsed.collection_name, "docs");
        assert_eq!(parsed.source_hash.as_deref(), Some("abc123"));
        assert_eq!(parsed.chunk_size, 500);
    }

    #[test]
    fn hash_source_path_matches_single_file_sha256() {
        let dir = make_temp_dir("single-file");
        let file_path = dir.join("doc.txt");
        std::fs::write(&file_path, b"hello kubemetal").unwrap();

        let expected = hex::encode(Sha256::digest(b"hello kubemetal"));
        assert_eq!(hash_source_path(&file_path).unwrap(), expected);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn hash_source_path_is_order_independent_for_directory_contents() {
        let dir = make_temp_dir("dir-order");
        std::fs::write(dir.join("b.txt"), b"second").unwrap();
        std::fs::write(dir.join("a.txt"), b"first").unwrap();

        // read_dir 순서와 무관하게 파일명 정렬(a.txt, b.txt) 순으로 이어붙인 해시와 같아야
        // 재현성이 보장된다.
        let mut hasher = Sha256::new();
        hasher.update(b"first");
        hasher.update(b"second");
        let expected = hex::encode(hasher.finalize());

        assert_eq!(hash_source_path(&dir).unwrap(), expected);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn hash_source_path_fails_on_missing_path() {
        let dir = make_temp_dir("missing");
        let missing = dir.join("does-not-exist");
        assert!(hash_source_path(&missing).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_dataset_provenance_writes_expected_fields_for_local_source() {
        let source_dir = make_temp_dir("provenance-local-source");
        std::fs::write(source_dir.join("a.txt"), b"content-a").unwrap();
        let db_dir = make_temp_dir("provenance-local-db");

        write_dataset_provenance(
            &db_dir,
            "my_collection",
            "local",
            "~/Documents/my_collection",
            source_dir.to_str().unwrap(),
            "sentence-transformers/all-MiniLM-L6-v2",
            500,
            50,
        )
        .expect("provenance write should succeed");

        let provenance_path = provenance_file_path(&db_dir, "my_collection");
        assert!(provenance_path.is_file());
        let content = std::fs::read_to_string(&provenance_path).unwrap();
        let parsed: DatasetProvenance = serde_json::from_str(&content).unwrap();

        assert_eq!(parsed.collection_name, "my_collection");
        assert_eq!(parsed.source_type, "local");
        assert_eq!(parsed.source_path, "~/Documents/my_collection");
        assert_eq!(
            parsed.source_hash.as_deref(),
            Some(hash_source_path(&source_dir).unwrap().as_str())
        );
        assert!(parsed.source_hash_reason.is_none());
        assert_eq!(parsed.chunk_size, 500);
        assert_eq!(parsed.chunk_overlap, 50);

        std::fs::remove_dir_all(&source_dir).ok();
        std::fs::remove_dir_all(&db_dir).ok();
    }

    #[test]
    fn write_dataset_provenance_keeps_separate_files_per_collection() {
        let source_dir = make_temp_dir("provenance-multi-source");
        std::fs::write(source_dir.join("a.txt"), b"content-a").unwrap();
        let db_dir = make_temp_dir("provenance-multi-db");

        write_dataset_provenance(
            &db_dir, "coll_one", "local", "/x", source_dir.to_str().unwrap(), "model", 500, 50,
        )
        .unwrap();
        write_dataset_provenance(
            &db_dir, "coll_two", "local", "/y", source_dir.to_str().unwrap(), "model", 500, 50,
        )
        .unwrap();

        assert!(provenance_file_path(&db_dir, "coll_one").is_file());
        assert!(provenance_file_path(&db_dir, "coll_two").is_file());

        std::fs::remove_dir_all(&source_dir).ok();
        std::fs::remove_dir_all(&db_dir).ok();
    }

    #[test]
    fn write_dataset_provenance_records_null_hash_with_reason_for_url_source() {
        let db_dir = make_temp_dir("provenance-url-db");

        write_dataset_provenance(
            &db_dir,
            "rss_collection",
            "rss",
            "https://docs.kubemetal.io/feed.xml",
            "https://docs.kubemetal.io/feed.xml",
            "sentence-transformers/all-MiniLM-L6-v2",
            500,
            50,
        )
        .expect("provenance write should succeed for URL sources");

        let content =
            std::fs::read_to_string(provenance_file_path(&db_dir, "rss_collection")).unwrap();
        let parsed: DatasetProvenance = serde_json::from_str(&content).unwrap();

        assert!(parsed.source_hash.is_none());
        assert!(parsed.source_hash_reason.is_some());
        assert!(parsed.source_hash_reason.unwrap().contains("rss"));

        std::fs::remove_dir_all(&db_dir).ok();
    }

    #[test]
    fn write_dataset_provenance_does_not_write_file_when_local_source_missing() {
        let db_dir = make_temp_dir("provenance-failure-db");
        let missing_source = db_dir.join("does-not-exist");

        let result = write_dataset_provenance(
            &db_dir,
            "broken_collection",
            "local",
            "/does/not/exist",
            missing_source.to_str().unwrap(),
            "model",
            500,
            50,
        );

        assert!(result.is_err());
        assert!(!provenance_file_path(&db_dir, "broken_collection").is_file());

        std::fs::remove_dir_all(&db_dir).ok();
    }
}
