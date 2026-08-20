//! 호스트 포트의 탐지와 런타임 배정.
//!
//! D1 표의 숫자는 **우선 시도값이지 보장이 아니다**. 무관한 로컬 프로세스가 같은 포트를
//! 점유하고 있을 수 있으므로(실측 2026-08-06: Docker 컨테이너가 5001·8080을 잡고 있었다),
//! 기동 시점에 비어 있는 포트를 골라 여기에 기록하고 모든 소비자가 그것을 읽는다.
//!
//! 접근은 `deploy_target::active_context()`와 같은 자유 함수 패턴이다 — `tauri::State`가
//! 닿지 않는 곳(상수를 대체하는 URL 빌더들)에서도 읽어야 하기 때문이다.

use std::sync::atomic::{AtomicU16, Ordering};

/// 서비스 하나의 호스트 포트 규격.
pub struct PortSpec {
    pub key: &'static str,
    /// D1이 지정한 우선 시도 포트.
    pub preferred: u16,
    /// 우선 포트가 막혔을 때 순회할 상한(하한은 `preferred`).
    pub range_end: u16,
}

/// D1 호스트 포트의 단일 출처. `port_forward.rs::JOBS`의 키는 여기 있는 키여야 하며,
/// 그 일치는 `jobs_keys_are_registered` 테스트가 고정한다.
///
/// `serving`은 포워딩이 아니라 호스트 프로세스지만, 범위(8080~8099)를 여기서 함께
/// 소유해야 `suggest_serving_port`와 실제 기동 경로가 같은 값을 본다.
pub const SPECS: [PortSpec; 6] = [
    PortSpec { key: "mlflow", preferred: 5001, range_end: 5010 },
    PortSpec { key: "seaweedfs-s3", preferred: 8333, range_end: 8343 },
    PortSpec { key: "seaweedfs-filer", preferred: 8888, range_end: 8898 },
    PortSpec { key: "prefect", preferred: 4200, range_end: 4210 },
    PortSpec { key: "kagent-ui", preferred: 8090, range_end: 8099 },
    PortSpec { key: "serving", preferred: 8080, range_end: 8099 },
];

/// 배정된 실제 포트. 0은 "아직 배정 안 됨"이며 그때는 `preferred`를 돌려준다 —
/// 포워딩을 켜기 전에도 기존과 같은 URL이 보이도록 하기 위함이다.
// 배열 초기화용 상수. clippy의 interior-mutability 경고는 여기서만 의도적으로 허용한다 —
// `[AtomicU16::new(0); N]`을 쓰려면 이 형태가 유일한 방법이고, 이 상수는 배열을 채우는
// 순간에만 쓰인다(복사본이 만들어지는 것이 정확히 원하는 동작이다).
#[allow(clippy::declare_interior_mutable_const)]
const UNASSIGNED: AtomicU16 = AtomicU16::new(0);
static ASSIGNED: [AtomicU16; SPECS.len()] = [UNASSIGNED; SPECS.len()];

fn index_of(key: &str) -> Option<usize> {
    SPECS.iter().position(|s| s.key == key)
}

/// 포트가 비어 있는가. **`0.0.0.0`과 `127.0.0.1` 양쪽**을 확인한다.
///
/// 루프백만 확인하면 와일드카드(`*:포트`)로 바인딩한 프로세스를 놓친다 — Docker가 정확히
/// 그렇게 바인딩하고, 더 구체적인 리스너가 없는 동안 IPv4 루프백 연결까지 받아간다.
/// 그 창에서 우리 포트를 호출하면 남의 프로세스가 응답한다(실측: `404 page not found`).
pub fn is_port_free(port: u16) -> bool {
    for addr in [[0, 0, 0, 0], [127, 0, 0, 1]] {
        match std::net::TcpListener::bind((std::net::Ipv4Addr::from(addr), port)) {
            Ok(listener) => drop(listener),
            Err(_) => return false,
        }
    }
    true
}

/// `preferred`를 먼저 시도하고, 막혀 있으면 `preferred+1..=range_end`를 순회한다.
///
/// 바인딩을 잡아두지 않고 즉시 놓으므로 TOCTOU 창이 남는다 — 호출자는 이 값을 실제로
/// 여는 쪽(kubectl/서버 프로세스)에 바로 넘겨야 하고, 그 기동이 실패하면 그대로 보고한다.
pub fn find_free_port(preferred: u16, range_end: u16) -> Result<u16, String> {
    for port in preferred..=range_end.max(preferred) {
        if is_port_free(port) {
            return Ok(port);
        }
    }
    Err(format!(
        "No free host port in {preferred}-{range_end}. Free one of them or stop the process holding it."
    ))
}

/// 규격의 우선 포트부터 비어 있는 포트를 찾아 배정하고 기록한다.
pub fn assign(key: &str) -> Result<u16, String> {
    let idx = index_of(key).ok_or_else(|| format!("Unknown port key: {key}"))?;
    let spec = &SPECS[idx];
    let port = find_free_port(spec.preferred, spec.range_end)?;
    ASSIGNED[idx].store(port, Ordering::Relaxed);
    Ok(port)
}

/// 외부에서 정해진 포트를 기록한다(서빙처럼 사용자가 지정할 수 있는 경우).
pub fn set_assigned(key: &str, port: u16) {
    if let Some(idx) = index_of(key) {
        ASSIGNED[idx].store(port, Ordering::Relaxed);
    }
}

/// 이 서비스가 실제로 쓰는 호스트 포트. 배정 전이면 D1 우선값.
pub fn port_for(key: &str) -> u16 {
    match index_of(key) {
        Some(idx) => match ASSIGNED[idx].load(Ordering::Relaxed) {
            0 => SPECS[idx].preferred,
            p => p,
        },
        None => 0,
    }
}

/// D1이 지정한 우선 포트(배정 결과와 무관). 대체 포트가 선택됐는지 판단할 때 쓴다.
pub fn preferred_for(key: &str) -> u16 {
    index_of(key).map(|i| SPECS[i].preferred).unwrap_or(0)
}

/// `http://127.0.0.1:{실제포트}` — 로컬 URL은 항상 `127.0.0.1`이다(`localhost`는 macOS에서
/// `::1`로 먼저 풀리고, 와일드카드로 바인딩한 남의 프로세스와 만난다).
pub fn local_url(key: &str) -> String {
    format!("http://127.0.0.1:{}", port_for(key))
}

/// 현재 배정된 호스트 포트 전부. 프런트가 링크 URL을 만들 때 쓴다 — 컴포넌트에 숫자를
/// 박아두면 대체 포트가 선택됐을 때 링크만 조용히 옛 곳을 가리킨다.
#[tauri::command]
pub async fn get_host_ports() -> Result<std::collections::HashMap<String, u16>, String> {
    Ok(SPECS
        .iter()
        .map(|s| (s.key.to_string(), port_for(s.key)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 이 테스트가 이 파일의 존재 이유다. 와일드카드로 잡힌 포트를 `127.0.0.1`만 보는
    /// 방식은 "비어 있음"으로 오판한다 — 그게 2026-08-06에 실제로 일어난 일이다.
    #[test]
    fn is_port_free_detects_wildcard_holder() {
        let wildcard = std::net::TcpListener::bind(("0.0.0.0", 0)).expect("bind wildcard");
        let port = wildcard.local_addr().unwrap().port();

        // 루프백만 보는 옛 방식은 이 포트를 비었다고 답한다 — 대조군.
        assert!(
            std::net::TcpListener::bind(("127.0.0.1", port)).is_ok(),
            "loopback-only probe should still succeed — that is the flaw being guarded"
        );
        assert!(!is_port_free(port), "wildcard holder must count as busy");
    }

    #[test]
    fn is_port_free_detects_loopback_holder() {
        let held = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
        let port = held.local_addr().unwrap().port();
        assert!(!is_port_free(port));
    }

    #[test]
    fn find_free_port_skips_occupied_preferred() {
        let held = std::net::TcpListener::bind(("0.0.0.0", 0)).expect("bind");
        let taken = held.local_addr().unwrap().port();
        let chosen = find_free_port(taken, taken + 20).expect("should find an alternative");
        assert_ne!(chosen, taken);
        assert!(chosen > taken && chosen <= taken + 20);
    }

    #[test]
    fn find_free_port_errors_when_range_exhausted() {
        let held = std::net::TcpListener::bind(("0.0.0.0", 0)).expect("bind");
        let taken = held.local_addr().unwrap().port();
        // 범위가 점유된 포트 하나뿐이면 대체할 곳이 없다.
        assert!(find_free_port(taken, taken).is_err());
    }

    #[test]
    fn port_for_falls_back_to_d1_preferred_when_unassigned() {
        // 배정하지 않은 키는 D1 값을 그대로 돌려준다 — 포워딩 전에도 URL이 성립해야 한다.
        assert_eq!(port_for("seaweedfs-filer"), 8888);
        assert_eq!(preferred_for("seaweedfs-filer"), 8888);
        assert_eq!(local_url("seaweedfs-filer"), "http://127.0.0.1:8888");
    }

    #[test]
    fn unknown_key_is_zero_not_a_guess() {
        assert_eq!(port_for("nope"), 0);
        assert!(assign("nope").is_err());
    }

    /// 호스트 스크립트가 단독 실행될 때 쓰는 폴백 주소는 파생시킬 수가 없다 — 파이썬은
    /// 앱 없이도 돌아야 하기 때문이다. 그래서 CLAUDE.md 규칙대로 "어긋나면 실패하는
    /// 테스트"를 둔다. 잡는 것은 두 가지다:
    ///
    /// 1. `localhost` 표기(D1 위반). macOS에서 `::1`로도 풀려, 와일드카드로 바인딩한
    ///    남의 프로세스와 만난다 — mistakes-log 2026-07-21에 실측 사례가 있다.
    /// 2. D1 우선 포트와 다른 숫자. 폴백이 5001이 아니면 앱이 주소를 주입하지 못했을 때
    ///    조용히 엉뚱한 곳을 가리킨다.
    ///
    /// 앱 경유 경로는 이 값을 쓰지 않는다(mlx.rs가 `--mlflow-uri`를, prefect.rs가
    /// `MLFLOW_TRACKING_URI`를 실제 배정 포트로 넘긴다) — 여기서 고정하는 것은 폴백이다.
    #[test]
    fn host_script_mlflow_fallbacks_follow_d1() {
        const WRAPPER: &str = include_str!("../../../scripts/mlx/finetune_wrapper.py");
        const RUNNER: &str = include_str!("../../../scripts/prefect/host_runner.py");

        let expected = format!("http://127.0.0.1:{}", preferred_for("mlflow"));
        assert!(
            WRAPPER.contains(&format!("default=\"{expected}\"")),
            "finetune_wrapper.py의 --mlflow-uri 폴백이 D1({expected})과 어긋났다"
        );
        assert!(
            RUNNER.contains(&format!("\"MLFLOW_TRACKING_URI\", \"{expected}\"")),
            "host_runner.py의 MLFLOW_TRACKING_URI 폴백이 D1({expected})과 어긋났다"
        );

        // URL 자리의 `localhost`는 어느 쪽에도 없어야 한다. 주석/판정 로직에 등장하는
        // 문자열까지 막지 않도록 스킴이 붙은 형태만 본다.
        for (name, src) in [("finetune_wrapper.py", WRAPPER), ("host_runner.py", RUNNER)] {
            assert!(
                !src.contains("http://localhost"),
                "{name}에 http://localhost가 남아 있다 — D1은 항상 127.0.0.1이다"
            );
        }
    }
}
