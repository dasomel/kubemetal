use tauri::Manager;

use crate::commands::deploy_target::get_deploy_target;
use crate::services::process::{external_command, resolve_bundled_resource};

/// 매니페스트 목록은 여기 없다 — `scripts/k8s/kustomization.yaml`이 단일 출처이고
/// 렌더링 규칙은 `scripts/k8s/render.sh`가 소유한다. 예전에는 같은 목록이 이 파일과
/// Makefile에 각각 있어 한쪽만 바뀌면 조용히 어긋났다.
const RENDER_SCRIPT: &str = "scripts/k8s/render.sh";

#[tauri::command]
pub async fn provision_mlops_stack(app: tauri::AppHandle) -> Result<String, String> {
    let target = get_deploy_target(app.clone()).await?;
    // 외부 클러스터의 기본 통합은 에이전트 온리다 — 풀스택 프로비저닝을 차단한다(D30).
    target.full_stack_gate()?;
    // 브리지가 미검증이면 여기서 막힌다 — 추측 주소를 클러스터로 보내지 않기 위해서다.
    let render_args = target.render_args()?;

    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let script = resolve_bundled_resource(&resource_dir, RENDER_SCRIPT);

    // 외부 클러스터는 전용 네임스페이스를 쓴다 — 없으면 만든다. 공유 클러스터의
    // default를 건드리지 않기 위한 D26 결정.
    if !target.is_colima() {
        ensure_namespace(&target.context, &target.namespace).await?;
    }

    let rendered = external_command("bash")?
        .arg(&script)
        .args(&render_args)
        .output()
        .await
        .map_err(|e| format!("failed to run render.sh: {e}"))?;

    if !rendered.status.success() {
        return Err(format!(
            "manifest render failed: {}",
            String::from_utf8_lossy(&rendered.stderr).trim()
        ));
    }

    let applied = apply_stdin(&target.context, &rendered.stdout).await?;

    Ok(format!(
        "[{}] applied MLOps stack to namespace {}.\n{}",
        target.context,
        target.namespace,
        applied.trim()
    ))
}

/// `kagent.rs::install_kagent`도 같은 ns-생성 규약(Makefile `kagent-up`과 동일하게
/// `--dry-run=client -o yaml` 렌더 후 apply)을 쓰므로 `pub(crate)`로 공유한다 — 네임스페이스
/// 생성 로직을 두 곳에 따로 두지 않는다.
pub(crate) async fn ensure_namespace(context: &str, namespace: &str) -> Result<(), String> {
    let manifest = external_command("kubectl")?
        .args([
            "--context", context, "create", "namespace", namespace,
            "--dry-run=client", "-o", "yaml",
        ])
        .output()
        .await
        .map_err(|e| format!("failed to generate namespace manifest: {e}"))?;

    if !manifest.status.success() {
        return Err(format!(
            "failed to generate namespace manifest: {}",
            String::from_utf8_lossy(&manifest.stderr).trim()
        ));
    }
    // ambient 메시 라벨은 붙이지 않는다 — Istio ambient에 편입되면 ztunnel HBONE이
    // plain-HTTP kubelet 프로브를 깬다(narwhal 실측: ambient는 ns opt-in이다).
    apply_stdin(context, &manifest.stdout).await.map(|_| ())
}

/// 렌더 결과를 파일로 떨구지 않고 stdin으로 흘린다 — 임시 파일이 남지 않고,
/// 적용된 내용이 렌더된 내용과 어긋날 여지도 없다.
async fn apply_stdin(context: &str, manifest: &[u8]) -> Result<String, String> {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;

    let mut child = external_command("kubectl")?
        .args(["--context", context, "--request-timeout=120s", "apply", "-f", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to run kubectl apply: {e}"))?;

    child
        .stdin
        .take()
        .ok_or("could not open kubectl stdin")?
        .write_all(manifest)
        .await
        .map_err(|e| format!("failed to write manifest: {e}"))?;

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| format!("failed waiting for kubectl apply: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "kubectl apply failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    fn repo_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf()
    }

    fn kustomization_resources() -> Vec<String> {
        let text = std::fs::read_to_string(repo_root().join("scripts/k8s/kustomization.yaml"))
            .expect("read kustomization.yaml");
        text.lines()
            .filter_map(|line| line.trim().strip_prefix("- ").map(str::to_string))
            .collect()
    }

    /// D13: Secret이 먼저 적용돼야 mlflow의 secretKeyRef가 기동 시점에 즉시 해석된다.
    /// 이 목록은 예전에 `provision.rs::MANIFESTS`와 Makefile에 이중으로 있었고, 실제로
    /// 어긋난 적이 있다. 이제 kustomization.yaml이 단일 출처이므로 순서만 지킨다.
    #[test]
    fn kustomization_keeps_d13_secret_first_order() {
        assert_eq!(
            kustomization_resources(),
            vec![
                "seaweedfs-s3-credentials.yaml",
                "mlflow-deployment.yaml",
                "seaweedfs-deployment.yaml",
                "mac-gpu-bridge.yaml",
                "prefect-deployment.yaml",
            ],
            "D13 order broken — Secret must come first for mlflow's secretKeyRef to resolve"
        );
    }

    /// 디렉터리 통째 apply 금지의 근거를 테스트로 고정한다. `scripts/k8s/`에는 ns가
    /// default가 아닌 매니페스트(security-agent, ns=kagent)와 E2E 산출물이 함께 있어
    /// 전부 넣으면 엉뚱한 리소스가 배포된다.
    #[test]
    fn kustomization_excludes_non_stack_manifests() {
        let resources = kustomization_resources();
        for excluded in ["security-agent.yaml", "e2e-remediated-nginx.yaml"] {
            assert!(
                !resources.contains(&excluded.to_string()),
                "{excluded} is not part of the MLOps stack — must not be in kustomization"
            );
        }
    }

    /// render.sh가 없으면 프로비저닝 경로 전체가 죽는다. 번들 리소스 목록에서
    /// 빠지는 사고를 막기 위해 존재와 실행 권한을 함께 확인한다.
    #[test]
    fn render_script_exists_and_is_executable() {
        let path = repo_root().join(super::RENDER_SCRIPT);
        assert!(path.is_file(), "{} is missing", path.display());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).expect("metadata").permissions().mode();
            assert!(mode & 0o111 != 0, "render.sh is not executable");
        }
    }
}
