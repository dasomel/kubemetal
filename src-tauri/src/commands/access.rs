use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::Serialize;

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
async fn fetch_seaweedfs_credentials() -> (Vec<CredentialItem>, Option<String>) {
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

#[tauri::command]
pub async fn get_service_access() -> Result<Vec<ServiceAccess>, String> {
    const MLFLOW_URL: &str = "http://localhost:5001";
    const S3_URL: &str = "http://localhost:8333";
    const FILER_URL: &str = "http://localhost:8888";
    const SERVING_URL: &str = "http://127.0.0.1:8080/v1";

    let (mlflow_health, s3_health, filer_health, serving_health, (s3_credentials, s3_hint)) = tokio::join!(
        check_health(MLFLOW_URL),
        check_health(S3_URL),
        check_health(FILER_URL),
        check_serving_health(SERVING_URL),
        fetch_seaweedfs_credentials(),
    );

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
            url: SERVING_URL.into(),
            health: serving_health,
            credential_hint: Some("OpenAI 호환 API — API 키가 필요하지 않습니다.".into()),
            credentials: Vec::new(),
        },
    ])
}
