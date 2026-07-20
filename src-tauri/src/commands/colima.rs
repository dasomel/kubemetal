use serde::{Deserialize, Serialize};
use crate::services::process::resolve_cli_path;

/// colima 0.10.x `status --json` 실측 스키마: 기동 중일 때만 exit 0 + stdout에
/// 평면 JSON({"kubernetes":true,...})을 출력하고, 미기동이면 exit 1 + stdout 없음.
/// "status" 필드는 존재하지 않는다 — 기동 여부는 파싱 성공 자체로 판별한다.
#[derive(Debug, Deserialize)]
struct ColimaStatusRaw {
    #[serde(default)]
    kubernetes: bool,
}

#[derive(Debug, Serialize)]
pub struct ClusterStatus {
    pub is_running: bool,
    pub kubernetes_active: bool,
    pub mlflow_ready: bool,
    pub seaweedfs_ready: bool,
}

#[tauri::command]
pub async fn get_cluster_status() -> Result<ClusterStatus, String> {
    let bin = resolve_cli_path("colima")?;
    let output = tokio::process::Command::new(&bin)
        .args(["status", "--json"])
        .output()
        .await
        .map_err(|e| format!("colima 실행 실패: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let raw: Option<ColimaStatusRaw> = if output.status.success() {
        serde_json::from_str(&stdout).ok()
    } else {
        None
    };

    let Some(raw) = raw else {
        return Ok(ClusterStatus {
            is_running: false,
            kubernetes_active: false,
            mlflow_ready: false,
            seaweedfs_ready: false,
        });
    };

    let is_running = true; // exit 0 + JSON 출력 = 기동 중 (미기동은 위에서 조기 반환)
    let kubernetes_active = raw.kubernetes;

    let (mlflow_ready, seaweedfs_ready) = if kubernetes_active {
        let kubectl = resolve_cli_path("kubectl")?;
        let deploy_out = tokio::process::Command::new(&kubectl)
            .args(["--context", "colima", "get", "deploy", "-n", "default", "-o", "json"])
            .output()
            .await
            .map_err(|e| format!("kubectl get deploy 실패: {e}"))?;
        let json: serde_json::Value =
            serde_json::from_slice(&deploy_out.stdout).unwrap_or(serde_json::json!({"items": []}));
        let items = json["items"].as_array().cloned().unwrap_or_default();
        let is_ready = |name: &str| {
            items.iter().any(|d| {
                d["metadata"]["name"].as_str() == Some(name)
                    && d["status"]["availableReplicas"].as_u64().unwrap_or(0) > 0
            })
        };
        (is_ready("mlflow"), is_ready("seaweedfs"))
    } else {
        (false, false)
    };

    Ok(ClusterStatus {
        is_running,
        kubernetes_active,
        mlflow_ready,
        seaweedfs_ready,
    })
}

#[tauri::command]
pub async fn start_cluster(cpu: u32, memory: u32) -> Result<String, String> {
    let bin = resolve_cli_path("colima")?;

    let mut sys = sysinfo::System::new_all();
    sys.refresh_memory();
    sys.refresh_cpu_usage();
    let host_ram_gb = sys.total_memory() / 1024 / 1024 / 1024;
    let host_cores = sys.cpus().len().max(1) as u32;
    let max_memory_gb: u64 = match host_ram_gb {
        0..=23 => 4,
        24..=55 => 8,
        _ => 12,
    };
    let memory = memory.min(max_memory_gb as u32).max(1);
    let cpu = cpu.clamp(1, host_cores);

    let output = tokio::process::Command::new(bin)
        .args([
            "start",
            "--cpu", &cpu.to_string(),
            "--memory", &memory.to_string(),
            "--vm-type=vz",
            "--mount-type=virtiofs",
            "--kubernetes",
        ])
        .output()
        .await
        .map_err(|e| format!("colima start 실행 실패: {e}"))?;

    if output.status.success() {
        Ok("Colima K8s 클러스터가 시작되었습니다.".into())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

#[tauri::command]
pub async fn stop_cluster() -> Result<String, String> {
    let bin = resolve_cli_path("colima")?;
    let output = tokio::process::Command::new(bin)
        .arg("stop")
        .output()
        .await
        .map_err(|e| format!("colima stop 실행 실패: {e}"))?;

    if output.status.success() {
        Ok("Colima 클러스터가 정지되었습니다.".into())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}
