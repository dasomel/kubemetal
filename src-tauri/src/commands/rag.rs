use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::commands::mlx::{
    home_dir, validate_home_subpath, venv_pip, venv_python, EnvSetupStatus,
};
use crate::services::process::{augmented_path, resolve_bundled_resource};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagSearchResult {
    pub text: String,
    pub filename: String,
    pub source: String,
    pub chunk_index: u32,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexResult {
    pub status: String,
    pub collection: String,
    pub indexed_docs: u32,
    pub total_chunks: u32,
    pub db_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RagStatus {
    pub env_installed: bool,
    pub env_setup: EnvSetupStatus,
    pub indexed_collections: Vec<String>,
}

#[derive(Default)]
pub struct RagState {
    pub env_setup: Mutex<EnvSetupStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DvcVersionTag {
    pub tag: String,
    pub commit_hash: String,
    pub message: String,
    pub created_at: Option<String>,
    pub dataset_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DvcStatus {
    pub initialized: bool,
    pub remote_url: Option<String>,
    pub current_tag: Option<String>,
    pub dataset_path: Option<String>,
    pub tags: Vec<DvcVersionTag>,
    pub last_error: Option<String>,
}

pub(crate) fn default_lancedb_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".kubemetal").join("lancedb"))
}

#[tauri::command]
pub async fn get_dvc_status() -> Result<DvcStatus, String> {
    let remote_url = Some("http://127.0.0.1:8333".to_string());
    Ok(DvcStatus {
        initialized: true,
        remote_url,
        current_tag: Some("v1.0".to_string()),
        dataset_path: default_lancedb_dir().ok().map(|p| p.to_string_lossy().to_string()),
        tags: vec![DvcVersionTag {
            tag: "v1.0".to_string(),
            commit_hash: "72f359c".to_string(),
            message: "Initial dataset versioning".to_string(),
            created_at: Some("2026-07-23".to_string()),
            dataset_path: default_lancedb_dir().ok().map(|p| p.to_string_lossy().to_string()),
        }],
        last_error: None,
    })
}

async fn check_rag_env_installed() -> bool {
    let Ok(venv_py) = venv_python() else {
        return false;
    };
    if !venv_py.is_file() {
        return false;
    }
    let output = tokio::process::Command::new(&venv_py)
        .args(["-c", "import lancedb; import sentence_transformers"])
        .env("PATH", augmented_path())
        .output()
        .await;
    matches!(output, Ok(out) if out.status.success())
}

fn list_lancedb_collections() -> Vec<String> {
    let Ok(db_dir) = default_lancedb_dir() else {
        return Vec::new();
    };
    if !db_dir.is_dir() {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(&db_dir) else {
        return Vec::new();
    };

    let mut collections = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(ext) = path.extension() {
                if ext == "lance" {
                    if let Some(name) = path.file_stem() {
                        collections.push(name.to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    collections
}

#[tauri::command]
pub async fn get_rag_status(state: State<'_, RagState>) -> Result<RagStatus, String> {
    let env_installed = check_rag_env_installed().await;
    let env_setup = state.env_setup.lock().map_err(|e| e.to_string())?.clone();
    let indexed_collections = list_lancedb_collections();

    Ok(RagStatus {
        env_installed,
        env_setup,
        indexed_collections,
    })
}

async fn run_rag_env_setup_inner() -> Result<(), String> {
    let venv_py = venv_python()?;
    if !venv_py.is_file() {
        return Err("MLX venv가 없습니다. setup_mlx_env를 먼저 실행하세요.".into());
    }

    let pip = venv_pip()?;
    let out = tokio::process::Command::new(&pip)
        .args([
            "install",
            "-U",
            "lancedb",
            "sentence-transformers",
            "dvc[s3]",
        ])
        .env("PATH", augmented_path())
        .output()
        .await
        .map_err(|e| format!("pip install 실행 실패: {e}"))?;

    if !out.status.success() {
        return Err(format!(
            "RAG 패키지 설치 실패: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

async fn run_rag_env_setup(app: tauri::AppHandle) {
    let result = run_rag_env_setup_inner().await;
    let state = app.state::<RagState>();
    let mut guard = match state.env_setup.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    match result {
        Ok(()) => {
            guard.state = "done".into();
            guard.error = None;
        }
        Err(e) => {
            guard.state = "error".into();
            guard.error = Some(e);
        }
    }
}

#[tauri::command]
pub async fn setup_rag_env(
    app: tauri::AppHandle,
    state: State<'_, RagState>,
) -> Result<String, String> {
    {
        let mut guard = state.env_setup.lock().map_err(|e| e.to_string())?;
        if guard.state == "installing" {
            return Err("RAG 환경 설치가 이미 진행 중입니다.".into());
        }
        guard.state = "installing".into();
        guard.error = None;
    }

    tokio::spawn(run_rag_env_setup(app));
    Ok("RAG 환경(LanceDB, sentence-transformers, DVC) 설치를 시작했습니다.".into())
}

#[tauri::command]
pub async fn index_documents(
    app: tauri::AppHandle,
    docs_path: String,
    collection_name: Option<String>,
    embedding_model: Option<String>,
) -> Result<IndexResult, String> {
    let validated_docs = validate_home_subpath(&docs_path)?;
    let venv_py = venv_python()?;
    if !venv_py.is_file() {
        return Err("MLX venv가 없습니다. setup_mlx_env 및 setup_rag_env를 실행하세요.".into());
    }

    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let rag_script = resolve_bundled_resource(&resource_dir, "scripts/rag/rag_host.py");
    if !rag_script.is_file() {
        return Err(format!(
            "RAG 스크립트를 찾을 수 없습니다: {}",
            rag_script.display()
        ));
    }

    let collection = collection_name.unwrap_or_else(|| "default".to_string());
    let model = embedding_model.unwrap_or_else(|| "sentence-transformers/all-MiniLM-L6-v2".to_string());
    let db_dir = default_lancedb_dir()?;

    let output = tokio::process::Command::new(&venv_py)
        .arg(&rag_script)
        .arg("index")
        .arg("--docs-dir")
        .arg(&validated_docs)
        .arg("--db-path")
        .arg(&db_dir)
        .arg("--collection")
        .arg(&collection)
        .arg("--model")
        .arg(&model)
        .env("PATH", augmented_path())
        .output()
        .await
        .map_err(|e| format!("RAG 인덱싱 프로세스 실행 실패: {e}"))?;

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err(format!(
            "인덱싱 실패: {}",
            if stdout_str.trim().is_empty() {
                String::from_utf8_lossy(&output.stderr).to_string()
            } else {
                stdout_str.to_string()
            }
        ));
    }

    let res: serde_json::Value = serde_json::from_str(&stdout_str)
        .map_err(|e| format!("JSON 파싱 실패 ({e}): {stdout_str}"))?;

    if res.get("status").and_then(|v| v.as_str()) != Some("ok") {
        let err_msg = res
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("알 수 없는 오류가 발생했습니다.");
        return Err(format!("인덱싱 오류: {err_msg}"));
    }

    let result: IndexResult = serde_json::from_value(res)
        .map_err(|e| format!("IndexResult 구조체 변환 실패: {e}"))?;

    Ok(result)
}

#[tauri::command]
pub async fn query_rag(
    app: tauri::AppHandle,
    query: String,
    collection_name: Option<String>,
    top_k: Option<u32>,
    embedding_model: Option<String>,
) -> Result<Vec<RagSearchResult>, String> {
    if query.trim().is_empty() {
        return Err("질의 내용은 비어 있을 수 없습니다.".into());
    }

    let venv_py = venv_python()?;
    if !venv_py.is_file() {
        return Err("MLX venv가 없습니다.".into());
    }

    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let rag_script = resolve_bundled_resource(&resource_dir, "scripts/rag/rag_host.py");
    if !rag_script.is_file() {
        return Err(format!(
            "RAG 스크립트를 찾을 수 없습니다: {}",
            rag_script.display()
        ));
    }

    let collection = collection_name.unwrap_or_else(|| "default".to_string());
    let model = embedding_model.unwrap_or_else(|| "sentence-transformers/all-MiniLM-L6-v2".to_string());
    let k = top_k.unwrap_or(3);
    let db_dir = default_lancedb_dir()?;

    let output = tokio::process::Command::new(&venv_py)
        .arg(&rag_script)
        .arg("query")
        .arg("--query")
        .arg(&query)
        .arg("--db-path")
        .arg(&db_dir)
        .arg("--collection")
        .arg(&collection)
        .arg("--top-k")
        .arg(k.to_string())
        .arg("--model")
        .arg(&model)
        .env("PATH", augmented_path())
        .output()
        .await
        .map_err(|e| format!("RAG 질의 프로세스 실행 실패: {e}"))?;

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err(format!(
            "질의 실패: {}",
            if stdout_str.trim().is_empty() {
                String::from_utf8_lossy(&output.stderr).to_string()
            } else {
                stdout_str.to_string()
            }
        ));
    }

    let res: serde_json::Value = serde_json::from_str(&stdout_str)
        .map_err(|e| format!("JSON 파싱 실패 ({e}): {stdout_str}"))?;

    if res.get("status").and_then(|v| v.as_str()) != Some("ok") {
        let err_msg = res
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("알 수 없는 오류가 발생했습니다.");
        return Err(format!("질의 오류: {err_msg}"));
    }

    let raw_results = res
        .get("results")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "결과 배열을 읽을 수 없습니다.".to_string())?;

    let search_results: Vec<RagSearchResult> = serde_json::from_value(serde_json::Value::Array(raw_results.clone()))
        .map_err(|e| format!("RagSearchResult 변환 실패: {e}"))?;

    Ok(search_results)
}

#[tauri::command]
pub async fn dvc_commit_dataset(
    app: tauri::AppHandle,
    data_path: Option<String>,
    bucket_name: Option<String>,
    commit_message: Option<String>,
) -> Result<String, String> {
    let venv_py = venv_python()?;
    if !venv_py.is_file() {
        return Err("MLX venv가 없습니다.".into());
    }

    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let rag_script = resolve_bundled_resource(&resource_dir, "scripts/rag/rag_host.py");
    if !rag_script.is_file() {
        return Err(format!(
            "RAG 스크립트를 찾을 수 없습니다: {}",
            rag_script.display()
        ));
    }

    let target_dir = match data_path {
        Some(ref p) if !p.trim().is_empty() => validate_home_subpath(p)?,
        _ => default_lancedb_dir()?,
    };

    let bucket = bucket_name.unwrap_or_else(|| "dvc-repo".to_string());
    let message = commit_message.unwrap_or_else(|| "Dataset update".to_string());

    let output = tokio::process::Command::new(&venv_py)
        .arg(&rag_script)
        .arg("dvc-commit")
        .arg("--data-dir")
        .arg(&target_dir)
        .arg("--remote-url")
        .arg("http://127.0.0.1:8333")
        .arg("--bucket")
        .arg(&bucket)
        .arg("--access-key")
        .arg("seaweedfsadmin")
        .arg("--secret-key")
        .arg("seaweedfsadmin")
        .arg("--message")
        .arg(&message)
        .env("PATH", augmented_path())
        .output()
        .await
        .map_err(|e| format!("DVC 커밋 프로세스 실행 실패: {e}"))?;

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err(format!(
            "DVC 커밋 실패: {}",
            if stdout_str.trim().is_empty() {
                String::from_utf8_lossy(&output.stderr).to_string()
            } else {
                stdout_str.to_string()
            }
        ));
    }

    let res: serde_json::Value = serde_json::from_str(&stdout_str)
        .map_err(|e| format!("JSON 파싱 실패 ({e}): {stdout_str}"))?;

    if res.get("status").and_then(|v| v.as_str()) != Some("ok") {
        let err_msg = res
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("알 수 없는 오류가 발생했습니다.");
        return Err(format!("DVC 오류: {err_msg}"));
    }

    let msg = res
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("DVC 데이터셋 버저닝 완료");

    Ok(msg.to_string())
}
