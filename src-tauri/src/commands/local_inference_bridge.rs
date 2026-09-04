use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::AbortHandle;

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

fn allowed_private_bind_host(host: &str) -> Result<IpAddr, String> {
    let ip = host
        .parse::<IpAddr>()
        .map_err(|_| "Bridge bind_host must be a literal IP address".to_string())?;
    let allowed = match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local(),
    };
    if !allowed || ip.is_unspecified() {
        return Err(format!(
            "Refusing public/unspecified inference bridge bind address: {host}"
        ));
    }
    Ok(ip)
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

    #[test]
    fn bridge_rejects_public_or_wildcard_addresses() {
        assert!(allowed_private_bind_host("0.0.0.0").is_err());
        assert!(allowed_private_bind_host("8.8.8.8").is_err());
        assert!(allowed_private_bind_host("127.0.0.1").is_ok());
        assert!(allowed_private_bind_host("192.168.64.1").is_ok());
        assert!(allowed_private_bind_host("10.0.0.2").is_ok());
    }

    #[test]
    fn target_is_always_loopback() {
        assert_eq!(target_addr(8000), SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8000));
    }
}
