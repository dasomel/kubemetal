use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::AbortHandle;

use crate::services::deploy_target::active_verified_bridge_host;

#[derive(Debug, Clone, Deserialize)]
pub struct LocalInferenceBridgeConfig {
    pub bind_host: String,
    pub bind_port: u16,
    pub target_port: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalInferenceBridgeStatus {
    pub running: bool,
    pub bind_address: Option<String>,
    pub target_address: Option<String>,
}

struct BridgeTask {
    abort: AbortHandle,
    bind_address: String,
    target_address: String,
}

#[derive(Default)]
pub struct LocalInferenceBridgeState {
    task: Mutex<Option<BridgeTask>>,
}

/// 릴레이 바인드 주소 가드(finding 5). 어떤 private/link-local 주소든 받아주면, 인증 없는
/// 릴레이가 colima vz 브리지가 아니라 사용자의 실제 LAN 인터페이스(예: 사무실 Wi-Fi)에 뜰 수
/// 있다 — `192.168.x.x`는 vmnet 서브넷과 사무실 LAN 서브넷을 구분해 주지 않는다.
///
/// 그래서 loopback을 넘어서는 바인드는 D26 배포 대상의 `BridgeState::Verified { host }` —
/// 클러스터 **안에서** 실측 도달을 확인한 그 주소 — 와 정확히 일치할 때만 허용한다
/// (`active_verified_bridge_host()`, `services/deploy_target.rs`). colima의 기본 브리지는
/// DNS 이름(`host.lima.internal`) 기반이라 이 코드베이스에 검증된 숫자 주소가 없고, 추측
/// 주소를 릴레이에 실어 보내지 않는다는 원칙(D22–D25, docs/11 "do not infer or guess a
/// reachable host address")은 colima에도 그대로 적용된다 — 현재는 loopback만 허용된다.
fn allowed_private_bind_host(host: &str) -> Result<IpAddr, String> {
    let ip = host
        .parse::<IpAddr>()
        .map_err(|_| "Bridge bind_host must be a literal IP address".to_string())?;
    if ip.is_loopback() {
        return Ok(ip);
    }
    if let Some(verified) = active_verified_bridge_host() {
        if verified.parse::<IpAddr>().as_ref() == Ok(&ip) {
            return Ok(ip);
        }
    }
    Err(format!(
        "Refusing inference bridge bind address {host}: it is not loopback and does not match \
         the deploy target's verified host bridge address. Run host bridge detection (D10) \
         first, or bind to a loopback address."
    ))
}

fn target_addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

async fn relay_connection(mut inbound: TcpStream, target: SocketAddr) -> Result<(), String> {
    let mut outbound = TcpStream::connect(target)
        .await
        .map_err(|e| format!("Failed to connect to loopback inference target {target}: {e}"))?;
    copy_bidirectional(&mut inbound, &mut outbound)
        .await
        .map_err(|e| format!("Inference bridge relay failed: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn get_local_inference_bridge_status(
    state: tauri::State<'_, LocalInferenceBridgeState>,
) -> Result<LocalInferenceBridgeStatus, String> {
    let guard = state
        .task
        .lock()
        .map_err(|_| "Local inference bridge state lock poisoned".to_string())?;
    Ok(match guard.as_ref() {
        Some(task) if !task.abort.is_finished() => LocalInferenceBridgeStatus {
            running: true,
            bind_address: Some(task.bind_address.clone()),
            target_address: Some(task.target_address.clone()),
        },
        _ => LocalInferenceBridgeStatus {
            running: false,
            bind_address: None,
            target_address: None,
        },
    })
}

#[tauri::command]
pub async fn start_local_inference_bridge(
    config: LocalInferenceBridgeConfig,
    state: tauri::State<'_, LocalInferenceBridgeState>,
) -> Result<LocalInferenceBridgeStatus, String> {
    if config.bind_port == 0 || config.target_port == 0 {
        return Err("Bridge ports must be non-zero".into());
    }
    let bind_ip = allowed_private_bind_host(&config.bind_host)?;
    let bind = SocketAddr::new(bind_ip, config.bind_port);
    let target = target_addr(config.target_port);

    {
        let guard = state
            .task
            .lock()
            .map_err(|_| "Local inference bridge state lock poisoned".to_string())?;
        if let Some(task) = guard.as_ref() {
            if !task.abort.is_finished() {
                return Err(format!(
                    "A local inference bridge is already running at {}",
                    task.bind_address
                ));
            }
        }
    }

    // Bind before spawning so port/address mistakes are returned synchronously to the UI.
    let listener = TcpListener::bind(bind)
        .await
        .map_err(|e| format!("Failed to bind inference bridge at {bind}: {e}"))?;
    let task = tokio::spawn(async move {
        loop {
            let Ok((inbound, peer)) = listener.accept().await else {
                break;
            };
            // The listener itself is restricted to a private/loopback host address. Keep the
            // relay protocol-agnostic so OpenAI SSE/streaming and Anthropic streaming work too.
            tokio::spawn(async move {
                if let Err(error) = relay_connection(inbound, target).await {
                    eprintln!("local inference bridge peer {peer}: {error}");
                }
            });
        }
    });
    let abort = task.abort_handle();
    // Detach; abort_handle is retained as the lifecycle control.
    drop(task);

    let status = LocalInferenceBridgeStatus {
        running: true,
        bind_address: Some(bind.to_string()),
        target_address: Some(target.to_string()),
    };
    let mut guard = state
        .task
        .lock()
        .map_err(|_| "Local inference bridge state lock poisoned".to_string())?;
    *guard = Some(BridgeTask {
        abort,
        bind_address: bind.to_string(),
        target_address: target.to_string(),
    });
    Ok(status)
}

#[tauri::command]
pub async fn stop_local_inference_bridge(
    state: tauri::State<'_, LocalInferenceBridgeState>,
) -> Result<LocalInferenceBridgeStatus, String> {
    let mut guard = state
        .task
        .lock()
        .map_err(|_| "Local inference bridge state lock poisoned".to_string())?;
    let task = guard
        .take()
        .ok_or_else(|| "No KubeMetal-managed local inference bridge is running".to_string())?;
    task.abort.abort();
    Ok(LocalInferenceBridgeStatus {
        running: false,
        bind_address: None,
        target_address: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::deploy_target::{
        set_active, BridgeState, DeployTarget, ACTIVE_TARGET_TEST_LOCK, COLIMA_CONTEXT,
    };

    /// 전역 `ACTIVE_BRIDGE` 캐시를 조작하는 테스트라 다른 모듈의 같은 계열 테스트
    /// (`services::deploy_target::tests`)와도 레이스한다 — 크레이트 전역 락으로 상호
    /// 배제하고, 두 시나리오는 한 테스트 함수에 몰아 이 테스트 안에서의 순서도 보장한다.
    #[test]
    fn bridge_bind_host_is_gated_by_the_active_verified_bridge() {
        let _guard = ACTIVE_TARGET_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_active(&DeployTarget::for_context(COLIMA_CONTEXT));
        assert!(allowed_private_bind_host("0.0.0.0").is_err());
        assert!(allowed_private_bind_host("8.8.8.8").is_err());
        assert!(allowed_private_bind_host("127.0.0.1").is_ok());
        // colima's default bridge (BridgeState::KeepBase) has no stored numeric address —
        // no private address is trusted without D10 verification.
        assert!(allowed_private_bind_host("192.168.64.1").is_err());
        assert!(allowed_private_bind_host("10.0.0.2").is_err());

        let mut target = DeployTarget::for_context("narwhal");
        target.bridge = BridgeState::Verified {
            host: "192.168.64.1".into(),
        };
        set_active(&target);
        assert!(allowed_private_bind_host("192.168.64.1").is_ok());
        // A different private address is rejected even though it is also private —
        // the address must match the verified bridge exactly, not just be private.
        assert!(allowed_private_bind_host("192.168.1.10").is_err());

        set_active(&DeployTarget::for_context(COLIMA_CONTEXT));
    }

    #[test]
    fn target_is_always_loopback() {
        assert_eq!(target_addr(8000), SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8000));
    }
}
