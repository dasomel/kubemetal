//! 배포/수명주기 작업 요약(OperationSummary) 생성 로직 (#18 축소 스코프).
//!
//! 파괴적 액션(provision_mlops_stack, install_kagent, start_cluster, stop_cluster) 실행 직전
//! 대상 클러스터 컨텍스트, 네임스페이스, 설명 및 위험 등급(risk_class) 요약을 생성한다.
//! 읽기 전용 순수 로직으로, 어떤 외부 명령이나 변경도 수행하지 않는다(D22).

use serde::{Deserialize, Serialize};

use crate::services::deploy_target::{DeployTarget, COLIMA_CONTEXT};

/// 배포 작업 위험 등급: 로컬(colima) 작업과 외부 클러스터 작업을 구분한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeployRiskClass {
    Local,
    External,
}

/// 지원되는 배포/수명주기 액션 목록.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeployAction {
    StartCluster,
    StopCluster,
    ProvisionMlopsStack,
    InstallKagent,
}

impl DeployAction {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "start_cluster" => Ok(Self::StartCluster),
            "stop_cluster" => Ok(Self::StopCluster),
            "provision_mlops_stack" => Ok(Self::ProvisionMlopsStack),
            "install_kagent" => Ok(Self::InstallKagent),
            other => Err(format!(
                "Unknown operation for confirmation summary: {other}"
            )),
        }
    }
}

/// 파괴적 액션 실행 직전 확인에 쓰는 요약(#18 축소 스코프).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationSummary {
    pub context: String,
    pub namespace: String,
    pub action: String,
    pub target_description: String,
    pub risk_class: DeployRiskClass,
}

/// `action`별 요약을 조립하는 순수 함수.
/// `DeployTarget`을 이미 들고 있는 호출부(테스트 포함)가 `AppHandle` 없이 바로 쓸 수 있게 분리했다.
///
/// colima 수명주기 액션(`start_cluster`, `stop_cluster`)은 저장된 배포 대상과 무관하게 항상
/// colima 컨텍스트로 고정하며(D26: colima 수명주기는 이 앱이 소유하는 유일한 대상), risk_class는
/// 항상 `DeployRiskClass::Local`이다.
///
/// `provision_mlops_stack`과 `install_kagent`는 반드시 유효한 `DeployTarget`이 필요하다.
/// 대상 해석이 실패하거나(None) 컨텍스트/네임스페이스가 비어 있으면 에러를 반환한다(D22).
/// 대상이 colima이면 `Local`, 외부 클러스터이면 `External` risk_class를 부여한다.
///
/// 알려지지 않은 `action`은 거부한다 — 요약을 지어내지 않는다(D22–D25).
pub fn build_operation_summary(
    action: &str,
    target: Option<&DeployTarget>,
) -> Result<OperationSummary, String> {
    let action_kind = DeployAction::parse(action)?;
    match action_kind {
        DeployAction::StartCluster | DeployAction::StopCluster => Ok(OperationSummary {
            context: COLIMA_CONTEXT.to_string(),
            namespace: "-".to_string(),
            action: action.to_string(),
            target_description: format!(
                "colima Kubernetes VM을 {}합니다. colima는 재진입 불가 — 진행 중에는 \
                 다른 수명주기 작업(start/stop)을 실행할 수 없습니다.",
                if action_kind == DeployAction::StartCluster {
                    "시작"
                } else {
                    "정지"
                }
            ),
            risk_class: DeployRiskClass::Local,
        }),
        DeployAction::ProvisionMlopsStack | DeployAction::InstallKagent => {
            let target = target.ok_or_else(|| {
                format!("'{action}' requires a deploy target to build the confirmation summary")
            })?;
            let ctx = target.context.trim();
            let ns = target.namespace.trim();
            if ctx.is_empty() || ns.is_empty() {
                return Err(format!(
                    "Deploy target has empty context or namespace: context='{}', namespace='{}'",
                    target.context, target.namespace
                ));
            }
            let is_colima = target.is_colima();
            let risk_class = if is_colima {
                DeployRiskClass::Local
            } else {
                DeployRiskClass::External
            };
            let cluster_kind = if is_colima { "로컬 colima" } else { "외부" };
            let action_label = match action_kind {
                DeployAction::ProvisionMlopsStack => {
                    "MLOps 스택(MLflow/SeaweedFS/Prefect) 전체 배포"
                }
                DeployAction::InstallKagent => "kagent 컨트롤러/도구/UI 설치",
                _ => unreachable!(),
            };
            Ok(OperationSummary {
                context: target.context.clone(),
                namespace: target.namespace.clone(),
                action: action.to_string(),
                target_description: format!(
                    "{cluster_kind} 클러스터 '{}'의 네임스페이스 '{}'에 {action_label}을(를) \
                     적용합니다.",
                    target.context, target.namespace
                ),
                risk_class,
            })
        }
    }
}

#[cfg(test)]
mod tests;
