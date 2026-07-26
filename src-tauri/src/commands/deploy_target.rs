//! 배포 대상 선택·사전점검·브리지 탐지 IPC.
//!
//! 사전점검과 탐지는 **전부 실측**이다. 조회에 실패하면 실패를 그대로 올린다 —
//! "아마 될 것"을 상태로 반환하지 않는다(D22–D25).

use std::net::Ipv4Addr;
use tauri::Manager;

use crate::services::deploy_target::{
    bridge_candidates, parse_ifconfig, set_active, BridgeState, DeployTarget, COLIMA_CONTEXT,
};
use crate::services::process::external_command;

const TARGET_FILE: &str = "deploy-target.json";

/// 브리지 프로브에 쓰는 이미지. mlflow initContainer가 이미 쓰는 이미지라 폐쇄망 번들에
/// 별도 항목을 추가하지 않아도 된다.
const PROBE_IMAGE: &str = "docker.io/curlimages/curl:8.21.0";

fn target_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("설정 디렉터리 생성 실패: {e}"))?;
    Ok(dir.join(TARGET_FILE))
}

#[tauri::command]
pub async fn get_deploy_target(app: tauri::AppHandle) -> Result<DeployTarget, String> {
    let path = target_path(&app)?;
    let target: DeployTarget = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| format!("{TARGET_FILE} 파싱 실패: {e}. 대상을 다시 선택하세요."))?,
        // 저장된 선택이 없으면 colima가 기본값 — 기존 사용자의 동작이 바뀌지 않는다.
        Err(_) => DeployTarget::for_context(COLIMA_CONTEXT),
    };
    set_active(&target);
    Ok(target)
}

#[tauri::command]
pub async fn save_deploy_target(
    app: tauri::AppHandle,
    target: DeployTarget,
) -> Result<DeployTarget, String> {
    let path = target_path(&app)?;
    let text = serde_json::to_string_pretty(&target).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{TARGET_FILE} 저장 실패: {e}"))?;
    set_active(&target);
    Ok(target)
}

async fn kubectl_json(context: &str, args: &[&str]) -> Result<serde_json::Value, String> {
    let output = external_command("kubectl")?
        .args(["--context", context, "--request-timeout=30s"])
        .args(args)
        .output()
        .await
        .map_err(|e| format!("kubectl 실행 실패: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "kubectl {} 실패: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("kubectl 출력 파싱 실패: {e}"))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightReport {
    pub context: String,
    pub reachable: bool,
    pub node_count: usize,
    pub node_ips: Vec<String>,
    pub default_storage_class: Option<String>,
    pub storage_classes: Vec<String>,
    pub argocd_present: bool,
    /// 대상 네임스페이스를 destination으로 삼는 ArgoCD Application. 비어 있지 않으면
    /// 직접 apply가 selfHeal에 되돌려지므로 GitOps 경로로 가야 한다.
    pub argocd_owners: Vec<String>,
    pub enforcing_policies: Vec<String>,
    pub bridge_candidates: Vec<String>,
    /// 사람이 읽을 차단 사유. 비어 있으면 배포 가능.
    pub blockers: Vec<String>,
}

#[tauri::command]
pub async fn preflight_deploy_target(
    context: String,
    namespace: String,
) -> Result<PreflightReport, String> {
    let mut blockers = Vec::new();

    let nodes = kubectl_json(&context, &["get", "nodes", "-o", "json"]).await?;
    let node_ips: Vec<String> = nodes["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|n| {
                    n["status"]["addresses"].as_array()?.iter().find_map(|a| {
                        (a["type"] == "InternalIP").then(|| a["address"].as_str())?
                    })
                })
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let node_count = node_ips.len();

    // StorageClass — 기본값이 없고 target에도 지정이 없으면 PVC가 Pending으로 멈춘다.
    let sc = kubectl_json(&context, &["get", "sc", "-o", "json"]).await?;
    let mut storage_classes = Vec::new();
    let mut default_storage_class = None;
    if let Some(items) = sc["items"].as_array() {
        for item in items {
            let Some(name) = item["metadata"]["name"].as_str() else {
                continue;
            };
            storage_classes.push(name.to_string());
            let is_default = item["metadata"]["annotations"]
                ["storageclass.kubernetes.io/is-default-class"]
                .as_str()
                == Some("true");
            if is_default {
                default_storage_class = Some(name.to_string());
            }
        }
    }
    if default_storage_class.is_none() {
        blockers.push(
            "대상 클러스터에 기본 StorageClass가 없습니다. StorageClass를 명시하지 않으면 \
             Prefect PVC가 Pending에서 멈춥니다."
                .into(),
        );
    }

    // ArgoCD — CRD 유무와, 대상 네임스페이스를 소유한 Application 목록.
    let argocd_present = kubectl_json(
        &context,
        &["get", "crd", "applications.argoproj.io", "-o", "json"],
    )
    .await
    .is_ok();

    let mut argocd_owners = Vec::new();
    if argocd_present {
        if let Ok(apps) = kubectl_json(&context, &["get", "applications", "-A", "-o", "json"]).await
        {
            if let Some(items) = apps["items"].as_array() {
                for app in items {
                    if app["spec"]["destination"]["namespace"].as_str() == Some(namespace.as_str())
                    {
                        if let Some(name) = app["metadata"]["name"].as_str() {
                            argocd_owners.push(name.to_string());
                        }
                    }
                }
            }
        }
    }
    if !argocd_owners.is_empty() {
        blockers.push(format!(
            "네임스페이스 '{namespace}'는 ArgoCD Application({})이 소유합니다. \
             직접 apply는 selfHeal이 되돌리므로 GitOps export 경로를 쓰세요.",
            argocd_owners.join(", ")
        ));
    }

    // Kyverno Enforce 정책 — 위반하면 admission에서 막힌다. 목록만 보여주고 판단은 사람이.
    let mut enforcing_policies = Vec::new();
    if let Ok(pols) = kubectl_json(&context, &["get", "cpol", "-o", "json"]).await {
        if let Some(items) = pols["items"].as_array() {
            for p in items {
                if p["spec"]["validationFailureAction"].as_str() == Some("Enforce") {
                    if let Some(name) = p["metadata"]["name"].as_str() {
                        enforcing_policies.push(name.to_string());
                    }
                }
            }
        }
    }

    let candidates = detect_bridge_candidates(&node_ips).await?;
    if candidates.is_empty() && context != COLIMA_CONTEXT {
        blockers.push(
            "노드 서브넷과 연결된 호스트 인터페이스가 없어 브리지 후보를 찾지 못했습니다. \
             MLX 호스트 연동이 필요하면 VM 네트워크를 확인하세요."
                .into(),
        );
    }

    Ok(PreflightReport {
        context,
        reachable: true,
        node_count,
        node_ips,
        default_storage_class,
        storage_classes,
        argocd_present,
        argocd_owners,
        enforcing_policies,
        bridge_candidates: candidates,
        blockers,
    })
}

/// 호스트 인터페이스를 열거해 노드 서브넷과 겹치는 주소를 후보로 낸다.
async fn detect_bridge_candidates(node_ips: &[String]) -> Result<Vec<String>, String> {
    let output = external_command("ifconfig")?
        .arg("-a")
        .output()
        .await
        .map_err(|e| format!("ifconfig 실행 실패: {e}"))?;
    let text = String::from_utf8_lossy(&output.stdout);

    let parsed: Vec<Ipv4Addr> = node_ips.iter().filter_map(|ip| ip.parse().ok()).collect();
    Ok(bridge_candidates(&parse_ifconfig(&text), &parsed))
}

#[tauri::command]
pub async fn detect_host_bridge(context: String, namespace: String) -> Result<BridgeState, String> {
    if context == COLIMA_CONTEXT {
        // D10 실측값이 이미 있다. 추가 탐지는 불필요하고, DNS 이름이라 인터페이스 계산 대상도 아니다.
        return Ok(BridgeState::KeepBase);
    }

    let nodes = kubectl_json(&context, &["get", "nodes", "-o", "json"]).await?;
    let node_ips: Vec<String> = nodes["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|n| {
                    n["status"]["addresses"].as_array()?.iter().find_map(|a| {
                        (a["type"] == "InternalIP").then(|| a["address"].as_str())?
                    })
                })
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let candidates = detect_bridge_candidates(&node_ips).await?;
    if candidates.is_empty() {
        return Ok(BridgeState::Unverified {
            candidates,
            reason: "노드 서브넷과 같은 대역의 호스트 인터페이스를 찾지 못했습니다.".into(),
        });
    }

    let mut failures = Vec::new();
    for candidate in &candidates {
        match probe_candidate(&context, &namespace, candidate).await {
            Ok(true) => {
                return Ok(BridgeState::Verified {
                    host: candidate.clone(),
                })
            }
            Ok(false) => failures.push(format!("{candidate}: 클러스터에서 도달 실패")),
            Err(e) => failures.push(format!("{candidate}: {e}")),
        }
    }

    Ok(BridgeState::Unverified {
        candidates,
        reason: format!(
            "후보를 찾았지만 클러스터 내부에서 도달을 확인하지 못했습니다 ({}). \
             macOS 방화벽이 수신을 차단했을 수 있습니다.",
            failures.join(" / ")
        ),
    })
}

/// 후보 주소에 임시 리스너를 띄우고, 클러스터 안에서 파드로 접속을 시도한다.
/// 호스트에서만 확인하면 아무 의미가 없다 — 검증의 주체는 **클러스터**여야 한다.
async fn probe_candidate(context: &str, namespace: &str, candidate: &str) -> Result<bool, String> {
    // 0.0.0.0이 아니라 후보 주소에만 바인드한다 — 노출 범위를 그 인터페이스로 제한.
    let listener = tokio::net::TcpListener::bind(format!("{candidate}:0"))
        .await
        .map_err(|e| format!("리스너 바인드 실패: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| e.to_string())?
        .port();

    let server = tokio::spawn(async move {
        // 프로브 파드 한 번의 접속만 받으면 된다.
        if let Ok((mut sock, _)) = listener.accept().await {
            use tokio::io::AsyncWriteExt;
            let _ = sock
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .await;
            let _ = sock.flush().await;
        }
    });

    let pod = format!("kubemetal-bridge-probe-{port}");
    let url = format!("http://{candidate}:{port}/");
    let result = external_command("kubectl")?
        .args([
            "--context",
            context,
            "--request-timeout=120s",
            "run",
            &pod,
            "-n",
            namespace,
            "--image",
            PROBE_IMAGE,
            "--restart=Never",
            "--attach",
            "--rm",
            "--quiet",
            "--command",
            "--",
            "curl",
            "-sf",
            "-m",
            "10",
            &url,
        ])
        .output()
        .await;

    server.abort();

    // 파드가 남아 있으면 다음 탐지가 이름 충돌로 실패한다. --rm이 실패했을 경우를 대비해 정리.
    let _ = external_command("kubectl")?
        .args([
            "--context", context, "delete", "pod", &pod, "-n", namespace,
            "--ignore-not-found", "--force", "--grace-period=0",
        ])
        .output()
        .await;

    let output = result.map_err(|e| format!("프로브 파드 실행 실패: {e}"))?;
    if output.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    // 이미지를 못 받은 것과 네트워크가 안 통한 것은 원인이 다르다 — 구분해서 올린다.
    if stderr.contains("ImagePull") || stderr.contains("ErrImagePull") {
        return Err(format!(
            "프로브 이미지({PROBE_IMAGE})를 받지 못해 검증할 수 없습니다: {}",
            stderr.trim()
        ));
    }
    Ok(false)
}
