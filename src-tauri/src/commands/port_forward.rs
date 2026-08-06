use std::collections::HashMap;
use std::sync::Mutex;
use tokio::process::Child;
use tauri::State;
use crate::services::ports;
use crate::services::process::external_command;

#[derive(Default)]
pub struct PortForwardState(pub Mutex<HashMap<&'static str, Child>>);

/// (레지스트리 키, k8s 대상, 네임스페이스, 클러스터 쪽 포트).
///
/// **호스트 포트는 여기 없다** — 기동 시점에 `services::ports`가 비어 있는 것을 골라
/// 배정한다(D1 개정: 표의 숫자는 우선 시도값이지 보장이 아니다). 키는 `ports::SPECS`에
/// 있어야 하며 `jobs_keys_are_registered` 테스트가 그 일치를 고정한다.
///
/// 네임스페이스가 `None`이면 배포 대상의 네임스페이스(D26: colima=`default`,
/// 외부=`kubemetal`)를 쓰고, `Some`이면 그 값을 쓴다. kagent은 대상과 무관하게 자기
/// 네임스페이스에 설치되므로 고정이다 — 예전에는 모든 잡에 대상 네임스페이스를 넘겨
/// `-n default svc/kagent-ui`가 되었고, 그 서비스는 거기 없어서 이 잡은 **항상 실패했다**
/// (실측 2026-08-06: `kagent-ui(:8090) not responding`). Makefile은 처음부터 `-n kagent`였다.
const JOBS: [(&str, &str, Option<&str>, u16); 5] = [
    ("mlflow", "svc/mlflow", None, 5000),
    ("seaweedfs-s3", "svc/seaweedfs", None, 8333),
    ("seaweedfs-filer", "svc/seaweedfs", None, 8888),
    ("prefect", "svc/prefect", None, 4200),
    ("kagent-ui", "svc/kagent-ui", Some("kagent"), 8080),
];

/// 우리 앱이 관리하는 서비스만 대상으로 하는 pgrep 패턴. 무관한 kubectl 포워드는
/// 매칭되지 않도록 서비스명을 포함시킨다(불가침 경계).
const SERVICE_PATTERNS: [&str; 4] = [
    "port-forward.*svc/mlflow",
    "port-forward.*svc/seaweedfs",
    "port-forward.*svc/prefect",
    "port-forward.*svc/kagent-ui",
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

    // 2-a. 방금 정리한 포워드가 D1 포트를 아직 놓지 않았을 수 있다. 그 상태로 배정하면
    //      "우리가 쓰던 포트"를 남의 것으로 오인해 재시작마다 포트가 한 칸씩 밀린다
    //      (포트 크리프). 우선 포트가 전부 풀릴 때까지 짧게 기다린 뒤 배정한다 —
    //      진짜로 남이 점유한 경우에는 이 대기가 끝나고 그대로 대체 포트로 간다.
    for _ in 0..10 {
        if JOBS
            .iter()
            .all(|(key, _, _, _)| ports::is_port_free(ports::preferred_for(key)))
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    // 3. 호스트 포트 배정 후 신규 spawn.
    //    배정은 반드시 위의 reap 이후다 — 우리 자신의 잔여 포워드를 정리하기 전에 검사하면
    //    스스로를 충돌로 오인해 매번 대체 포트로 밀려난다.
    let mut chosen: Vec<(&'static str, u16)> = Vec::new();
    {
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        let (context, namespace) = crate::services::deploy_target::active_context();
        for (key, svc, job_ns, target_port) in JOBS {
            let host_port = ports::assign(key)?;
            chosen.push((key, host_port));
            let ns = job_ns.unwrap_or(namespace.as_str());
            let child = external_command("kubectl")?
                .args(["--context", &context, "port-forward", "-n", ns, svc])
                .arg(format!("{host_port}:{target_port}"))
                // 바인드 실패 사유를 잡아둔다 — 예전에는 상속돼 사라졌고 화면에는
                // "not responding"만 남아 원인을 알 수 없었다(D22).
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("port-forward({key}) failed to start: {e}"))?;
            guard.insert(key, child);
        }
    }

    // 4. 성공 검증: 2초 대기 후 포트별 curl 확인. 실패한 포트의 자식은 정리하고
    //    부분 실패 사유를 포트별로 명시한다.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let mut failed: Vec<(&str, u16)> = Vec::new();
    for (key, host_port) in &chosen {
        if !check_port_alive(&host_port.to_string()).await {
            failed.push((key, *host_port));
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

    // 대체 포트가 선택됐으면 조용히 넘어가지 않는다 — 사용자의 북마크나 레포에 박힌
    // `.dvc/config`는 D1 포트를 가정하고 있을 수 있다.
    let moved: Vec<String> = chosen
        .iter()
        .filter(|(key, port)| *port != ports::preferred_for(key))
        .map(|(key, port)| format!("{key}: {} → {port}", ports::preferred_for(key)))
        .collect();

    let mut msg = "Port forwarding started.".to_string();
    if !moved.is_empty() {
        msg.push_str(&format!(
            " Host port(s) in use, moved: {}.",
            moved.join(", ")
        ));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 호스트 포트의 단일 출처는 `services::ports`다. JOBS에 레지스트리에 없는 키가 생기면
    /// `assign`이 런타임에 실패하므로, 컴파일 타임 대신 여기서 잡는다.
    #[test]
    fn jobs_keys_are_registered() {
        for (key, _svc, _ns, _target) in JOBS {
            assert!(
                ports::SPECS.iter().any(|s| s.key == key),
                "JOBS key [{key}] is missing from services::ports::SPECS"
            );
        }
    }

    /// 프런트의 `PORT_FORWARD_TOTAL`(useColima.ts)이 이 배열 길이와 어긋나면 진행률이
    /// 거짓말을 한다 — 예전에 실제로 어긋났다.
    #[test]
    fn jobs_count_matches_frontend_total() {
        let ts = include_str!("../../../src/hooks/useColima.ts");
        let declared = ts
            .lines()
            .find_map(|l| l.split("PORT_FORWARD_TOTAL = ").nth(1))
            .and_then(|rest| {
                rest.trim_start()
                    .split(|c: char| !c.is_ascii_digit())
                    .find(|s| !s.is_empty())
                    .and_then(|n| n.parse::<usize>().ok())
            })
            .expect("PORT_FORWARD_TOTAL not found in useColima.ts");
        assert_eq!(declared, JOBS.len(), "PORT_FORWARD_TOTAL must match JOBS length");
    }
}
