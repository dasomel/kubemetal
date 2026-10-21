//! 배포/수명주기 작업 요약(OperationSummary) 생성 로직 (#18 축소 스코프).
//!
//! 파괴적 액션(provision_mlops_stack, install_kagent, start_cluster, stop_cluster) 실행 직전
//! 대상 클러스터 컨텍스트, 네임스페이스, 설명 및 위험 등급(risk_class) 요약을 생성한다.
//! 읽기 전용 순수 로직으로, 어떤 외부 명령이나 변경도 수행하지 않는다(D22).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::services::deploy_target::{colima_context, context_is_colima, DeployTarget};

static COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
struct RequestIdentity {
    action: DeployAction,
    target: (String, String),
    parameters: Vec<String>,
}

struct DeployState {
    in_flight: HashSet<(String, String)>,
    active_ids: HashSet<String>,
    completed: HashMap<String, (Instant, RequestIdentity, String)>,
}

static DEPLOY_STATE: OnceLock<Mutex<DeployState>> = OnceLock::new();

fn deploy_state() -> &'static Mutex<DeployState> {
    DEPLOY_STATE.get_or_init(|| {
        Mutex::new(DeployState {
            in_flight: HashSet::new(),
            active_ids: HashSet::new(),
            completed: HashMap::new(),
        })
    })
}

/// 유일한 작업 식별자 생성(타임스탬프 + PID + 원자적 카운터).
pub fn new_operation_id(action: &str) -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!(
        "op-{action}-{ms:013}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

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
    #[serde(default)]
    pub operation_id: String,
}

/// 사용자가 확인 다이얼로그에서 본 작업 대상의 최소 식별자. 설명이나 위험 등급은
/// 표시용이라 실행 대상 비교에 넣지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmedOperationTarget {
    pub context: String,
    pub namespace: String,
    #[serde(default)]
    pub operation_id: Option<String>,
}

/// 확인 이후 저장 대상이나 화면 선택기가 바뀌었으면, 적용 전에 멈춘다. `expected`는 필수다
/// (#18 승인 리뷰) — 이전에는 `Option`이었고 `None`이 "확인 생략"으로 통과됐는데, 프런트가
/// 항상 값을 넘기는 상황에서는 TS 쪽 키 오타(`expectedTarget` 누락)가 Tauri에서 조용히
/// `None`으로 역직렬화되어 가드 자체를 무력화할 수 있었다. 필수 파라미터로 만들면 키가
/// 빠졌을 때 serde가 즉시 실패해 이 경로가 아예 열리지 않는다.
pub fn ensure_confirmed_target(
    expected: &ConfirmedOperationTarget,
    actual_context: &str,
    actual_namespace: &str,
) -> Result<(), String> {
    if expected.context != actual_context || expected.namespace != actual_namespace {
        return Err(format!(
            "deploy target changed after confirmation: confirmed context='{}', namespace='{}'; actual context='{}', namespace='{}'. Reconfirm before applying.",
            expected.context, expected.namespace, actual_context, actual_namespace
        ));
    }
    if let Some(ref op_id) = expected.operation_id {
        if op_id.trim().is_empty() {
            return Err("operation_id cannot be empty when provided".into());
        }
    }
    Ok(())
}

/// 배포 작업 락 획득 결과.
#[derive(Debug)]
pub enum DeployGuardOutcome {
    Acquired(DeployOperationGuard),
    AlreadyCompleted(String),
}

/// 배포 작업 실행 중 동일 타깃(context/namespace)에 대한 동시 실행 방지 RAII 가드.
#[derive(Debug)]
pub struct DeployOperationGuard {
    request: RequestIdentity,
    operation_id: Option<String>,
    completed_result: Option<String>,
}

impl DeployOperationGuard {
    /// 성공 완료 시 결과를 기록하여 향후 동일 operation_id 재시도 시 멱등 응답을 가능하게 한다.
    pub fn complete(mut self, result: String) {
        self.completed_result = Some(result);
    }
}

impl Drop for DeployOperationGuard {
    fn drop(&mut self) {
        let mut state = deploy_state().lock().unwrap_or_else(|e| e.into_inner());
        state.in_flight.remove(&self.request.target);
        if let Some(id) = &self.operation_id {
            state.active_ids.remove(id);
        }
        if let (Some(id), Some(result)) = (&self.operation_id, &self.completed_result) {
            state.completed.insert(
                id.clone(),
                (Instant::now(), self.request.clone(), result.clone()),
            );
        }
    }
}

/// 배포 작업 실행 전 동시 실행 가드 및 멱등성 검사를 수행한다(#18).
/// - 동일한 operation_id와 action/target/parameters가 최근(5분 내) 이미 완료된 경우 캐시된 성공 결과를 즉시 반환(AlreadyCompleted).
/// - 동일한 (context, namespace) 대상에 이미 작업이 진행 중이면 Err(busy) 반환.
/// - 그 외의 경우 배타적 실행 권한(Acquired)을 부여하며, 반환된 가드가 drop될 때 슬롯이 해제된다.
pub fn acquire_deploy_guard(
    action: DeployAction,
    parameters: &[String],
    context: &str,
    namespace: &str,
    operation_id: Option<&str>,
) -> Result<DeployGuardOutcome, String> {
    let mut state = deploy_state().lock().unwrap_or_else(|e| e.into_inner());

    // 5분 초과 완료 이력 만료 정리
    let now = Instant::now();
    state
        .completed
        .retain(|_, (t, _, _)| now.duration_since(*t) < Duration::from_secs(300));

    let request = RequestIdentity {
        action,
        target: (context.to_string(), namespace.to_string()),
        parameters: parameters.to_vec(),
    };
    // D1 (#18 local): bind replay to the actual request, not a caller's ID alone.
    // Cost: retain small parameter vectors for five minutes; omit the optional ID
    // to opt out of replay while preserving the target concurrency guard.
    if let Some(id) = operation_id {
        if id.trim().is_empty() {
            return Err("operation_id cannot be empty when provided".into());
        }
        if state.active_ids.contains(id) {
            return Err("deploy operation busy: operation_id is already in progress".into());
        }
        if let Some((_, previous, cached_result)) = state.completed.get(id) {
            if previous != &request {
                return Err(
                    "operation_id was already used for a different request; reconfirm".into(),
                );
            }
            return Ok(DeployGuardOutcome::AlreadyCompleted(cached_result.clone()));
        }
    }

    let key = request.target.clone();
    if state.in_flight.contains(&key) {
        return Err(format!(
            "deploy operation busy: an operation is already in progress on context='{context}', namespace='{namespace}'"
        ));
    }

    state.in_flight.insert(key);
    if let Some(id) = operation_id {
        state.active_ids.insert(id.to_string());
    }
    Ok(DeployGuardOutcome::Acquired(DeployOperationGuard {
        request,
        operation_id: operation_id.map(str::to_string),
        completed_result: None,
    }))
}

#[cfg(test)]
pub fn reset_deploy_state_for_test() {
    let mut state = deploy_state().lock().unwrap_or_else(|e| e.into_inner());
    state.in_flight.clear();
    state.active_ids.clear();
    state.completed.clear();
}

/// `action`별 요약을 조립하는 순수 함수.
/// `DeployTarget`을 이미 들고 있는 호출부(테스트 포함)가 `AppHandle` 없이 바로 쓸 수 있게 분리했다.
///
/// colima 수명주기 액션(`start_cluster`, `stop_cluster`)은 저장된 배포 대상과 무관하게 항상
/// colima 컨텍스트로 고정하며(D26: colima 수명주기는 이 앱이 소유하는 유일한 대상), risk_class는
/// 항상 `DeployRiskClass::Local`이다.
///
/// `provision_mlops_stack`은 반드시 유효한 `DeployTarget`이 필요하다. 대상 해석이 실패하거나
/// (None) 컨텍스트/네임스페이스가 비어 있으면 에러를 반환한다(D22). `provision_mlops_stack`
/// 커맨드 자신이 실행 전 거는 `target.full_stack_gate()`(D30)를 여기서도 그대로 적용한다 —
/// 그래야 요약이 실제로는 거부될 대상을 보여주는 일이 없다(#18 리뷰). 대상이 colima이면
/// `Local`, 외부 클러스터이면 `External` risk_class를 부여한다.
///
/// `install_kagent`은 이 함수가 다루지 않는다 — `install_kagent`은 저장된 `DeployTarget`과
/// 무관하게 호출자가 고른 컨텍스트에, 항상 고정 네임스페이스로 설치하므로
/// `build_kagent_install_summary`가 별도로 담당한다(#18 리뷰: 두 축을 하나의 `DeployTarget`
/// 파라미터로 섞으면 어긋난 컨텍스트/네임스페이스를 보여주게 된다).
///
/// 알려지지 않은 `action`은 거부한다 — 요약을 지어내지 않는다(D22–D25).
pub fn build_operation_summary(
    action: &str,
    target: Option<&DeployTarget>,
) -> Result<OperationSummary, String> {
    let action_kind = DeployAction::parse(action)?;
    match action_kind {
        DeployAction::StartCluster | DeployAction::StopCluster => Ok(OperationSummary {
            context: colima_context().to_string(),
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
            operation_id: new_operation_id(action),
        }),
        DeployAction::ProvisionMlopsStack => {
            let target = target.ok_or_else(|| {
                format!("'{action}' requires a deploy target to build the confirmation summary")
            })?;
            // D30: provision_mlops_stack이 실행 전 거는 게이트와 같은 판정. 그래야 요약이
            // 실제로는 거부될 대상(외부 클러스터 + agent-only)을 미리 보여주지 않는다.
            target.full_stack_gate()?;
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
            Ok(OperationSummary {
                context: target.context.clone(),
                namespace: target.namespace.clone(),
                action: action.to_string(),
                target_description: format!(
                    "{cluster_kind} 클러스터 '{}'의 네임스페이스 '{}'에 MLOps 스택(MLflow/SeaweedFS/Prefect) \
                     전체 배포을(를) 적용합니다.",
                    target.context, target.namespace
                ),
                risk_class,
                operation_id: new_operation_id(action),
            })
        }
        DeployAction::InstallKagent => Err(format!(
            "'{action}' summaries are built via build_kagent_install_summary, not \
             build_operation_summary (see #18 review — this branch should be unreachable \
             through describe_deploy_operation)"
        )),
    }
}

/// `install_kagent` 확인 요약(#18 리뷰 수정) — `DeployTarget`을 받지 않는다. `install_kagent`은
/// 저장된 배포 대상과 무관하게 호출자가 고른 컨텍스트(`resolved_context`,
/// `resolve_kagent_install_context`가 계산)에, 항상 `namespace`(고정값, 호출부는
/// `KAGENT_NAMESPACE`를 넘긴다)로 설치한다. 저장된 `DeployTarget.namespace`를 보면 다른
/// 네임스페이스를 확인 요약에 보여주게 된다 — 이 함수가 고치는 결함이다.
///
/// `resolved_context`가 비어 있으면 에러를 반환한다(D22) — 대상 해석 실패를 지어내지 않는다.
/// colima 컨텍스트이면 `Local`, 그 외는 `External` risk_class를 부여하며, 판정은
/// `context_is_colima`(`DeployTarget::is_colima`와 동일 규칙)를 그대로 쓴다.
pub fn build_kagent_install_summary(
    resolved_context: &str,
    namespace: &str,
) -> Result<OperationSummary, String> {
    let ctx = resolved_context.trim();
    let ns = namespace.trim();
    if ctx.is_empty() || ns.is_empty() {
        return Err(format!(
            "install_kagent requires a non-empty context and namespace to build the \
             confirmation summary: context='{resolved_context}', namespace='{namespace}'"
        ));
    }
    let is_colima = context_is_colima(ctx);
    let risk_class = if is_colima {
        DeployRiskClass::Local
    } else {
        DeployRiskClass::External
    };
    let cluster_kind = if is_colima { "로컬 colima" } else { "외부" };
    Ok(OperationSummary {
        context: resolved_context.to_string(),
        namespace: namespace.to_string(),
        action: "install_kagent".to_string(),
        target_description: format!(
            "{cluster_kind} 클러스터 '{resolved_context}'의 네임스페이스 '{namespace}'에 \
             kagent 컨트롤러/도구/UI 설치을(를) 적용합니다."
        ),
        risk_class,
        operation_id: new_operation_id("install_kagent"),
    })
}

#[cfg(test)]
mod tests;
