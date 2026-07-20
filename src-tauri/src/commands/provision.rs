use tauri::Manager;
use crate::services::process::{resolve_bundled_resource, resolve_cli_path};

const MANIFESTS: [&str; 4] = [
    // Secret을 먼저 적용해 mlflow-deployment.yaml의 secretKeyRef가 기동 시점에
    // 즉시 해석되도록 한다(D13 — SeaweedFS S3 크리덴셜 자동 와이어링).
    "scripts/k8s/seaweedfs-s3-credentials.yaml",
    "scripts/k8s/mlflow-deployment.yaml",
    "scripts/k8s/seaweedfs-deployment.yaml",
    "scripts/k8s/mac-gpu-bridge.yaml",
];

#[tauri::command]
pub async fn provision_mlops_stack(app: tauri::AppHandle) -> Result<String, String> {
    let kubectl = resolve_cli_path("kubectl")?;
    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;

    for manifest in MANIFESTS {
        let path = resolve_bundled_resource(&resource_dir, manifest);
        let output = tokio::process::Command::new(&kubectl)
            .args(["--context", "colima", "apply", "-f"])
            .arg(&path)
            .output()
            .await
            .map_err(|e| format!("kubectl apply 실패({manifest}): {e}"))?;

        if !output.status.success() {
            return Err(format!(
                "{manifest} 적용 실패: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }

    Ok("MLflow / SeaweedFS / GPU 브리지 매니페스트가 적용되었습니다.".into())
}
