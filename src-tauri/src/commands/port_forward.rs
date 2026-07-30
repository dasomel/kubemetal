use std::collections::HashMap;
use std::sync::Mutex;
use tokio::process::Child;
use tauri::State;
use crate::services::process::external_command;

#[derive(Default)]
pub struct PortForwardState(pub Mutex<HashMap<&'static str, Child>>);

const JOBS: [(&str, &str, &str); 4] = [
    ("mlflow", "svc/mlflow", "5001:5000"),
    ("seaweedfs-s3", "svc/seaweedfs", "8333:8333"),
    ("seaweedfs-filer", "svc/seaweedfs", "8888:8888"),
    ("prefect", "svc/prefect", "4200:4200"),
];

/// 우리 앱이 관리하는 서비스만 대상으로 하는 pgrep 패턴. 무관한 kubectl 포워드는
/// 매칭되지 않도록 서비스명을 포함시킨다(불가침 경계).
const SERVICE_PATTERNS: [&str; 3] = [
    "port-forward.*svc/mlflow",
    "port-forward.*svc/seaweedfs",
    "port-forward.*svc/prefect",
];

/// `pgrep -f <pattern>`으로 매칭되는 pid 목록을 반환. 실패 시 빈 벡터(포워드 없음으로 취급).
async fn find_pids_by_pattern(pattern: &str) -> Vec<i32> {
    let Ok(mut cmd) = external_command("pgrep") else {
        return Vec::new();
    };
    match cmd.args(["-f", pattern]).output().await {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.trim().parse::<i32>().ok())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// 우리 서비스(svc/mlflow, svc/seaweedfs) 대상 kubectl port-forward를 셸에서 띄웠든
/// 앱 자식이든 상관없이 SIGTERM으로 인수/정리한다. `mlx.rs::terminate_pid`와 동일한
/// `libc::kill` 패턴 — 여기서는 자식 프로세스가 아니라 pgrep으로 찾은 외부 pid라 SIGKILL
/// 승급 없이 SIGTERM만 보낸다(kubectl은 SIGTERM에 즉시 종료).
async fn reap_external_port_forwards() -> usize {
    let mut count = 0usize;
    for pattern in SERVICE_PATTERNS {
        for pid in find_pids_by_pattern(pattern).await {
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
            count += 1;
        }
    }
    count
}

/// `curl -m 2`로 로컬 포트가 응답하는지 확인. HTTP 상태코드와 무관하게 000(연결 실패)만
/// 아니면 포워드가 살아있다고 판단한다(SeaweedFS S3 API는 403/400을 정상 응답).
async fn check_port_alive(host_port: &str) -> bool {
    let url = format!("http://127.0.0.1:{host_port}");
    let Ok(mut cmd) = external_command("curl") else {
        return false;
    };
    match cmd
        .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "-m", "2", &url])
        .output()
        .await
    {
        Ok(out) => {
            let code = String::from_utf8_lossy(&out.stdout);
            let code = code.trim();
            !code.is_empty() && code != "000"
        }
        Err(_) => false,
    }
}

#[tauri::command]
pub async fn start_port_forward(state: State<'_, PortForwardState>) -> Result<String, String> {
    // 1. 이전 세션에서 남은 우리 자식이 있으면 먼저 정리(재시작 케이스).
    let stale: Vec<Child> = {
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        guard.drain().map(|(_, c)| c).collect()
    };
    for mut child in stale {
        let _ = child.kill().await;
    }

    // 2. 외부(쉘 nohup 등)에서 떠 있는 잔여 포워드를 인수 — 우리 서비스 대상만.
    let reaped_external = reap_external_port_forwards().await;
    if reaped_external > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    }

    // 3. 신규 spawn.
    {
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        let (context, namespace) = crate::services::deploy_target::active_context();
        for (key, svc, ports) in JOBS {
            let child = external_command("kubectl")?
                .args(["--context", &context, "port-forward", "-n", &namespace, svc, ports])
                .spawn()
                .map_err(|e| format!("port-forward({key}) failed to start: {e}"))?;
            guard.insert(key, child);
        }
    }

    // 4. 성공 검증: 2초 대기 후 포트별 curl 확인. 실패한 포트의 자식은 정리하고
    //    부분 실패 사유를 포트별로 명시한다.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let mut failed: Vec<(&str, &str)> = Vec::new();
    for (key, _svc, ports) in JOBS {
        let host_port = ports.split(':').next().unwrap_or("");
        if !check_port_alive(host_port).await {
            failed.push((key, host_port));
        }
    }

    if !failed.is_empty() {
        let mut to_kill: Vec<Child> = Vec::new();
        {
            let mut guard = state.0.lock().map_err(|e| e.to_string())?;
            for (key, _) in &failed {
                if let Some(child) = guard.remove(key) {
                    to_kill.push(child);
                }
            }
        }
        for mut child in to_kill {
            let _ = child.kill().await;
        }
        let detail = failed
            .iter()
            .map(|(key, port)| format!("{key}(:{port}) not responding"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "Port forwarding partially failed: {detail} (remaining ports started normally)"
        ));
    }

    let mut msg = "Port forwarding started.".to_string();
    if reaped_external > 0 {
        msg.push_str(&format!(" (took over {reaped_external} leftover external forward(s))"));
    }
    Ok(msg)
}

#[tauri::command]
pub async fn stop_port_forward(state: State<'_, PortForwardState>) -> Result<String, String> {
    let children: Vec<_> = {
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        guard.drain().map(|(_, child)| child).collect()
    };
    let tracked_count = children.len();
    for mut child in children {
        let _ = child.kill().await;
    }

    // "정지는 항상 통한다" — 우리가 추적하지 못한 외부 매칭 포워드도 함께 정리.
    let reaped_external = reap_external_port_forwards().await;

    let total = tracked_count + reaped_external;
    Ok(format!("Port forwarding stopped. ({total} process(es) cleaned up)"))
}
