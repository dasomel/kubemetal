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
/// - 하나라도 조회 실패했거나, 조회에 성공했지만 이상 신호(guardrail의
///   `memory_pressure_level == "critical"`, kagent의 `pod_issues_count > 0`, colima의
///   `!is_running`)가 있으면 "degraded".
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

    let colima_unhealthy = colima.is_some_and(|c| !c.is_running);
    let guardrails_unhealthy = guardrails.is_some_and(|g| g.memory_pressure_level == "critical");
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
}
