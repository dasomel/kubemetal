use std::collections::HashMap;
use std::sync::Mutex;
use tokio::process::Child;
use tauri::State;
use crate::services::process::resolve_cli_path;

#[derive(Default)]
pub struct PortForwardState(pub Mutex<HashMap<&'static str, Child>>);

#[tauri::command]
pub async fn start_port_forward(state: State<'_, PortForwardState>) -> Result<String, String> {
    let kubectl = resolve_cli_path("kubectl")?;
    let jobs: [(&str, &str, &str); 3] = [
        ("mlflow", "svc/mlflow", "5001:5000"),
        ("seaweedfs-s3", "svc/seaweedfs", "8333:8333"),
        ("seaweedfs-filer", "svc/seaweedfs", "8888:8888"),
    ];
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;
    for (key, svc, ports) in jobs {
        let child = tokio::process::Command::new(&kubectl)
            .args(["--context", "colima", "port-forward", "-n", "default", svc, ports])
            .spawn()
            .map_err(|e| format!("port-forward({key}) 실행 실패: {e}"))?;
        guard.insert(key, child);
    }
    Ok("포트포워딩이 시작되었습니다.".into())
}

#[tauri::command]
pub async fn stop_port_forward(state: State<'_, PortForwardState>) -> Result<String, String> {
    let children: Vec<_> = {
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        guard.drain().map(|(_, child)| child).collect()
    };
    for mut child in children {
        let _ = child.kill().await;
    }
    Ok("포트포워딩이 정지되었습니다.".into())
}
