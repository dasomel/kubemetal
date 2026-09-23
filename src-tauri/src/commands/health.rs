//! #17 축소 스코프: 이미 흩어져 있는 컴포넌트별 상태 조회(colima/guardrails/kagent)를
//! 하나의 IPC 호출로 모아 보여준다. 이슈 #17 원안의 dependency graph, correlation ID,
//! redaction, evidence bundle export는 이 슬라이스의 스코프 밖 — 세션 검토에서 "#1/#13/#15가
//! 먼저 정리돼야 한다"고 판단했고, 그중 #1/#15가 종결된 지금 첫 슬라이스만 착수한다.
//!
//! D22(모르면 지어내지 않는다) 준수: 하위 컴포넌트 하나의 조회가 실패해도 나머지 결과를
//! 가리지 않는다 — 실패한 필드는 `None` + 에러 메시지로 남기고, 전체 커맨드는 계속 성공을
//! 반환한다(`Err`로 통째로 실패시키지 않는다).

use serde::Serialize;

use crate::commands::colima::{get_cluster_status, ClusterStatus};
use crate::commands::guardrails::{get_guardrail_status, GuardrailStatus};
use crate::commands::kagent::{get_kagent_diagnostics, KagentDiagnosticReport};

#[derive(Debug, Serialize)]
pub struct SystemHealthSummary {
    pub colima: Option<ClusterStatus>,
    pub colima_error: Option<String>,
    pub guardrails: Option<GuardrailStatus>,
    pub guardrails_error: Option<String>,
    pub kagent: Option<KagentDiagnosticReport>,
    pub kagent_error: Option<String>,
    /// "healthy" | "degraded" | "unknown" — `derive_overall_status`가 매기는 최소 판정.
    pub overall: String,
}

#[tauri::command]
pub async fn get_system_health_summary(
    app: tauri::AppHandle,
) -> Result<SystemHealthSummary, String> {
    // 세 조회는 서로 무관하므로 동시에 실행한다(kagent.rs의 기존 tokio::join! 패턴과 동일).
    let (colima_result, guardrails_result, kagent_result) = tokio::join!(
        get_cluster_status(),
        get_guardrail_status(app.clone()),
        get_kagent_diagnostics(None),
    );

    let (colima, colima_error) = split_result(colima_result);
    let (guardrails, guardrails_error) = split_result(guardrails_result);
    let (kagent, kagent_error) = split_result(kagent_result);

    let overall = derive_overall_status(colima.as_ref(), guardrails.as_ref(), kagent.as_ref());

    Ok(SystemHealthSummary {
        colima,
        colima_error,
        guardrails,
        guardrails_error,
        kagent,
        kagent_error,
        overall,
    })
}

/// 조회 실패를 전체 실패로 전파하지 않기 위한 변환 — 성공/실패를 `(값, 에러메시지)`로 나눈다.
fn split_result<T>(result: Result<T, String>) -> (Option<T>, Option<String>) {
    match result {
        Ok(value) => (Some(value), None),
        Err(e) => (None, Some(e)),
    }
}

/// 파생 판정 — 새 정책을 만들지 않고 각 하위 구조체가 이미 갖고 있는 필드만 본다.
/// - 셋 다 조회 자체가 실패하면 아무것도 판정할 수 없으므로 "unknown".
/// - 하나라도 조회 실패했거나, 조회에 성공했지만 이상 신호가 있으면 "degraded":
///   - colima: **클러스터가 떠 있을 때만** `kubernetes_active`/`mlflow_ready`/
///     `seaweedfs_ready`를 함께 요구한다 — `is_running`만 보면 "떠는 있지만 스택이
///     하나도 안 올라온" 상태가 healthy로 새는 결함이 있었다. 꺼져 있는 클러스터는
///     그 자체로 이미 `is_running=false`로 잡힌다.
///   - guardrails: `memory_pressure_level == "critical"`뿐 아니라 `"unknown"`도
///     포함한다(sysctl 실패, `measure_memory_pressure_level`이 에러 대신 `"unknown"`
///     문자열로 성공 반환한다) — 측정 못 한 값이 "healthy"로 새면 D22 위반이다.
///     같은 이유로 `thermal_state.is_none()`(NSProcessInfo 읽기 실패)도 포함한다.
///     "unknown 측정치"와 "실측된 위험 신호"를 이 필드 하나로는 구분하지 않기로
///     했다 — 구분이 필요하면 호출자가 `guardrails`/`guardrails_error`를 직접
///     본다(`overall`은 최소 판정일 뿐이다). `on_battery`는 pmset 실패 시 `false`로
///     채워지는 미검증 값이라(guardrails.rs, 다른 레인 소유— 여기서 고치지 않는다)
///     "healthy"의 근거로 쓰지 않는다(원래도 안 썼다 — 앞으로도 추가하지 않는다는
///     것을 명시).
///   - kagent: `pod_issues_count > 0`만 본다. **`kagent_installed == false`는
///     이상 신호가 아니다** — kagent는 옵트인 설치이므로(D30/D33) 미설치가 기본
///     상태다. 설치 안 됨을 degraded로 잡으면 kagent를 안 쓰는 대다수 설치에서
///     이 커맨드가 항상 degraded를 반환하게 된다.
/// - 셋 다 조회 성공하고 이상 신호가 하나도 없으면 "healthy".
fn derive_overall_status(
    colima: Option<&ClusterStatus>,
    guardrails: Option<&GuardrailStatus>,
    kagent: Option<&KagentDiagnosticReport>,
) -> String {
    if colima.is_none() && guardrails.is_none() && kagent.is_none() {
        return "unknown".to_string();
    }

    let any_lookup_failed = colima.is_none() || guardrails.is_none() || kagent.is_none();

    let colima_unhealthy = colima.is_some_and(|c| {
        !(c.is_running && c.kubernetes_active && c.mlflow_ready && c.seaweedfs_ready)
    });
    let guardrails_unhealthy = guardrails.is_some_and(|g| {
        g.memory_pressure_level == "critical"
            || g.memory_pressure_level == "unknown"
            || g.thermal_state.is_none()
    });
    let kagent_unhealthy = kagent.is_some_and(|k| k.pod_issues_count > 0);

    if any_lookup_failed || colima_unhealthy || guardrails_unhealthy || kagent_unhealthy {
        "degraded".to_string()
    } else {
        "healthy".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy_colima() -> ClusterStatus {
        ClusterStatus {
            is_running: true,
            kubernetes_active: true,
            mlflow_ready: true,
            seaweedfs_ready: true,
            artifact_store_wired: true,
        }
    }

    fn healthy_guardrails() -> GuardrailStatus {
        GuardrailStatus {
            memory_pressure_level: "normal".to_string(),
            on_battery: false,
            battery_pause_enabled: false,
            training_paused: false,
            caffeinate_active: false,
            thermal_state: Some("nominal".to_string()),
            thermal_pause_enabled: false,
            resume_overrides: vec![],
        }
    }

    fn healthy_kagent() -> KagentDiagnosticReport {
        KagentDiagnosticReport {
            target_context: "colima".to_string(),
            kagent_ready: true,
            kagent_installed: true,
            pod_issues_count: 0,
            recent_diagnosis: "ok".to_string(),
            recommended_action: "none".to_string(),
            active_agents: vec![],
            available_agents: vec![],
        }
    }

    #[test]
    fn all_healthy_yields_healthy() {
        let colima = healthy_colima();
        let guardrails = healthy_guardrails();
        let kagent = healthy_kagent();
        assert_eq!(
            derive_overall_status(Some(&colima), Some(&guardrails), Some(&kagent)),
            "healthy"
        );
    }

    #[test]
    fn one_unhealthy_component_yields_degraded() {
        let colima = healthy_colima();
        let mut guardrails = healthy_guardrails();
        guardrails.memory_pressure_level = "critical".to_string();
        let kagent = healthy_kagent();
        assert_eq!(
            derive_overall_status(Some(&colima), Some(&guardrails), Some(&kagent)),
            "degraded"
        );
    }

    #[test]
    fn one_failed_lookup_yields_degraded() {
        let guardrails = healthy_guardrails();
        let kagent = healthy_kagent();
        assert_eq!(
            derive_overall_status(None, Some(&guardrails), Some(&kagent)),
            "degraded"
        );
    }

    #[test]
    fn all_lookups_failed_yields_unknown() {
        assert_eq!(derive_overall_status(None, None, None), "unknown");
    }

    /// sysctl 실패 시 `measure_memory_pressure_level`은 Err가 아니라 `"unknown"`
    /// 문자열로 성공 반환한다(guardrails.rs) — 그 값이 healthy로 새면 안 된다(D22).
    #[test]
    fn unknown_memory_pressure_level_is_not_healthy() {
        let colima = healthy_colima();
        let mut guardrails = healthy_guardrails();
        guardrails.memory_pressure_level = "unknown".to_string();
        let kagent = healthy_kagent();
        assert_eq!(
            derive_overall_status(Some(&colima), Some(&guardrails), Some(&kagent)),
            "degraded"
        );
    }

    /// NSProcessInfo.thermalState를 못 읽으면 `thermal_state`가 None이다 — 측정
    /// 실패를 "정상"으로 폴백하지 않는다(D22).
    #[test]
    fn missing_thermal_state_is_not_healthy() {
        let colima = healthy_colima();
        let mut guardrails = healthy_guardrails();
        guardrails.thermal_state = None;
        let kagent = healthy_kagent();
        assert_eq!(
            derive_overall_status(Some(&colima), Some(&guardrails), Some(&kagent)),
            "degraded"
        );
    }

    /// 클러스터는 떠 있지만(`is_running=true`) 스택 컴포넌트가 하나도 준비되지
    /// 않은 상태 — `is_running`만 보면 이게 healthy로 샜다.
    #[test]
    fn cluster_running_but_stack_not_ready_is_not_healthy() {
        let mut colima = healthy_colima();
        colima.kubernetes_active = false;
        colima.mlflow_ready = false;
        colima.seaweedfs_ready = false;
        let guardrails = healthy_guardrails();
        let kagent = healthy_kagent();
        assert_eq!(
            derive_overall_status(Some(&colima), Some(&guardrails), Some(&kagent)),
            "degraded"
        );
    }

    /// kagent는 옵트인 설치다(D30/D33) — 미설치는 이상 신호가 아니라 기본 상태이므로
    /// `pod_issues_count == 0`이면 여전히 healthy여야 한다.
    #[test]
    fn kagent_not_installed_with_no_pod_issues_is_still_healthy() {
        let colima = healthy_colima();
        let guardrails = healthy_guardrails();
        let mut kagent = healthy_kagent();
        kagent.kagent_installed = false;
        kagent.kagent_ready = false;
        kagent.pod_issues_count = 0;
        assert_eq!(
            derive_overall_status(Some(&colima), Some(&guardrails), Some(&kagent)),
            "healthy"
        );
    }
}
