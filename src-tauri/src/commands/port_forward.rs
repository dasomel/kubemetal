use std::collections::HashMap;
use std::sync::Mutex;
use tokio::process::Child;
use tauri::State;
use crate::services::ports;
use crate::services::process::external_command;

#[derive(Default)]
pub struct PortForwardState {
    /// 대시보드가 켜고 끄는 스택 포워드. 대상은 저장된 배포 대상(D26) 하나다.
    pub jobs: Mutex<HashMap<&'static str, Child>>,
    /// kagent 패널이 보고 있는 컨텍스트별 kagent UI 포워드(컨텍스트 → (호스트 포트, 자식)).
    ///
    /// 대시보드 수명주기와 분리한 이유: kagent 패널은 클러스터를 옮겨 다니며 보는 화면이라
    /// "지금 보고 있는 클러스터의 UI"가 필요한데, 대시보드의 시작/정지 버튼이 다른 탭의
    /// 일시적 선택값에 의존하면 보이지 않는 결합이 생긴다. 배포 대상과 같은 컨텍스트면
    /// 여기에 새로 만들지 않고 `jobs`의 포워드를 재사용한다.
    pub kagent_ui: Mutex<HashMap<String, (u16, Child)>>,
}

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

/// 우리 앱이 관리하는 서비스만 대상으로 하는 pgrep 패턴의 서비스 부분. 무관한 kubectl
/// 포워드는 매칭되지 않도록 서비스명을 포함시킨다(불가침 경계).
const SERVICE_NAMES: [&str; 4] = ["svc/mlflow", "svc/seaweedfs", "svc/prefect", "svc/kagent-ui"];

/// 특정 컨텍스트를 대상으로 하는 우리 포워드만 매칭하는 pgrep 패턴.
///
/// 컨텍스트를 넣지 않으면 **다른 클러스터로 건 포워드까지 죽인다** — kagent 패널이
/// 카카오/narwhal의 UI를 보고 있는데 대시보드에서 포워딩을 켜면 그게 끊긴다.
/// 남의 클러스터를 건드리지 않는다는 경계는 여기에도 적용된다.
fn service_patterns_for(context: &str) -> Vec<String> {
    SERVICE_NAMES
        .iter()
        .map(|svc| format!("--context {context} port-forward.*{svc}"))
        .collect()
}

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
async fn reap_external_port_forwards(context: &str) -> usize {
    let mut count = 0usize;
    for pattern in service_patterns_for(context) {
        for pid in find_pids_by_pattern(&pattern).await {
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
        let mut guard = state.jobs.lock().map_err(|e| e.to_string())?;
        guard.drain().map(|(_, c)| c).collect()
    };
    for mut child in stale {
        let _ = child.kill().await;
    }

    // 2. 외부(쉘 nohup 등)에서 떠 있는 잔여 포워드를 인수 — 우리 서비스이면서
    //    **이 배포 대상 컨텍스트**인 것만. 다른 클러스터로 건 포워드는 건드리지 않는다.
    let (target_context, target_namespace) = crate::services::deploy_target::active_context();
    let reaped_external = reap_external_port_forwards(&target_context).await;
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
        let mut guard = state.jobs.lock().map_err(|e| e.to_string())?;
        let (context, namespace) = (&target_context, &target_namespace);
        for (key, svc, job_ns, target_port) in JOBS {
            let host_port = ports::assign(key)?;
            chosen.push((key, host_port));
            let ns = job_ns.unwrap_or(namespace.as_str());
            let child = external_command("kubectl")?
                .args(["--context", context, "port-forward", "-n", ns, svc])
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
            let mut guard = state.jobs.lock().map_err(|e| e.to_string())?;
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

/// kagent 패널이 보고 있는 컨텍스트의 kagent UI로 포워딩하고 그 URL을 돌려준다.
///
/// 이 커맨드가 따로 있는 이유는 D33과 같은 결함을 막기 위해서다 — 패널의 kubeconfig
/// 선택기가 진단·에이전트 토글·설치에는 반영되는데 "UI 열기"만 저장된 배포 대상을 보면,
/// 한 화면에 "어느 클러스터인가"가 두 개 생긴다(실측 2026-08-07: 카카오를 보면서 열었는데
/// colima UI가 떴다). 대시보드의 포워딩 수명주기는 배포 대상 기준으로 그대로 두고,
/// 여기서만 컨텍스트별 임시 포워드를 관리한다.
#[tauri::command]
pub async fn open_kagent_ui(
    state: State<'_, PortForwardState>,
    context: String,
) -> Result<String, String> {
    let url_of = |port: u16| format!("http://127.0.0.1:{port}");

    // 1. 배포 대상과 같은 컨텍스트고 대시보드 포워드가 살아 있으면 그것을 재사용한다.
    //    같은 클러스터에 포워드를 두 개 띄울 이유가 없다.
    let (target_context, _) = crate::services::deploy_target::active_context();
    if context == target_context {
        let has_job = state
            .jobs
            .lock()
            .map_err(|e| e.to_string())?
            .contains_key("kagent-ui");
        if has_job {
            return Ok(url_of(ports::port_for("kagent-ui")));
        }
    }

    // 2. 이 컨텍스트로 이미 띄워둔 임시 포워드가 살아 있으면 재사용, 죽었으면 버린다.
    //    (std Mutex 가드는 await를 넘길 수 없으므로 블록 안에서 끝낸다)
    let existing: Option<u16> = {
        let mut guard = state.kagent_ui.lock().map_err(|e| e.to_string())?;
        match guard.get_mut(&context) {
            Some((port, child)) => match child.try_wait() {
                Ok(None) => Some(*port), // 아직 살아 있음
                _ => {
                    guard.remove(&context);
                    None
                }
            },
            None => None,
        }
    };
    if let Some(port) = existing {
        if check_port_alive(&port.to_string()).await {
            return Ok(url_of(port));
        }
        // 프로세스는 살아 있는데 응답이 없으면 신뢰하지 않고 새로 띄운다.
        if let Ok(mut guard) = state.kagent_ui.lock() {
            guard.remove(&context);
        }
    }

    // 3. 새 포워드. 대시보드 포워드가 쓰는 포트는 `find_free_port`가 자연히 건너뛴다.
    let (preferred, range_end) = {
        let spec = ports::SPECS
            .iter()
            .find(|s| s.key == "kagent-ui")
            .ok_or("kagent-ui spec missing")?;
        (spec.preferred, spec.range_end)
    };
    let port = ports::find_free_port(preferred, range_end)?;

    let child = external_command("kubectl")?
        .args(["--context", &context, "port-forward", "-n", "kagent", "svc/kagent-ui"])
        .arg(format!("{port}:8080"))
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("kagent UI port-forward({context}) failed to start: {e}"))?;

    {
        let mut guard = state.kagent_ui.lock().map_err(|e| e.to_string())?;
        guard.insert(context.clone(), (port, child));
    }

    // 기동 확인 — 응답하지 않으면 자식을 정리하고 실패를 그대로 올린다(D22).
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    if !check_port_alive(&port.to_string()).await {
        let dead = state
            .kagent_ui
            .lock()
            .map_err(|e| e.to_string())?
            .remove(&context);
        if let Some((_, mut child)) = dead {
            let _ = child.kill().await;
        }
        return Err(format!(
            "kagent UI on context [{context}] is not responding at :{port}. \
             Verify kagent is installed there (kubectl --context {context} get svc kagent-ui -n kagent)."
        ));
    }

    Ok(url_of(port))
}

#[tauri::command]
pub async fn stop_port_forward(state: State<'_, PortForwardState>) -> Result<String, String> {
    let children: Vec<_> = {
        let mut guard = state.jobs.lock().map_err(|e| e.to_string())?;
        guard.drain().map(|(_, child)| child).collect()
    };
    // kagent 패널이 다른 컨텍스트로 띄운 임시 포워드도 함께 정리한다 — 수명주기는 분리돼
    // 있어도 "정지"는 앱이 띄운 것을 남기지 않는다는 뜻이어야 한다. 남겨두면 세션 사이로
    // 새어 나가고, 그건 원격 클러스터로 향한 프로세스라 더 나쁘다.
    let ad_hoc: Vec<Child> = {
        let mut guard = state.kagent_ui.lock().map_err(|e| e.to_string())?;
        guard.drain().map(|(_, (_, child))| child).collect()
    };
    let tracked_count = children.len() + ad_hoc.len();
    for mut child in children.into_iter().chain(ad_hoc) {
        let _ = child.kill().await;
    }

    // "정지는 항상 통한다" — 우리가 추적하지 못한 외부 매칭 포워드도 함께 정리하되,
    // 배포 대상 컨텍스트로 한정한다(kagent 패널이 다른 클러스터로 띄운 UI 포워드는 별도 관리).
    let (target_context, _) = crate::services::deploy_target::active_context();
    let reaped_external = reap_external_port_forwards(&target_context).await;

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
