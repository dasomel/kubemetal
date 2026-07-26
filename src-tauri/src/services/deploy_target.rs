//! 배포 대상(DeployTarget) — "colima를 쓸지"가 아니라 "어느 클러스터에 배포할지"를
//! 1급 개념으로 만든다. colima k3s는 그중 한 경우일 뿐이다.
//!
//! 이 모듈이 존재하는 이유: 매니페스트와 kubectl 호출이 colima 한 곳을 하드코딩하고
//! 있었다(`--context colima` 6곳, ns=default, ExternalName=host.lima.internal).
//! 외부 클러스터에 배포하려면 이 값들이 대상마다 달라져야 한다.

use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

/// colima 컨텍스트만 특별 취급한다 — 수명주기(start/stop)를 이 앱이 소유하는 유일한 대상이고,
/// D10 브리지 주소가 실측으로 확정된 유일한 대상이다.
pub const COLIMA_CONTEXT: &str = "colima";

/// 외부 클러스터의 기본 네임스페이스. `default`를 쓰지 않는다 — 공유 IDP 클러스터의
/// default는 남의 영역이고, prune/삭제 사고의 반경이 너무 넓다.
pub const DEFAULT_EXTERNAL_NAMESPACE: &str = "kubemetal";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BridgeState {
    /// base 매니페스트의 `host.lima.internal`을 그대로 쓴다. colima 전용 —
    /// 2026-07-20 CoreDNS 실측(→192.168.5.2)으로 확인된 값이다(D10).
    KeepBase,
    /// 클러스터 **내부에서** 실제로 도달을 확인한 주소만 여기 들어온다.
    Verified { host: String },
    /// 후보는 뽑았지만 검증에 실패했거나 아직 검증하지 않았다. 이 상태로는 배포하지 않는다 —
    /// 추측 주소를 실으면 파드가 조용히 죽는다(D22–D25).
    Unverified {
        candidates: Vec<String>,
        reason: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeployTarget {
    pub context: String,
    pub namespace: String,
    /// None이면 대상 클러스터의 기본 StorageClass를 따른다(colima=local-path,
    /// narwhal=nfs-csi 모두 default 지정됨 — 2026-07-26 실측).
    pub storage_class: Option<String>,
    /// 사내 레지스트리/미러. docker.io 이미지만 여기로 재지정된다.
    pub image_registry: Option<String>,
    pub bridge: BridgeState,
}

impl DeployTarget {
    pub fn for_context(context: &str) -> Self {
        if context == COLIMA_CONTEXT {
            Self {
                context: context.to_string(),
                namespace: "default".into(),
                storage_class: None,
                image_registry: None,
                bridge: BridgeState::KeepBase,
            }
        } else {
            Self {
                context: context.to_string(),
                namespace: DEFAULT_EXTERNAL_NAMESPACE.into(),
                storage_class: None,
                image_registry: None,
                bridge: BridgeState::Unverified {
                    candidates: Vec::new(),
                    reason: "브리지 주소를 아직 탐지·검증하지 않았습니다.".into(),
                },
            }
        }
    }

    pub fn is_colima(&self) -> bool {
        self.context == COLIMA_CONTEXT
    }

    /// `render.sh`에 넘길 인자. 브리지가 미검증이면 인자를 만들지 않고 거부한다 —
    /// 렌더 단계에서 막아야 추측값이 클러스터까지 가지 않는다.
    pub fn render_args(&self) -> Result<Vec<String>, String> {
        let mut args = vec!["--namespace".to_string(), self.namespace.clone()];

        match &self.bridge {
            BridgeState::KeepBase => args.push("--keep-bridge".into()),
            BridgeState::Verified { host } => {
                args.push("--bridge-host".into());
                args.push(host.clone());
            }
            BridgeState::Unverified { candidates, reason } => {
                return Err(format!(
                    "호스트 브리지 주소가 검증되지 않아 배포를 중단합니다. 사유: {reason} \
                     (후보: {}). `호스트 브리지 탐지`를 먼저 실행하세요.",
                    if candidates.is_empty() {
                        "없음".to_string()
                    } else {
                        candidates.join(", ")
                    }
                ));
            }
        }

        if let Some(sc) = &self.storage_class {
            args.push("--storage-class".into());
            args.push(sc.clone());
        }
        if let Some(reg) = &self.image_registry {
            args.push("--image-registry".into());
            args.push(reg.clone());
        }
        Ok(args)
    }
}

/// 활성 대상의 (context, namespace) 캐시.
///
/// 상태 조회·포트포워드·크리덴셜 조회는 AppHandle 없이 호출되는 경로가 섞여 있어,
/// 매 호출마다 설정 파일을 읽는 대신 선택 시점에 여기 반영한다. 초기값은 colima —
/// 대상을 고른 적 없는 기존 사용자의 동작이 그대로 유지된다.
static ACTIVE: std::sync::RwLock<Option<(String, String)>> = std::sync::RwLock::new(None);

pub fn set_active(target: &DeployTarget) {
    if let Ok(mut guard) = ACTIVE.write() {
        *guard = Some((target.context.clone(), target.namespace.clone()));
    }
}

/// 활성 대상의 (컨텍스트, 네임스페이스). kubectl 인자로 바로 쓴다.
pub fn active_context() -> (String, String) {
    ACTIVE
        .read()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_else(|| (COLIMA_CONTEXT.to_string(), "default".to_string()))
}

/// 호스트의 IPv4 인터페이스 하나.
#[derive(Debug, Clone, PartialEq)]
pub struct HostIface {
    pub name: String,
    pub addr: Ipv4Addr,
    pub mask: Ipv4Addr,
}

/// `ifconfig -a` 출력에서 IPv4 인터페이스를 뽑는다.
///
/// 순수 함수로 둔 이유: 이 파싱이 D10 브리지 후보의 유일한 근거라 픽스처로 검증할 수 있어야
/// 한다. `route -n get <nodeIP>`는 쓰지 않는다 — 실측 2026-07-26, host-only 인터페이스가
/// 내려가 있을 때 기본 경로로 조용히 폴백해 **LAN 공유기 주소(192.168.75.1)를 호스트 주소로
/// 반환했다**. 게이트웨이는 호스트 자신이 아니다.
pub fn parse_ifconfig(text: &str) -> Vec<HostIface> {
    let mut out = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        if !line.starts_with(char::is_whitespace) {
            if let Some((name, _)) = line.split_once(':') {
                current = name.to_string();
            }
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.first() != Some(&"inet") {
            continue;
        }
        let (Some(addr), Some(mask)) = (fields.get(1), netmask_field(&fields)) else {
            continue;
        };
        let (Ok(addr), Some(mask)) = (addr.parse::<Ipv4Addr>(), parse_mask(mask)) else {
            continue;
        };
        out.push(HostIface {
            name: current.clone(),
            addr,
            mask,
        });
    }
    out
}

fn netmask_field<'a>(fields: &[&'a str]) -> Option<&'a str> {
    let idx = fields.iter().position(|f| *f == "netmask")?;
    fields.get(idx + 1).copied()
}

/// macOS는 넷마스크를 `0xffffff00` 형태로 낸다. 점표기도 받아둔다.
fn parse_mask(raw: &str) -> Option<Ipv4Addr> {
    if let Some(hex) = raw.strip_prefix("0x") {
        return u32::from_str_radix(hex, 16).ok().map(Ipv4Addr::from);
    }
    raw.parse::<Ipv4Addr>().ok()
}

fn same_subnet(iface: &HostIface, node: Ipv4Addr) -> bool {
    let mask = u32::from(iface.mask);
    (u32::from(iface.addr) & mask) == (u32::from(node) & mask)
}

/// 클러스터 노드 IP와 같은 서브넷에 있는 **호스트 인터페이스의 자기 주소**를 후보로 낸다.
/// 루프백은 제외한다 — 클러스터에서 도달할 수 없다.
///
/// 후보가 비어 있으면 그게 정답이다: 호스트와 노드를 잇는 인터페이스가 없다는 뜻이므로
/// 폴백으로 아무 주소나 만들어내지 않는다.
pub fn bridge_candidates(ifaces: &[HostIface], node_ips: &[Ipv4Addr]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for iface in ifaces {
        if iface.addr.is_loopback() {
            continue;
        }
        if node_ips.iter().any(|node| same_subnet(iface, *node)) {
            let candidate = iface.addr.to_string();
            if !out.contains(&candidate) {
                out.push(candidate);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-07-26 이 개발기에서 narwhal 클러스터가 떠 있을 때의 실제 `ifconfig -a` 발췌.
    const IFCONFIG_CLUSTER_UP: &str = "\
lo0: flags=8049<UP,LOOPBACK>
\tinet 127.0.0.1 netmask 0xff000000
en0: flags=8863<UP,BROADCAST>
\tinet 192.168.75.57 netmask 0xffffff00 broadcast 192.168.75.255
bridge102: flags=8863<UP,BROADCAST>
\tinet 192.168.56.1 netmask 0xffffff00 broadcast 192.168.56.255
";

    /// 같은 개발기에서 클러스터가 내려가 있을 때 — host-only 인터페이스 자체가 없다.
    const IFCONFIG_CLUSTER_DOWN: &str = "\
lo0: flags=8049<UP,LOOPBACK>
\tinet 127.0.0.1 netmask 0xff000000
en0: flags=8863<UP,BROADCAST>
\tinet 192.168.75.57 netmask 0xffffff00 broadcast 192.168.75.255
";

    fn narwhal_nodes() -> Vec<Ipv4Addr> {
        // 실측: masters .10-.12, workers .21-.23
        vec![
            "192.168.56.10".parse().unwrap(),
            "192.168.56.21".parse().unwrap(),
        ]
    }

    #[test]
    fn parses_macos_hex_netmask() {
        let ifaces = parse_ifconfig(IFCONFIG_CLUSTER_UP);
        let bridge = ifaces.iter().find(|i| i.name == "bridge102").expect("bridge102");
        assert_eq!(bridge.addr, "192.168.56.1".parse::<Ipv4Addr>().unwrap());
        assert_eq!(bridge.mask, "255.255.255.0".parse::<Ipv4Addr>().unwrap());
    }

    #[test]
    fn finds_host_address_on_node_subnet() {
        let ifaces = parse_ifconfig(IFCONFIG_CLUSTER_UP);
        assert_eq!(
            bridge_candidates(&ifaces, &narwhal_nodes()),
            vec!["192.168.56.1".to_string()],
            "노드와 같은 서브넷에 있는 호스트 인터페이스 주소를 골라야 한다"
        );
    }

    /// 이 테스트가 회귀 방지의 핵심이다. 인터페이스가 없으면 후보도 없어야 한다 —
    /// `route -n get`을 쓰던 초안은 여기서 LAN 공유기 주소를 뱉었고, 그 값이 그대로
    /// ExternalName에 실릴 뻔했다.
    #[test]
    fn no_candidate_when_no_interface_on_node_subnet() {
        let ifaces = parse_ifconfig(IFCONFIG_CLUSTER_DOWN);
        assert!(
            bridge_candidates(&ifaces, &narwhal_nodes()).is_empty(),
            "연결 인터페이스가 없으면 후보를 만들어내면 안 된다"
        );
    }

    #[test]
    fn loopback_is_never_a_candidate() {
        let ifaces = parse_ifconfig(IFCONFIG_CLUSTER_UP);
        let nodes = vec!["127.0.0.5".parse().unwrap()];
        assert!(bridge_candidates(&ifaces, &nodes).is_empty());
    }

    #[test]
    fn colima_target_keeps_verified_d10_defaults() {
        let t = DeployTarget::for_context(COLIMA_CONTEXT);
        assert_eq!(t.namespace, "default");
        assert_eq!(t.bridge, BridgeState::KeepBase);
        assert_eq!(
            t.render_args().unwrap(),
            vec!["--namespace", "default", "--keep-bridge"]
        );
    }

    #[test]
    fn external_target_defaults_to_own_namespace_and_refuses_render() {
        let t = DeployTarget::for_context("narwhal");
        assert_eq!(t.namespace, DEFAULT_EXTERNAL_NAMESPACE);
        let err = t.render_args().expect_err("미검증 브리지는 렌더를 거부해야 한다");
        assert!(err.contains("검증되지 않아"), "사유가 드러나야 한다: {err}");
    }

    #[test]
    fn verified_bridge_renders_with_host() {
        let mut t = DeployTarget::for_context("narwhal");
        t.bridge = BridgeState::Verified {
            host: "192.168.56.1".into(),
        };
        t.storage_class = Some("nfs-csi".into());
        let args = t.render_args().unwrap();
        assert_eq!(
            args,
            vec![
                "--namespace",
                "kubemetal",
                "--bridge-host",
                "192.168.56.1",
                "--storage-class",
                "nfs-csi"
            ]
        );
    }
}
