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
    std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create config directory: {e}"))?;
    Ok(dir.join(TARGET_FILE))
}

#[tauri::command]
pub async fn get_deploy_target(app: tauri::AppHandle) -> Result<DeployTarget, String> {
    let path = target_path(&app)?;
    let target: DeployTarget = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| {
            format!("failed to parse {TARGET_FILE}: {e}. Please re-select the deploy target.")
        })?,
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
    std::fs::write(&path, text).map_err(|e| format!("failed to save {TARGET_FILE}: {e}"))?;
    set_active(&target);
    Ok(target)
}

/// 파괴적 액션 실행 직전 확인 다이얼로그에 쓰는 요약(#18 축소 스코프). 프런트가 이 값을
/// 받아 "지금 어느 컨텍스트/네임스페이스에 무엇을 하려는지"를 보여준다 — 다이얼로그 자체는
/// 이 lane의 스코프 밖이다.
#[derive(Debug, serde::Serialize)]
pub struct OperationSummary {
    pub context: String,
    pub namespace: String,
    pub action: String,
    pub target_description: String,
}

/// `action`별 요약을 조립하는 순수 함수 — `DeployTarget`을 이미 들고 있는 호출부(테스트 포함)가
/// `AppHandle` 없이 바로 쓸 수 있게 분리했다. colima 수명주기 액션은 저장된 배포 대상과
/// 무관하게 항상 colima 컨텍스트로 고정한다(D26: colima 수명주기는 이 앱이 소유하는 유일한
/// 대상).
///
/// 알려지지 않은 `action`은 거부한다 — 요약을 지어내면 다이얼로그가 틀린 대상을 보여줄 수
/// 있으므로(D22–D25), 이 커맨드가 아는 액션만 명시적으로 처리한다.
pub fn build_operation_summary(
    action: &str,
    target: Option<&DeployTarget>,
) -> Result<OperationSummary, String> {
    match action {
        "start_cluster" | "stop_cluster" => Ok(OperationSummary {
            context: COLIMA_CONTEXT.to_string(),
            namespace: "-".to_string(),
            action: action.to_string(),
            target_description: format!(
                "colima Kubernetes VM을 {}합니다. colima는 재진입 불가 — 진행 중에는 \
                 다른 수명주기 작업(start/stop)을 실행할 수 없습니다.",
                if action == "start_cluster" { "시작" } else { "정지" }
            ),
        }),
        "provision_mlops_stack" | "install_kagent" => {
            let target = target.ok_or_else(|| {
                format!("'{action}' requires a deploy target to build the confirmation summary")
            })?;
            let cluster_kind = if target.is_colima() { "로컬 colima" } else { "외부" };
            let action_label = if action == "provision_mlops_stack" {
                "MLOps 스택(MLflow/SeaweedFS/Prefect) 전체 배포"
            } else {
                "kagent 컨트롤러/도구/UI 설치"
            };
            Ok(OperationSummary {
                context: target.context.clone(),
                namespace: target.namespace.clone(),
                action: action.to_string(),
                target_description: format!(
                    "{cluster_kind} 클러스터 '{}'의 네임스페이스 '{}'에 {action_label}을(를) \
                     적용합니다.",
                    target.context, target.namespace
                ),
            })
        }
        other => Err(format!("Unknown operation for confirmation summary: {other}")),
    }
}

/// 프런트가 파괴적 액션(재프로비저닝, 외부 클러스터 풀스택 배포, colima 수명주기 등)
/// 실행 직전에 호출해 확인 다이얼로그에 쓸 요약을 받는다. 백엔드는 요약만 만든다 —
/// 다이얼로그 표시·사용자 확인은 프런트 몫이다.
#[tauri::command]
pub async fn describe_deploy_operation(
    app: tauri::AppHandle,
    action: String,
) -> Result<OperationSummary, String> {
    match action.as_str() {
        "start_cluster" | "stop_cluster" => build_operation_summary(&action, None),
        _ => {
            let target = get_deploy_target(app).await?;
            build_operation_summary(&action, Some(&target))
        }
    }
}

pub(crate) async fn kubectl_json(context: &str, args: &[&str]) -> Result<serde_json::Value, String> {
    let output = external_command("kubectl")?
        .args(["--context", context, "--request-timeout=30s"])
        .args(args)
        .output()
        .await
        .map_err(|e| format!("failed to run kubectl: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "kubectl {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("failed to parse kubectl output: {e}"))
}

/// 배포 차단 사유를 안정 코드로 나른다(D31). `detail`은 IP·개수 같은 언어중립 가변값만
/// 담는다 — 프런트는 `code`를 i18n 테이블 키로 쓴다.
///
/// 알려진 코드: `no_default_storage_class`, `namespace_owned_by_argocd`(소유 앱 목록은
/// `argocd_owners` 필드에 이미 있어 detail 없음), `no_bridge_candidates`.
#[derive(serde::Serialize)]
pub struct Blocker {
    pub code: String,
    pub detail: Option<String>,
}

#[derive(serde::Serialize)]
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
    /// 배포 차단 사유. 비어 있으면 배포 가능.
    pub blockers: Vec<Blocker>,
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
        blockers.push(Blocker {
            code: "no_default_storage_class".into(),
            detail: None,
        });
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
        // 소유 앱 이름 목록은 argocd_owners 필드에 이미 있다 — detail에 중복하지 않는다.
        blockers.push(Blocker {
            code: "namespace_owned_by_argocd".into(),
            detail: None,
        });
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
        blockers.push(Blocker {
            code: "no_bridge_candidates".into(),
            detail: None,
        });
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
        .map_err(|e| format!("failed to run ifconfig: {e}"))?;
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
            reason_code: "no_interface_candidates".into(),
            detail: None,
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
            Ok(false) => failures.push(format!("{candidate}: unreachable from cluster")),
            Err(e) => failures.push(format!("{candidate}: {e}")),
        }
    }

    Ok(BridgeState::Unverified {
        candidates,
        reason_code: "unreachable_from_cluster".into(),
        // macOS 방화벽이 수신을 차단했을 가능성이 detail의 흔한 원인이다 — 사람이 읽는
        // 문장이 아니라 후보별 실패 사유를 " / "로 이어붙인 진단 텍스트다.
        detail: Some(failures.join(" / ")),
    })
}

/// 후보 주소에 임시 리스너를 띄우고, 클러스터 안에서 파드로 접속을 시도한다.
/// 호스트에서만 확인하면 아무 의미가 없다 — 검증의 주체는 **클러스터**여야 한다.
async fn probe_candidate(context: &str, namespace: &str, candidate: &str) -> Result<bool, String> {
    // 0.0.0.0이 아니라 후보 주소에만 바인드한다 — 노출 범위를 그 인터페이스로 제한.
    let listener = tokio::net::TcpListener::bind(format!("{candidate}:0"))
        .await
        .map_err(|e| format!("failed to bind listener: {e}"))?;
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

    let output = result.map_err(|e| format!("failed to run probe pod: {e}"))?;
    if output.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    // 이미지를 못 받은 것과 네트워크가 안 통한 것은 원인이 다르다 — 구분해서 올린다.
    if stderr.contains("ImagePull") || stderr.contains("ErrImagePull") {
        return Err(format!(
            "could not verify: failed to pull probe image ({PROBE_IMAGE}): {}",
            stderr.trim()
        ));
    }
    Ok(false)
}

#[cfg(test)]
mod summary_tests {
    use super::*;

    #[test]
    fn colima_lifecycle_summary_ignores_saved_target() {
        // 저장된 대상이 외부 클러스터여도 colima 수명주기 액션은 항상 colima를 가리켜야 한다.
        let external = DeployTarget::for_context("narwhal");
        let summary = build_operation_summary("stop_cluster", Some(&external)).unwrap();
        assert_eq!(summary.context, COLIMA_CONTEXT);
        assert_eq!(summary.action, "stop_cluster");
        assert!(summary.target_description.contains("정지"));

        let summary_none = build_operation_summary("start_cluster", None).unwrap();
        assert_eq!(summary_none.context, COLIMA_CONTEXT);
        assert!(summary_none.target_description.contains("시작"));
    }

    #[test]
    fn provision_summary_reflects_saved_deploy_target() {
        let mut target = DeployTarget::for_context("narwhal");
        target.namespace = "team-ml".into();
        let summary = build_operation_summary("provision_mlops_stack", Some(&target)).unwrap();
        assert_eq!(summary.context, "narwhal");
        assert_eq!(summary.namespace, "team-ml");
        assert!(summary.target_description.contains("외부"));
        assert!(summary.target_description.contains("team-ml"));
    }

    #[test]
    fn provision_summary_requires_a_target() {
        let err = build_operation_summary("provision_mlops_stack", None)
            .expect_err("provisioning without a target must be rejected");
        assert!(err.contains("provision_mlops_stack"));
    }

    #[test]
    fn unknown_action_is_rejected_not_fabricated() {
        let err = build_operation_summary("delete_everything", None)
            .expect_err("unknown actions must not get a fabricated summary");
        assert!(err.contains("delete_everything"));
    }
}
