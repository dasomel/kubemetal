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

// IPC 타입은 프로젝트 규약대로 snake_case를 유지한다(`src/types/ipc.ts` 상단 주석).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BridgeState {
    /// base 매니페스트의 `host.lima.internal`을 그대로 쓴다. colima 전용 —
    /// 2026-07-20 CoreDNS 실측(→192.168.5.2)으로 확인된 값이다(D10).
    KeepBase,
    /// 클러스터 **내부에서** 실제로 도달을 확인한 주소만 여기 들어온다.
    Verified { host: String },
    /// 후보는 뽑았지만 검증에 실패했거나 아직 검증하지 않았다. 이 상태로는 배포하지 않는다 —
    /// 추측 주소를 실으면 파드가 조용히 죽는다(D22–D25).
    ///
    /// `reason_code`는 프런트가 i18n 테이블로 매핑하는 안정 코드다(D31) — 언어별 문장은
    /// 여기서 만들지 않는다. IP·개수 등 가변 값은 `detail`에 언어중립으로 담는다.
    /// 알려진 코드: `not_probed`(아직 탐지 전), `no_interface_candidates`(노드 서브넷과
    /// 겹치는 호스트 인터페이스 없음), `unreachable_from_cluster`(후보는 있으나 클러스터
    /// 내부에서 도달 실패 — detail에 후보별 실패 사유).
    Unverified {
        candidates: Vec<String>,
        reason_code: String,
        detail: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum IntegrationLevel {
    AgentOnly,
    FullStack,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeployTarget {
    pub context: String,
    pub namespace: String,
    /// None이면 대상 클러스터의 기본 StorageClass를 따른다(colima=local-path,
    /// narwhal=nfs-csi 모두 default 지정됨 — 2026-07-26 실측).
    pub storage_class: Option<String>,
    /// 사내 레지스트리/미러. docker.io 이미지만 여기로 재지정된다.
    pub image_registry: Option<String>,
    pub bridge: BridgeState,
    /// None=미지정 → 파생 기본값. 기존 deploy-target.json(필드 없음)은 None으로 역직렬화된다.
    #[serde(default)]
    pub integration_level: Option<IntegrationLevel>,
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
                integration_level: None,
            }
        } else {
            Self {
                context: context.to_string(),
                namespace: DEFAULT_EXTERNAL_NAMESPACE.into(),
                storage_class: None,
                image_registry: None,
                bridge: BridgeState::Unverified {
                    candidates: Vec::new(),
                    reason_code: "not_probed".into(),
                    detail: None,
                },
                integration_level: None,
            }
        }
    }

    pub fn is_colima(&self) -> bool {
        self.context == COLIMA_CONTEXT
    }

    /// D30: 자체 k3s가 스택의 정식 거처, 외부 기본은 에이전트 온리.
    pub fn effective_integration_level(&self) -> IntegrationLevel {
        match self.integration_level {
            Some(level) => level,
            None => {
                if self.is_colima() {
                    IntegrationLevel::FullStack
                } else {
                    IntegrationLevel::AgentOnly
                }
            }
        }
    }

    pub fn full_stack_gate(&self) -> Result<(), String> {
        if !self.is_colima() && self.effective_integration_level() == IntegrationLevel::AgentOnly {
            Err("External clusters default to agent-only integration (D30) — select L2 full-stack \
                 explicitly on the deploy target card and save before provisioning the full stack"
                .into())
        } else {
            Ok(())
        }
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
            BridgeState::Unverified {
                candidates,
                reason_code,
                detail,
            } => {
                return Err(format!(
                    "Deploy refused: host bridge address is not verified (reason: {reason_code}\
                     {}, candidates: {}). Run host bridge detection first.",
                    detail
                        .as_deref()
                        .map(|d| format!(" — {d}"))
                        .unwrap_or_default(),
                    if candidates.is_empty() {
                        "none".to_string()
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

/// 임의 컨텍스트의 MLOps 스택 네임스페이스.
///
/// 화면의 kubeconfig 선택기(드롭다운)가 축인 호출부를 위한 것이다 — `active_context()`는
/// **저장된** 대상을 돌려주므로, 사용자가 드롭다운에서 다른 클러스터를 고른 상태에서
/// 그것을 쓰면 엉뚱한 클러스터의 네임스페이스가 섞인다(D33이 `install_kagent`에서 겪은
/// 것과 같은 축 혼동).
///
/// 저장된 대상과 컨텍스트가 같으면 그쪽 네임스페이스를 쓴다 — 사용자가 커스텀
/// 네임스페이스를 설정했을 수 있고, 그 경우 기본값으로 되돌리면 안 된다.
pub fn namespace_for_context(context: &str) -> String {
    let (active_ctx, active_ns) = active_context();
    if active_ctx == context {
        active_ns
    } else {
        DeployTarget::for_context(context).namespace
    }
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
            "must pick the host interface address on the same subnet as the node"
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
            "must not invent a candidate when there is no connecting interface"
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
        let err = t
            .render_args()
            .expect_err("an unverified bridge must refuse render");
        assert!(
            err.contains("not verified") && err.contains("not_probed"),
            "reason code must surface: {err}"
        );
    }

    /// 진단이 조회할 네임스페이스는 화면 드롭다운의 컨텍스트에서 나와야 한다.
    /// "default" 고정이었을 때는 외부 클러스터의 스택 파드를 한 건도 못 보고
    /// "0 pod(s)"를 정상처럼 보고했다(D26/D30 L2).
    #[test]
    fn namespace_for_context_follows_the_requested_context() {
        // 저장된 대상과 무관한 컨텍스트는 그 컨텍스트의 기본 네임스페이스를 쓴다.
        assert_eq!(namespace_for_context(COLIMA_CONTEXT), "default");
        assert_eq!(namespace_for_context("some-external"), DEFAULT_EXTERNAL_NAMESPACE);

        // 저장된 대상과 컨텍스트가 같으면 저장된 네임스페이스를 존중한다 —
        // 사용자가 커스텀 네임스페이스를 골랐을 수 있다.
        let mut custom = DeployTarget::for_context("some-external");
        custom.namespace = "team-ml".into();
        set_active(&custom);
        assert_eq!(namespace_for_context("some-external"), "team-ml");
        // 다른 컨텍스트에는 그 값이 새지 않아야 한다.
        assert_eq!(namespace_for_context("other-cluster"), DEFAULT_EXTERNAL_NAMESPACE);

        // 전역 상태를 원복해 다른 테스트에 영향을 주지 않는다.
        set_active(&DeployTarget::for_context(COLIMA_CONTEXT));
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

    #[test]
    fn full_stack_gate_passes_for_colima_default() {
        let t = DeployTarget::for_context(COLIMA_CONTEXT);
        assert_eq!(t.effective_integration_level(), IntegrationLevel::FullStack);
        assert!(t.full_stack_gate().is_ok());
    }

    #[test]
    fn full_stack_gate_rejects_external_none() {
        let t = DeployTarget::for_context("narwhal");
        assert_eq!(t.effective_integration_level(), IntegrationLevel::AgentOnly);
        let err = t.full_stack_gate().expect_err("external + None must be blocked");
        assert!(err.contains("D30"), "D30 message must be included: {err}");
    }

    #[test]
    fn full_stack_gate_passes_external_full_stack() {
        let mut t = DeployTarget::for_context("narwhal");
        t.integration_level = Some(IntegrationLevel::FullStack);
        assert_eq!(t.effective_integration_level(), IntegrationLevel::FullStack);
        assert!(t.full_stack_gate().is_ok());
    }

    #[test]
    fn deserializes_legacy_json_without_integration_level_to_none() {
        let json = r#"{
            "context": "narwhal",
            "namespace": "kubemetal",
            "storage_class": null,
            "image_registry": null,
            "bridge": {
                "kind": "unverified",
                "candidates": [],
                "reason_code": "not_probed",
                "detail": null
            }
        }"#;
        let t: DeployTarget = serde_json::from_str(json).expect("deserialization should succeed");
        assert_eq!(t.integration_level, None);
        assert_eq!(t.effective_integration_level(), IntegrationLevel::AgentOnly);
    }
}
