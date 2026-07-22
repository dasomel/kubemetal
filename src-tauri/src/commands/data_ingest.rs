use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::commands::mlx::{
    validate_home_subpath, venv_python,
};
use crate::commands::rag::default_lancedb_dir;
use crate::services::process::{augmented_path, resolve_bundled_resource};

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
    source_type: String,
    source_path: String,
    collection_name: Option<String>,
    embedding_model: Option<String>,
    chunk_size: Option<u32>,
    chunk_overlap: Option<u32>,
    enable_dvc_backup: Option<bool>,
    dvc_remote_url: Option<String>,
    dvc_bucket: Option<String>,
) -> Result<IngestFlowResult, String> {
    if source_path.trim().is_empty() {
        return Err("소스 경로는 비어 있을 수 없습니다.".into());
    }

    let target_source_path = if source_type.to_lowercase() == "local" {
        validate_home_subpath(&source_path)?
            .to_string_lossy()
            .to_string()
    } else {
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
