use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::commands::access::resolve_s3_credentials;
use crate::commands::mlx::{
    home_dir, validate_home_subpath, venv_pip, venv_python, EnvSetupStatus,
};
use crate::services::process::{augmented_path, external_command, resolve_bundled_resource};

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

/// `.dvc/config`에서 `endpointurl` 값을 읽는다(`dvc remote modify <name> endpointurl <url>`로
/// 기록된 형식, ingest_host.py/rag_host.py의 dvc-commit 경로가 남기는 실제 파일). 없거나
/// 파싱 불가면 `None` — 호출부는 이를 "조회 불가"로 정직하게 리턴해야 한다.
fn parse_dvc_remote_url(config_path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(config_path).ok()?;
    for line in text.lines() {
        let trimmed = line.trim();
        let rest = trimmed.strip_prefix("endpointurl")?;
        if let Some(val) = rest.trim_start().strip_prefix('=') {
            let val = val.trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

/// `ingest_host.py`/`rag_host.py`의 `dvc init`은 항상 `--no-scm`(git 저장소 미생성)으로
/// 실행되므로, 실제 파이프라인에서는 git 태그가 존재하지 않는 것이 정상이다. `git` 실행
/// 실패·비-git 디렉터리 모두 빈 벡터로 수렴시켜 "조회 불가 = 정직하게 빈 값" 규칙을 따른다.
async fn fetch_git_tags(dir: &Path) -> Vec<DvcVersionTag> {
    let Ok(mut cmd) = external_command("git") else {
        return Vec::new();
    };
    let output = cmd
        .arg("-C")
        .arg(dir)
        .args([
            "for-each-ref",
            "refs/tags",
            "--sort=-creatordate",
            "--format=%(refname:short)%09%(objectname:short)%09%(subject)%09%(creatordate:iso-strict)",
        ])
        .output()
        .await;
    let Ok(out) = output else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\t');
            let tag = parts.next()?.to_string();
            let commit_hash = parts.next()?.to_string();
            let message = parts.next().unwrap_or("").to_string();
            let created_at = parts.next().map(|s| s.to_string());
            Some(DvcVersionTag {
                tag,
                commit_hash,
                message,
                created_at,
                dataset_path: None,
            })
        })
        .collect()
}

#[tauri::command]
pub async fn get_dvc_status() -> Result<DvcStatus, String> {
    let Ok(db_dir) = default_lancedb_dir() else {
        return Ok(DvcStatus {
            initialized: false,
            remote_url: None,
            current_tag: None,
            dataset_path: None,
            tags: Vec::new(),
            last_error: Some("Cannot resolve default LanceDB path.".into()),
        });
    };
    let dataset_path = Some(db_dir.to_string_lossy().to_string());

    let dvc_dir = db_dir.join(".dvc");
    if !dvc_dir.is_dir() {
        return Ok(DvcStatus {
            initialized: false,
            remote_url: None,
            current_tag: None,
            dataset_path,
            tags: Vec::new(),
            last_error: Some(
                "DVC is not initialized yet — run dvc_commit_dataset first.".into(),
            ),
        });
    }

    let remote_url = parse_dvc_remote_url(&dvc_dir.join("config"));
    let tags = fetch_git_tags(&db_dir).await;
    let current_tag = tags.first().map(|t| t.tag.clone());

    Ok(DvcStatus {
        initialized: true,
        remote_url,
        current_tag,
        dataset_path,
        tags,
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
        return Err("MLX venv not found. Run setup_mlx_env first.".into());
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
        .map_err(|e| format!("pip install execution failed: {e}"))?;

    if !out.status.success() {
        return Err(format!(
            "RAG package installation failed: {}",
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
            return Err("RAG environment setup is already in progress.".into());
        }
        guard.state = "installing".into();
        guard.error = None;
    }

    tokio::spawn(run_rag_env_setup(app));
    Ok("Started RAG environment (LanceDB, sentence-transformers, DVC) setup.".into())
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
        return Err("MLX venv not found. Run setup_mlx_env and setup_rag_env.".into());
    }

    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let rag_script = resolve_bundled_resource(&resource_dir, "scripts/rag/rag_host.py");
    if !rag_script.is_file() {
        return Err(format!(
            "RAG script not found: {}",
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
        .map_err(|e| format!("Failed to run RAG indexing process: {e}"))?;

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err(format!(
            "Indexing failed: {}",
            if stdout_str.trim().is_empty() {
                String::from_utf8_lossy(&output.stderr).to_string()
            } else {
                stdout_str.to_string()
            }
        ));
    }

    let res: serde_json::Value = serde_json::from_str(&stdout_str)
        .map_err(|e| format!("JSON parse failed ({e}): {stdout_str}"))?;

    if res.get("status").and_then(|v| v.as_str()) != Some("ok") {
        let err_msg = res
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("An unknown error occurred.");
        return Err(format!("Indexing error: {err_msg}"));
    }

    let result: IndexResult = serde_json::from_value(res)
        .map_err(|e| format!("Failed to convert to IndexResult struct: {e}"))?;

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
        return Err("Query text cannot be empty.".into());
    }

    let venv_py = venv_python()?;
    if !venv_py.is_file() {
        return Err("MLX venv not found.".into());
    }

    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let rag_script = resolve_bundled_resource(&resource_dir, "scripts/rag/rag_host.py");
    if !rag_script.is_file() {
        return Err(format!(
            "RAG script not found: {}",
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
        .map_err(|e| format!("Failed to run RAG query process: {e}"))?;

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err(format!(
            "Query failed: {}",
            if stdout_str.trim().is_empty() {
                String::from_utf8_lossy(&output.stderr).to_string()
            } else {
                stdout_str.to_string()
            }
        ));
    }

    let res: serde_json::Value = serde_json::from_str(&stdout_str)
        .map_err(|e| format!("JSON parse failed ({e}): {stdout_str}"))?;

    if res.get("status").and_then(|v| v.as_str()) != Some("ok") {
        let err_msg = res
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("An unknown error occurred.");
        return Err(format!("Query error: {err_msg}"));
    }

    let raw_results = res
        .get("results")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Cannot read results array.".to_string())?;

    let search_results: Vec<RagSearchResult> = serde_json::from_value(serde_json::Value::Array(raw_results.clone()))
        .map_err(|e| format!("Failed to convert to RagSearchResult: {e}"))?;

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
        return Err("MLX venv not found.".into());
    }

    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let rag_script = resolve_bundled_resource(&resource_dir, "scripts/rag/rag_host.py");
    if !rag_script.is_file() {
        return Err(format!(
            "RAG script not found: {}",
            rag_script.display()
        ));
    }

    let target_dir = match data_path {
        Some(ref p) if !p.trim().is_empty() => validate_home_subpath(p)?,
        _ => default_lancedb_dir()?,
    };

    let bucket = bucket_name.unwrap_or_else(|| "dvc-repo".to_string());
    let message = commit_message.unwrap_or_else(|| "Dataset update".to_string());
    // D13/D21: 크리덴셜은 CLI 인자(ps로 노출됨)가 아니라 env var로 자식 프로세스에 주입한다.
    let (s3_access_key, s3_secret_key) = resolve_s3_credentials().await;

    let output = tokio::process::Command::new(&venv_py)
        .arg(&rag_script)
        .arg("dvc-commit")
        .arg("--data-dir")
        .arg(&target_dir)
        .arg("--remote-url")
        .arg("http://127.0.0.1:8333")
        .arg("--bucket")
        .arg(&bucket)
        .arg("--message")
        .arg(&message)
        .env("PATH", augmented_path())
        .env("KUBEMETAL_S3_ACCESS_KEY", &s3_access_key)
        .env("KUBEMETAL_S3_SECRET_KEY", &s3_secret_key)
        .output()
        .await
        .map_err(|e| format!("Failed to run DVC commit process: {e}"))?;

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err(format!(
            "DVC commit failed: {}",
            if stdout_str.trim().is_empty() {
                String::from_utf8_lossy(&output.stderr).to_string()
            } else {
                stdout_str.to_string()
            }
        ));
    }

    let res: serde_json::Value = serde_json::from_str(&stdout_str)
        .map_err(|e| format!("JSON parse failed ({e}): {stdout_str}"))?;

    if res.get("status").and_then(|v| v.as_str()) != Some("ok") {
        let err_msg = res
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("An unknown error occurred.");
        return Err(format!("DVC error: {err_msg}"));
    }

    let msg = res
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("DVC dataset versioning complete");

    Ok(msg.to_string())
}
