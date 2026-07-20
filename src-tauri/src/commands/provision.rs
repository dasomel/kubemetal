use tauri::Manager;
use crate::services::process::resolve_cli_path;

const MANIFESTS: [&str; 3] = [
    "scripts/k8s/mlflow-deployment.yaml",
    "scripts/k8s/minio-deployment.yaml",
    "scripts/k8s/mac-gpu-bridge.yaml",
];

#[tauri::command]
pub async fn provision_mlops_stack(app: tauri::AppHandle) -> Result<String, String> {
    let kubectl = resolve_cli_path("kubectl")?;
    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;

    for manifest in MANIFESTS {
        let path = resource_dir.join(manifest);
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

    Ok("MLflow / MinIO / GPU 브리지 매니페스트가 적용되었습니다.".into())
}

// Phase 2 커맨드 예약:
// #[tauri::command]
// pub async fn run_mlx_finetune(config: serde_json::Value) -> Result<u32, String> {
//     todo!("Phase 2: MLX 파인튜닝 프로세스 띄우고 PID 리턴")
// }
//
// #[tauri::command]
// pub async fn kill_mlx_process(pid: u32) -> Result<bool, String> {
//     todo!("Phase 2: 실행 중인 MLX 학습/서빙 프로세스 중지")
// }
