use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::Serialize;
use tauri::State;

use crate::commands::mlx::MlxState;
use crate::services::ports;
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

/// `curl -w %{http_code}`로 헬스를 판정한다 — **HTTP 200일 때만** ok다.
/// 예전에는 "연결만 되면 ok"였는데, 그러면 포워드가 죽은 뒤 그 포트를 선점한 무관한
/// 프로세스의 404가 정상으로 보고된다. `check_serving_health`가 같은 이유로 이미 200을
/// 요구하고 있었고, 그 기준을 나머지 서비스에도 맞춘다(D22).
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
    // 200만 ok로 친다. "무엇이든 HTTP로 답하면 ok"였을 때는 포워드가 죽고 그 포트를
    // 선점한 남의 프로세스가 404를 돌려줘도 정상으로 보고했다 — 실측 2026-08-06:
    // Docker 컨테이너 `narwhal-airgap-registry`가 5001을 잡고 있고 MLflow 경로에
    // 404를 답한다. 포트가 아니라 서비스가 살아 있는지를 봐야 한다(D22).
    // 정상 상태의 세 서비스는 모두 200을 반환함을 실측 확인했다.
    if code == "200" {
        "ok".into()
    } else {
        "unreachable".into()
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
        Err(e) => return (Vec::new(), Some(format!("kubectl executable not found: {e}"))),
    };
    let (context, namespace) = crate::services::deploy_target::active_context();
    let output = match cmd
        .args([
            "--context",
            &context,
            "get",
            "secret",
            "seaweedfs-s3-credentials",
            "-n",
            &namespace,
            "-o",
            "json",
        ])
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => return (Vec::new(), Some(format!("kubectl execution failed: {e}"))),
    };
    if !output.status.success() {
        return (
            Vec::new(),
            Some(format!(
                "Failed to fetch SeaweedFS credential secret — verify MLOps stack provisioning completed in the dashboard: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )),
        );
    }

    let json: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => return (Vec::new(), Some(format!("Failed to parse secret response: {e}"))),
    };
    let data = match json.get("data").and_then(|d| d.as_object()) {
        Some(d) => d,
        None => return (Vec::new(), Some("Secret has no data field.".into())),
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
        (Vec::new(), Some("Failed to read credential values from secret.".into()))
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
    // 포워딩이 실제로 잡은 호스트 포트를 따른다 — D1 포트가 점유돼 대체 포트로 밀렸을 수
    // 있고, 그때 여기가 옛 숫자를 보면 화면의 링크와 헬스가 통째로 엉뚱한 곳을 가리킨다.
    // 포워딩 전이면 D1 우선값이 나오므로 기존 동작은 그대로다.
    let mlflow_url = ports::local_url("mlflow");
    let s3_url = ports::local_url("seaweedfs-s3");
    let filer_url = ports::local_url("seaweedfs-filer");

    // Model Serving 포트는 고정값이 아니라 실제 기동된 서빙 프로세스의 포트를 따른다
    // (사용자가 서빙 시작 시 임의의 빈 포트를 지정할 수 있으므로).
    let serving_port = {
        let guard = state.serving.lock().map_err(|e| e.to_string())?;
        guard.as_ref().map(|s| s.port)
    };

    let (mlflow_health, s3_health, filer_health, (s3_credentials, s3_hint)) = tokio::join!(
        check_health(&mlflow_url),
        check_health(&s3_url),
        check_health(&filer_url),
        fetch_seaweedfs_credentials(),
    );

    let (serving_url, serving_health, serving_hint) = match serving_port {
        Some(port) => {
            let base_url = format!("http://127.0.0.1:{port}/v1");
            let health = check_serving_health(&base_url).await;
            (
                base_url,
                health,
                Some("OpenAI-compatible API — no API key required.".to_string()),
            )
        }
        None => (
            String::new(),
            "unreachable".to_string(),
            Some("Serving is not running — start serving from the MLX Studio tab.".to_string()),
        ),
    };

    Ok(vec![
        ServiceAccess {
            service: "MLflow".into(),
            url: mlflow_url,
            health: mlflow_health,
            credential_hint: Some("No authentication required.".into()),
            credentials: Vec::new(),
        },
        ServiceAccess {
            service: "SeaweedFS S3 API".into(),
            url: s3_url,
            health: s3_health,
            credential_hint: s3_hint,
            credentials: s3_credentials,
        },
        ServiceAccess {
            service: "SeaweedFS Filer UI".into(),
            url: filer_url,
            health: filer_health,
            credential_hint: Some("No authentication required.".into()),
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
