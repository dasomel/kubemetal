use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::services::process::external_command;

/// Hugging Face `/api/models?search=...` 실측 스키마(2026-07-21, `Qwen/Qwen3-*` 대상 확인)
/// 응답에는 `_id`, `tags`, `library_name` 등 다른 필드도 있으나 여기서 쓰는 필드만 매핑한다.
#[derive(Debug, Deserialize, Serialize)]
pub struct HfModel {
    pub id: String,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub likes: u64,
    pub pipeline_tag: Option<String>,
    /// 모델 용량(바이트) 추정치 — `safetensors.parameters`의 dtype별 텐서 원소 수 ×
    /// dtype 바이트폭을 합산해 계산한다(아래 `estimate_size_bytes` 참고). safetensors 형식이
    /// 아니거나(GGUF 전용 등) HF가 메타데이터를 못 주는 리포지토리는 `None`.
    pub size_bytes: Option<u64>,
}

/// `expand[]=safetensors` 실측(2026-07-21, `mlx-community/Qwen2.5-7B-Instruct-4bit` 대상):
/// `parameters`는 dtype -> 텐서 원소 수 맵이다. 실제 `model.safetensors` 파일 크기(블롭 API로
/// 대조 확인, 약 4.30GB)와 이 필드로 계산한 바이트 합계가 거의 일치했다(F16 2바이트 +
/// U32 4바이트 가중합 ≈ 4.284GB) — MLX 4bit 양자화 텐서가 U32에 패킹되어 있어도 바이트
/// 단위 합산은 실제 파일 크기를 정확히 반영한다.
#[derive(Debug, Deserialize)]
struct HfSafetensorsInfo {
    #[serde(default)]
    parameters: HashMap<String, u64>,
}

#[derive(Debug, Deserialize)]
struct HfModelRaw {
    id: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    likes: u64,
    pipeline_tag: Option<String>,
    safetensors: Option<HfSafetensorsInfo>,
}

/// safetensors dtype 태그의 바이트 폭. 알려지지 않은 태그는 0으로 처리해 과대평가보다
/// 과소평가(용량 표시 누락 대신 실제보다 작게)를 택한다.
fn dtype_byte_width(dtype: &str) -> u64 {
    match dtype {
        "F64" | "I64" | "U64" => 8,
        "F32" | "I32" | "U32" => 4,
        "F16" | "BF16" | "I16" | "U16" => 2,
        "I8" | "U8" | "BOOL" | "F8_E4M3" | "F8_E5M2" => 1,
        _ => 0,
    }
}

fn estimate_size_bytes(info: &HfSafetensorsInfo) -> Option<u64> {
    if info.parameters.is_empty() {
        return None;
    }
    Some(
        info.parameters
            .iter()
            .map(|(dtype, count)| count * dtype_byte_width(dtype))
            .sum(),
    )
}

impl From<HfModelRaw> for HfModel {
    fn from(raw: HfModelRaw) -> Self {
        let size_bytes = raw.safetensors.as_ref().and_then(estimate_size_bytes);
        HfModel {
            id: raw.id,
            downloads: raw.downloads,
            likes: raw.likes,
            pipeline_tag: raw.pipeline_tag,
            size_bytes,
        }
    }
}

/// `/api/models/{repo_id}/tree/main?recursive=true` 실측 스키마 중 필요한 필드만 매핑.
/// `type`은 "file" | "directory"이며, 디렉터리 항목은 다운로드 대상에서 제외한다.
#[derive(Debug, Deserialize)]
struct HfTreeEntry {
    #[serde(rename = "type")]
    entry_type: String,
    path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadStatus {
    pub repo_id: String,
    pub total_files: u32,
    pub done_files: u32,
    pub state: String, // "downloading" | "done" | "error"
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LocalModel {
    pub repo_id: String,
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct RegisteredModel {
    pub name: String,
    pub latest_version: Option<String>,
    pub last_updated_ms: Option<i64>,
}

/// `/api/2.0/mlflow/registered-models/search` 실측 스키마(2026-07-21, MLflow 3.x,
/// `localhost:5001` 포트포워딩 경유 확인). 모델이 없으면 `registered_models` 필드 자체가
/// 응답 JSON에서 생략되므로(`{}`) `#[serde(default)]`로 빈 벡터 취급한다.
#[derive(Debug, Deserialize)]
struct MlflowSearchResponse {
    #[serde(default)]
    registered_models: Vec<MlflowRegisteredModel>,
}

#[derive(Debug, Deserialize)]
struct MlflowRegisteredModel {
    name: String,
    #[serde(default)]
    last_updated_timestamp: Option<i64>,
    #[serde(default)]
    latest_versions: Vec<MlflowModelVersion>,
}

#[derive(Debug, Deserialize)]
struct MlflowModelVersion {
    version: String,
}

#[derive(Default)]
pub struct ModelHubState(pub Mutex<HashMap<String, DownloadStatus>>);

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// `owner/name` 형태의 repo_id를 로컬 저장 경로 / 오브젝트 스토리지 키에 쓸 슬러그로 변환한다.
fn slug(repo_id: &str) -> String {
    repo_id.replace('/', "__")
}

/// repo_id는 `owner/name` 정확히 두 세그먼트여야 하고 각 세그먼트는 `[A-Za-z0-9._-]+`만 허용한다
/// (`^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$`와 동치). 원격에서 온 값이 아니라 사용자가 UI에서
/// 선택한 검색 결과의 `id`이지만, 이후 로컬 경로/URL/스토리지 키 조립에 그대로 쓰이므로
/// 슬러그 변환 이전에 형식을 고정해 경로 탈출 소스를 원천 차단한다.
fn validate_repo_id(repo_id: &str) -> Result<(), String> {
    let is_valid_segment = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
            // "." / ".." 같은 점(dot)만으로 구성된 세그먼트는 문자 클래스상 정규식을 통과하지만
            // traversal 성분(CurDir/ParentDir)과 동일한 의미를 가지므로 별도로 거부한다.
            && s.chars().any(|c| c != '.')
    };
    let parts: Vec<&str> = repo_id.split('/').collect();
    if parts.len() == 2 && is_valid_segment(parts[0]) && is_valid_segment(parts[1]) {
        Ok(())
    } else {
        Err(format!("잘못된 repo_id 형식입니다: {repo_id}"))
    }
}

/// HF tree API의 `file.path`나 로컬 스캔 결과처럼 신뢰할 수 없는 상대 경로 문자열(`p`)을
/// 검증한다. 절대경로는 거부하고, 각 `Component`가 `Normal`이 아니면(ParentDir/CurDir/
/// RootDir/Prefix) 즉시 Err. 호출부는 반환된 `PathBuf`를 `base.join(&rel)`한 뒤 반드시
/// `starts_with(base)`로 재확인해야 한다(이중 방어 — 이 함수 자체도 방어선이지만 호출부의
/// 명시적 재검증이 마지막 방어선이다).
fn safe_rel(p: &str) -> Result<PathBuf, String> {
    let candidate = Path::new(p);
    if candidate.is_absolute() {
        return Err(format!("절대경로는 허용되지 않습니다: {p}"));
    }
    let mut rel = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => rel.push(part),
            _ => return Err(format!("허용되지 않은 경로 성분입니다: {p}")),
        }
    }
    if rel.as_os_str().is_empty() {
        return Err(format!("빈 경로는 허용되지 않습니다: {p}"));
    }
    Ok(rel)
}

/// `safe_rel`로 검증된 상대 경로의 각 세그먼트를 percent-encode해 URL 경로로 조립한다.
fn rel_to_url_path(rel: &Path) -> String {
    rel.iter()
        .map(|part| percent_encode(&part.to_string_lossy()))
        .collect::<Vec<_>>()
        .join("/")
}

fn models_root() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME 환경변수를 찾을 수 없습니다.".to_string())?;
    Ok(PathBuf::from(home).join(".kubemetal").join("models"))
}

fn dir_size(dir: &Path) -> std::io::Result<u64> {
    let mut total = 0u64;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            total += dir_size(&entry.path())?;
        } else {
            total += entry.metadata()?.len();
        }
    }
    Ok(total)
}

fn collect_files(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_files(base, &path, out)?;
        } else {
            out.push(path.strip_prefix(base).unwrap().to_path_buf());
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn search_hf_models(
    query: String,
    limit: u32,
    author: Option<String>,
) -> Result<Vec<HfModel>, String> {
    // expand[] 파라미터를 하나라도 지정하면 HF API가 기본 필드 세트를 내려주지 않고
    // 지정한 필드로 완전히 대체한다(2026-07-21 실측) — 그래서 원래 기본 응답에 있던
    // downloads/likes/pipeline_tag까지 전부 명시해야 하고, 여기에 용량 계산용
    // safetensors도 함께 추가한다.
    let mut url = format!(
        "https://huggingface.co/api/models?search={}&limit={}&sort=downloads\
         &expand%5B%5D=downloads&expand%5B%5D=likes&expand%5B%5D=pipeline_tag\
         &expand%5B%5D=safetensors",
        percent_encode(&query),
        limit
    );
    // 빈 query("")도 HF API가 그대로 받아들이며(2026-07-21 실측), author만으로 필터링된
    // 인기 모델 목록을 반환한다 — "인기 MLX 모델" 자동 로드에 사용.
    if let Some(author) = author.filter(|a| !a.is_empty()) {
        url.push_str(&format!("&author={}", percent_encode(&author)));
    }
    let output = external_command("curl")?
        .args(["-sL", &url])
        .output()
        .await
        .map_err(|e| format!("curl 실행 실패: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "Hugging Face 검색 실패: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let raw: Vec<HfModelRaw> = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Hugging Face 응답 파싱 실패: {e}"))?;
    Ok(raw.into_iter().map(HfModel::from).collect())
}

fn update_status(app: &tauri::AppHandle, repo_id: &str, f: impl FnOnce(&mut DownloadStatus)) {
    let state = app.state::<ModelHubState>();
    let mut guard = match state.0.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    if let Some(status) = guard.get_mut(repo_id) {
        f(status);
    }
}

async fn run_download_inner(app: &tauri::AppHandle, repo_id: &str) -> Result<(), String> {
    validate_repo_id(repo_id)?;
    let tree_url =
        format!("https://huggingface.co/api/models/{repo_id}/tree/main?recursive=true");
    let output = external_command("curl")?
        .args(["-sL", &tree_url])
        .output()
        .await
        .map_err(|e| format!("curl 실행 실패: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "파일 목록 조회 실패: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let entries: Vec<HfTreeEntry> = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("파일 목록 파싱 실패: {e}"))?;
    let files: Vec<HfTreeEntry> = entries
        .into_iter()
        .filter(|e| e.entry_type == "file")
        .collect();

    update_status(app, repo_id, |s| {
        s.total_files = files.len() as u32;
    });

    let dir = models_root()?.join(slug(repo_id));

    for file in &files {
        let rel = safe_rel(&file.path)?;
        let dest = dir.join(&rel);
        if !dest.starts_with(&dir) {
            return Err(format!("경로 탈출이 감지되었습니다: {}", file.path));
        }
        let file_url = format!(
            "https://huggingface.co/{repo_id}/resolve/main/{}",
            rel_to_url_path(&rel)
        );
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("디렉터리 생성 실패: {e}"))?;
        }
        let out = external_command("curl")?
            .arg("-sfL")
            .arg("-o")
            .arg(&dest)
            .arg(&file_url)
            .output()
            .await
            .map_err(|e| format!("curl 실행 실패: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "{} 다운로드 실패: {}",
                file.path,
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        update_status(app, repo_id, |s| {
            s.done_files += 1;
        });
    }

    update_status(app, repo_id, |s| {
        s.state = "done".into();
    });
    Ok(())
}

async fn run_download(app: tauri::AppHandle, repo_id: String) {
    if let Err(e) = run_download_inner(&app, &repo_id).await {
        update_status(&app, &repo_id, |s| {
            s.state = "error".into();
            s.error = Some(e);
        });
    }
}

#[tauri::command]
pub async fn download_hf_model(
    app: tauri::AppHandle,
    state: State<'_, ModelHubState>,
    repo_id: String,
) -> Result<String, String> {
    validate_repo_id(&repo_id)?;
    {
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        if let Some(existing) = guard.get(&repo_id) {
            if existing.state == "downloading" {
                return Err(format!("{repo_id} 다운로드가 이미 진행 중입니다."));
            }
        }
        guard.insert(
            repo_id.clone(),
            DownloadStatus {
                repo_id: repo_id.clone(),
                total_files: 0,
                done_files: 0,
                state: "downloading".into(),
                error: None,
            },
        );
    }

    tokio::spawn(run_download(app, repo_id.clone()));

    Ok(format!("{repo_id} 다운로드를 시작했습니다."))
}

#[tauri::command]
pub async fn get_model_downloads(
    state: State<'_, ModelHubState>,
) -> Result<Vec<DownloadStatus>, String> {
    let guard = state.0.lock().map_err(|e| e.to_string())?;
    Ok(guard.values().cloned().collect())
}

#[tauri::command]
pub fn list_local_models() -> Result<Vec<LocalModel>, String> {
    let root = models_root()?;
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let entries = std::fs::read_dir(&root).map_err(|e| format!("모델 디렉터리 조회 실패: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            let dir_name = entry.file_name().to_string_lossy().to_string();
            let size_bytes = dir_size(&entry.path()).unwrap_or(0);
            result.push(LocalModel {
                repo_id: dir_name.replace("__", "/"),
                path: entry.path().to_string_lossy().to_string(),
                size_bytes,
            });
        }
    }
    Ok(result)
}

#[tauri::command]
pub async fn upload_model_to_storage(repo_id: String) -> Result<String, String> {
    validate_repo_id(&repo_id)?;
    let repo_slug = slug(&repo_id);
    let dir = models_root()?.join(&repo_slug);
    if !dir.is_dir() {
        return Err(format!(
            "{repo_id} 로컬 모델 디렉터리를 찾을 수 없습니다: {}",
            dir.display()
        ));
    }

    let create_bucket = external_command("curl")?
        .args([
            "-s",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            "-X",
            "PUT",
            "http://localhost:8333/models",
        ])
        .output()
        .await
        .map_err(|e| format!("SeaweedFS(8333) 연결 실패 — 포트포워딩이 활성화되어 있는지 확인하세요: {e}"))?;
    let bucket_code = String::from_utf8_lossy(&create_bucket.stdout).to_string();
    if !(bucket_code.starts_with('2') || bucket_code == "409") {
        return Err(format!(
            "SeaweedFS 버킷 생성 실패(HTTP {bucket_code}) — 포트포워딩(8333)이 활성화되어 있는지 확인하세요."
        ));
    }

    let files = list_repo_files(&dir).map_err(|e| format!("로컬 파일 목록 조회 실패: {e}"))?;
    if files.is_empty() {
        return Err(format!("{repo_id}: 업로드할 로컬 파일이 없습니다."));
    }

    for rel in &files {
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        // 로컬 디렉터리 스캔이라 위험도는 낮지만, S3 키로 쓰기 전 동일한 safe_rel 규칙으로
        // 정규화/재검증한다(이중 방어 원칙을 업로드 경로에도 동일 적용).
        let safe_rel_path = safe_rel(&rel_str)?;
        let abs = dir.join(&safe_rel_path);
        if !abs.starts_with(&dir) {
            return Err(format!("경로 탈출이 감지되었습니다: {rel_str}"));
        }
        let url = format!(
            "http://localhost:8333/models/{repo_slug}/{}",
            rel_to_url_path(&safe_rel_path)
        );
        let out = external_command("curl")?
            .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "-X", "PUT", "--data-binary"])
            .arg(format!("@{}", abs.display()))
            .arg(&url)
            .output()
            .await
            .map_err(|e| format!("curl 실행 실패({rel_str}): {e}"))?;
        let code = String::from_utf8_lossy(&out.stdout).to_string();
        if !code.starts_with('2') {
            return Err(format!("{rel_str} 업로드 실패(HTTP {code})"));
        }
    }

    Ok(format!(
        "{repo_id} ({} 파일)가 SeaweedFS models 버킷에 업로드되었습니다.",
        files.len()
    ))
}

fn list_repo_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    collect_files(dir, dir, &mut out)?;
    Ok(out)
}

#[tauri::command]
pub async fn register_model_mlflow(repo_id: String) -> Result<String, String> {
    validate_repo_id(&repo_id)?;
    let repo_slug = slug(&repo_id);

    let create_body = serde_json::json!({ "name": repo_slug }).to_string();
    let create_out = external_command("curl")?
        .args([
            "-s",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-d",
            &create_body,
            "http://localhost:5001/api/2.0/mlflow/registered-models/create",
        ])
        .output()
        .await
        .map_err(|e| format!("MLflow(5001) 연결 실패 — 포트포워딩이 활성화되어 있는지 확인하세요: {e}"))?;

    let create_json: serde_json::Value =
        serde_json::from_slice(&create_out.stdout).unwrap_or(serde_json::json!({}));
    if let Some(code) = create_json.get("error_code").and_then(|v| v.as_str()) {
        if code != "RESOURCE_ALREADY_EXISTS" {
            return Err(format!(
                "MLflow 모델 등록 실패: {}",
                create_json
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or(code)
            ));
        }
    }

    let source = format!("s3://models/{repo_slug}");
    let version_body = serde_json::json!({ "name": repo_slug, "source": source }).to_string();
    let version_out = external_command("curl")?
        .args([
            "-s",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-d",
            &version_body,
            "http://localhost:5001/api/2.0/mlflow/model-versions/create",
        ])
        .output()
        .await
        .map_err(|e| format!("MLflow(5001) 연결 실패: {e}"))?;

    let version_json: serde_json::Value = serde_json::from_slice(&version_out.stdout)
        .map_err(|e| format!("MLflow 응답 파싱 실패: {e}"))?;

    if let Some(code) = version_json.get("error_code").and_then(|v| v.as_str()) {
        return Err(format!(
            "MLflow 모델 버전 등록 실패: {}",
            version_json
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or(code)
        ));
    }

    let version = version_json["model_version"]["version"]
        .as_str()
        .ok_or_else(|| "MLflow 응답에 version 필드가 없습니다.".to_string())?;

    Ok(format!(
        "{repo_id} → MLflow Model Registry에 {repo_slug} v{version}으로 등록되었습니다."
    ))
}

#[tauri::command]
pub async fn list_registered_models() -> Result<Vec<RegisteredModel>, String> {
    let output = external_command("curl")?
        .args([
            "-s",
            "http://localhost:5001/api/2.0/mlflow/registered-models/search",
        ])
        .output()
        .await
        .map_err(|e| format!("MLflow(5001) 연결 실패 — 포트포워딩이 활성화되어 있는지 확인하세요: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "MLflow 등록 모델 조회 실패: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let parsed: MlflowSearchResponse = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("MLflow 응답 파싱 실패: {e}"))?;

    Ok(parsed
        .registered_models
        .into_iter()
        .map(|m| RegisteredModel {
            name: m.name,
            latest_version: m.latest_versions.first().map(|v| v.version.clone()),
            last_updated_ms: m.last_updated_timestamp,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dtype_byte_width_covers_known_safetensors_dtypes() {
        assert_eq!(dtype_byte_width("F16"), 2);
        assert_eq!(dtype_byte_width("BF16"), 2);
        assert_eq!(dtype_byte_width("F32"), 4);
        assert_eq!(dtype_byte_width("U32"), 4);
        assert_eq!(dtype_byte_width("U8"), 1);
        assert_eq!(dtype_byte_width("UNKNOWN_DTYPE"), 0);
    }

    #[test]
    fn estimate_size_bytes_matches_real_file_size_within_tolerance() {
        // 실측(2026-07-21): mlx-community/Qwen2.5-7B-Instruct-4bit의 safetensors 원소 수
        // (F16 238310912 + U32 951910400) — 실제 model.safetensors 파일 크기(blobs=true로
        // 대조 확인)는 4,284,346,255바이트였다. 가중합이 그 값과 거의 일치해야 한다.
        let mut parameters = HashMap::new();
        parameters.insert("F16".to_string(), 238_310_912u64);
        parameters.insert("U32".to_string(), 951_910_400u64);
        let info = HfSafetensorsInfo { parameters };

        let estimated = estimate_size_bytes(&info).expect("safetensors 정보가 있으므로 Some이어야 함");
        let actual_file_size = 4_284_346_255u64;
        let diff = actual_file_size.abs_diff(estimated);
        // 0.1% 이내 오차만 허용 — 텐서 원소 수 x dtype 바이트폭 가중합이 실제 파일 크기의
        // 근사치임을 보장한다.
        assert!(
            diff * 1000 < actual_file_size,
            "추정치 {estimated}가 실제 크기 {actual_file_size}와 너무 차이남(diff={diff})"
        );
    }

    #[test]
    fn estimate_size_bytes_returns_none_for_empty_parameters() {
        let info = HfSafetensorsInfo {
            parameters: HashMap::new(),
        };
        assert!(estimate_size_bytes(&info).is_none());
    }

    #[test]
    fn safe_rel_rejects_parent_dir_traversal() {
        assert!(safe_rel("../evil").is_err());
    }

    #[test]
    fn safe_rel_rejects_absolute_path() {
        assert!(safe_rel("/abs").is_err());
    }

    #[test]
    fn safe_rel_rejects_mixed_traversal() {
        assert!(safe_rel("a/../../b").is_err());
    }

    #[test]
    fn safe_rel_accepts_normal_relative_path() {
        let rel = safe_rel("normal/file.txt").unwrap();
        assert_eq!(rel, PathBuf::from("normal").join("file.txt"));

        let base = Path::new("/tmp/kubemetal-test/base");
        let joined = base.join(&rel);
        assert!(joined.starts_with(base));
    }

    #[test]
    fn safe_rel_join_never_escapes_base_for_malicious_inputs() {
        let base = Path::new("/tmp/kubemetal-test/base");
        for malicious in ["../evil", "/abs", "a/../../b", "../../etc/passwd"] {
            // safe_rel 자체가 거부하지 못하더라도(이론상), 호출부의 starts_with 재검증이
            // base 밖 쓰기를 반드시 막아야 한다는 이중 방어 계약을 재확인한다.
            match safe_rel(malicious) {
                Err(_) => {} // 기대 경로: 1차 방어에서 거부
                Ok(rel) => assert!(base.join(&rel).starts_with(base)),
            }
        }
    }

    #[test]
    fn validate_repo_id_accepts_owner_slash_name() {
        assert!(validate_repo_id("Qwen/Qwen3-0.6B").is_ok());
    }

    #[test]
    fn validate_repo_id_rejects_traversal_and_malformed() {
        assert!(validate_repo_id("../evil").is_err());
        assert!(validate_repo_id("/abs/path").is_err());
        assert!(validate_repo_id("owner/name/extra").is_err());
        assert!(validate_repo_id("owner").is_err());
        assert!(validate_repo_id("owner/..").is_err());
    }
}
