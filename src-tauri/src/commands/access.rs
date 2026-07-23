use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::Serialize;
use tauri::State;

use crate::commands::mlx::MlxState;
use crate::services::process::external_command;

#[derive(Debug, Clone, Serialize)]
pub struct CredentialItem {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceAccess {
    pub service: String,
    pub url: String,
    pub health: String, // "ok" | "unreachable"
    pub credential_hint: Option<String>,
    pub credentials: Vec<CredentialItem>,
}

/// `curl -w %{http_code}`로 헬스를 판정한다. 연결 자체가 실패하면 curl은 "000"을 쓴다
/// (FR-09 범위에서는 TCP 응답이 오면 상태 코드와 무관하게 "ok"로 취급 — 서비스가 살아있다는
/// 신호로 충분하며, 라우트별 200 여부까지 검증하지는 않는다).
async fn check_health(url: &str) -> String {
    let mut cmd = match external_command("curl") {
        Ok(c) => c,
        Err(_) => return "unreachable".into(),
    };
    let output = cmd
        .args(["-s", "-o", "/dev/null", "-m", "2", "-w", "%{http_code}", url])
        .output()
        .await;
    let code = match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout).to_string(),
        Err(_) => return "unreachable".into(),
    };
    if code == "000" || code.is_empty() {
        "unreachable".into()
    } else {
        "ok".into()
    }
}

/// 모델 서빙 전용 헬스: 8080은 무관한 로컬 프로세스가 선점할 수 있어(실측: Tomcat 404)
/// "TCP 응답 = ok" 판정이 오탐을 낸다. OpenAI 호환 `/v1/models`가 HTTP 200일 때만 ok.
async fn check_serving_health(base_url: &str) -> String {
    let mut cmd = match external_command("curl") {
        Ok(c) => c,
        Err(_) => return "unreachable".into(),
    };
    let url = format!("{base_url}/models");
    let output = cmd
        .args(["-s", "-o", "/dev/null", "-m", "2", "-w", "%{http_code}", &url])
        .output()
        .await;
    match output {
        Ok(out) if String::from_utf8_lossy(&out.stdout) == "200" => "ok".into(),
        _ => "unreachable".into(),
    }
}

/// SeaweedFS S3 크리덴셜은 `seaweedfs-s3-credentials` Secret(default 네임스페이스,
/// `scripts/k8s/seaweedfs-s3-credentials.yaml`로 프로비저닝됨)에서 조회한다.
/// `kubectl get secret -o json`의 `data` 필드는 K8s가 항상 base64 인코딩해 반환하므로
/// (매니페스트가 `stringData`를 쓰더라도) 여기서 디코드가 필요하다.
pub(crate) async fn fetch_seaweedfs_credentials() -> (Vec<CredentialItem>, Option<String>) {
    let mut cmd = match external_command("kubectl") {
        Ok(c) => c,
        Err(e) => return (Vec::new(), Some(format!("kubectl 실행 파일을 찾을 수 없습니다: {e}"))),
    };
    let output = match cmd
        .args([
            "--context",
            "colima",
            "get",
            "secret",
            "seaweedfs-s3-credentials",
            "-n",
            "default",
            "-o",
            "json",
        ])
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => return (Vec::new(), Some(format!("kubectl 실행 실패: {e}"))),
    };
    if !output.status.success() {
        return (
            Vec::new(),
            Some(format!(
                "SeaweedFS 크리덴셜 시크릿 조회 실패 — 대시보드에서 MLOps 스택 프로비저닝이 완료되었는지 확인하세요: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )),
        );
    }

    let json: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => return (Vec::new(), Some(format!("시크릿 응답 파싱 실패: {e}"))),
    };
    let data = match json.get("data").and_then(|d| d.as_object()) {
        Some(d) => d,
        None => return (Vec::new(), Some("시크릿에 data 필드가 없습니다.".into())),
    };

    let mut creds = Vec::new();
    for key in ["access-key-id", "secret-access-key"] {
        if let Some(encoded) = data.get(key).and_then(|v| v.as_str()) {
            if let Ok(bytes) = BASE64.decode(encoded) {
                if let Ok(value) = String::from_utf8(bytes) {
                    creds.push(CredentialItem { key: key.into(), value });
                }
            }
        }
    }

    if creds.is_empty() {
        (Vec::new(), Some("시크릿에서 크리덴셜 값을 읽지 못했습니다.".into()))
    } else {
        (creds, None)
    }
}

/// `data_ingest.rs`/`rag.rs`가 DVC S3 원격 자격증명으로 쓸 (access_key, secret_key)를
/// 리턴한다. `fetch_seaweedfs_credentials`가 시크릿을 읽지 못하면(K8s 미기동/미프로비저닝)
/// `seaweedfs-s3-credentials.yaml`의 `stringData` 기본값(`kubemetal`/`kubemetal-local`)으로
/// 대체한다 — SeaweedFS S3 게이트웨이는 IAM 미설정 시 어떤 자격증명이든 수락하므로 이 값으로도
/// DVC push가 동작한다(D13/D21). CLI 인자 대신 호출부가 env var로 자식 프로세스에 주입한다
/// (`ps`로 프로세스 인자가 노출되는 경로를 피하기 위함).
pub(crate) async fn resolve_s3_credentials() -> (String, String) {
    let (creds, _) = fetch_seaweedfs_credentials().await;
    let mut access_key = "kubemetal".to_string();
    let mut secret_key = "kubemetal-local".to_string();
    for c in creds {
        match c.key.as_str() {
            "access-key-id" => access_key = c.value,
            "secret-access-key" => secret_key = c.value,
            _ => {}
        }
    }
    (access_key, secret_key)
}

#[tauri::command]
pub async fn get_service_access(state: State<'_, MlxState>) -> Result<Vec<ServiceAccess>, String> {
    const MLFLOW_URL: &str = "http://localhost:5001";
    const S3_URL: &str = "http://localhost:8333";
    const FILER_URL: &str = "http://localhost:8888";

    // Model Serving 포트는 고정값이 아니라 실제 기동된 서빙 프로세스의 포트를 따른다
    // (사용자가 서빙 시작 시 임의의 빈 포트를 지정할 수 있으므로).
    let serving_port = {
        let guard = state.serving.lock().map_err(|e| e.to_string())?;
        guard.as_ref().map(|s| s.port)
    };

    let (mlflow_health, s3_health, filer_health, (s3_credentials, s3_hint)) = tokio::join!(
        check_health(MLFLOW_URL),
        check_health(S3_URL),
        check_health(FILER_URL),
        fetch_seaweedfs_credentials(),
    );

    let (serving_url, serving_health, serving_hint) = match serving_port {
        Some(port) => {
            let base_url = format!("http://127.0.0.1:{port}/v1");
            let health = check_serving_health(&base_url).await;
            (
                base_url,
                health,
                Some("OpenAI 호환 API — API 키가 필요하지 않습니다.".to_string()),
            )
        }
        None => (
            String::new(),
            "unreachable".to_string(),
            Some("서빙이 실행 중이 아닙니다 — MLX 스튜디오 탭에서 서빙을 시작하세요.".to_string()),
        ),
    };

    Ok(vec![
        ServiceAccess {
            service: "MLflow".into(),
            url: MLFLOW_URL.into(),
            health: mlflow_health,
            credential_hint: Some("인증이 필요하지 않습니다.".into()),
            credentials: Vec::new(),
        },
        ServiceAccess {
            service: "SeaweedFS S3 API".into(),
            url: S3_URL.into(),
            health: s3_health,
            credential_hint: s3_hint,
            credentials: s3_credentials,
        },
        ServiceAccess {
            service: "SeaweedFS Filer UI".into(),
            url: FILER_URL.into(),
            health: filer_health,
            credential_hint: Some("인증이 필요하지 않습니다.".into()),
            credentials: Vec::new(),
        },
        ServiceAccess {
            service: "Model Serving".into(),
            url: serving_url,
            health: serving_health,
            credential_hint: serving_hint,
            credentials: Vec::new(),
        },
    ])
}
