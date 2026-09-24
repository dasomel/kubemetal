use super::*;
use crate::commands::kagent::KAGENT_NAMESPACE;
use crate::services::deploy_target::{
    DeployTarget, IntegrationLevel, COLIMA_CONTEXT, DEFAULT_EXTERNAL_NAMESPACE,
};

#[test]
fn colima_lifecycle_start_cluster_with_no_target() {
    let summary = build_operation_summary("start_cluster", None).unwrap();
    assert_eq!(summary.context, COLIMA_CONTEXT);
    assert_eq!(summary.namespace, "-");
    assert_eq!(summary.action, "start_cluster");
    assert_eq!(summary.risk_class, DeployRiskClass::Local);
    assert!(summary.target_description.contains("시작"));
    assert!(summary.target_description.contains("colima Kubernetes VM"));
}

#[test]
fn colima_lifecycle_stop_cluster_ignores_saved_target() {
    // 저장된 대상이 외부 클러스터여도 colima 수명주기 액션은 항상 colima를 가리켜야 한다.
    let external = DeployTarget::for_context("narwhal");
    let summary = build_operation_summary("stop_cluster", Some(&external)).unwrap();
    assert_eq!(summary.context, COLIMA_CONTEXT);
    assert_eq!(summary.namespace, "-");
    assert_eq!(summary.action, "stop_cluster");
    assert_eq!(summary.risk_class, DeployRiskClass::Local);
    assert!(summary.target_description.contains("정지"));
}

#[test]
fn colima_lifecycle_start_cluster_with_saved_target() {
    let external = DeployTarget::for_context("narwhal");
    let summary = build_operation_summary("start_cluster", Some(&external)).unwrap();
    assert_eq!(summary.context, COLIMA_CONTEXT);
    assert_eq!(summary.namespace, "-");
    assert_eq!(summary.risk_class, DeployRiskClass::Local);
    assert!(summary.target_description.contains("시작"));
}

#[test]
fn local_colima_provision_mlops_stack() {
    let target = DeployTarget::for_context(COLIMA_CONTEXT);
    let summary = build_operation_summary("provision_mlops_stack", Some(&target)).unwrap();
    assert_eq!(summary.context, COLIMA_CONTEXT);
    assert_eq!(summary.namespace, "default");
    assert_eq!(summary.action, "provision_mlops_stack");
    assert_eq!(summary.risk_class, DeployRiskClass::Local);
    assert!(summary.target_description.contains("로컬 colima"));
    assert!(summary.target_description.contains("default"));
    assert!(summary.target_description.contains("MLOps 스택"));
}

#[test]
fn external_cluster_provision_mlops_stack() {
    let mut target = DeployTarget::for_context("narwhal");
    target.namespace = "team-ml".into();
    // D30: 외부 클러스터 기본은 agent-only라 게이트가 막는다 — L2를 명시로 켜야 통과한다.
    target.integration_level = Some(IntegrationLevel::FullStack);
    let summary = build_operation_summary("provision_mlops_stack", Some(&target)).unwrap();
    assert_eq!(summary.context, "narwhal");
    assert_eq!(summary.namespace, "team-ml");
    assert_eq!(summary.action, "provision_mlops_stack");
    assert_eq!(summary.risk_class, DeployRiskClass::External);
    assert!(summary.target_description.contains("외부"));
    assert!(summary.target_description.contains("team-ml"));
    assert!(summary.target_description.contains("MLOps 스택"));
}

/// #18 리뷰: provision_mlops_stack 커맨드가 실행 전 거는 D30 게이트를 요약도 똑같이
/// 적용해야 한다 — 그러지 않으면 실제로는 거부될 외부+agent-only 대상을 확인 다이얼로그가
/// 그대로 보여주게 된다.
#[test]
fn provision_summary_rejects_target_that_fails_full_stack_gate() {
    // DeployTarget::for_context("narwhal")의 기본 integration_level은 None →
    // effective_integration_level() == AgentOnly (D30 기본값) → 게이트가 막아야 한다.
    let target = DeployTarget::for_context("narwhal");
    let err = build_operation_summary("provision_mlops_stack", Some(&target))
        .expect_err("agent-only external target must fail the full-stack gate");
    assert!(err.contains("agent-only") || err.contains("L2"));
}

#[test]
fn provision_summary_requires_a_target() {
    let err = build_operation_summary("provision_mlops_stack", None)
        .expect_err("provisioning without a target must be rejected");
    assert!(err.contains("provision_mlops_stack"));
}

#[test]
fn unknown_action_is_rejected_not_fabricated() {
    let err = build_operation_summary("delete_everything", None)
        .expect_err("unknown actions must not get a fabricated summary");
    assert!(err.contains("delete_everything"));

    let target = DeployTarget::for_context("narwhal");
    let err2 = build_operation_summary("drop_db", Some(&target))
        .expect_err("unknown actions with target must also be rejected");
    assert!(err2.contains("drop_db"));
}

#[test]
fn empty_action_is_rejected() {
    let err = build_operation_summary("", None).expect_err("empty action must be rejected");
    assert!(err.contains("Unknown operation"));
}

#[test]
fn rejects_target_with_empty_context_or_namespace() {
    let mut bad_target = DeployTarget::for_context("narwhal");
    // 게이트를 먼저 통과시켜, 이 테스트가 실제로 검증하려는 빈 컨텍스트 체크에 닿게 한다.
    bad_target.integration_level = Some(IntegrationLevel::FullStack);
    bad_target.context = "   ".into();
    let err = build_operation_summary("provision_mlops_stack", Some(&bad_target))
        .expect_err("blank context must be rejected");
    assert!(err.contains("empty context or namespace"));
}

/// `build_operation_summary`는 더 이상 InstallKagent를 직접 다루지 않는다(#18 리뷰) —
/// `describe_deploy_operation`을 거치지 않고 잘못 호출되는 경로가 있다면 조용히 대상을
/// 지어내지 않고 에러로 알려야 한다.
#[test]
fn build_operation_summary_rejects_install_kagent() {
    let target = DeployTarget::for_context(COLIMA_CONTEXT);
    let err = build_operation_summary("install_kagent", Some(&target))
        .expect_err("install_kagent summaries must go through build_kagent_install_summary");
    assert!(err.contains("build_kagent_install_summary"));
}

// --- build_kagent_install_summary (#18 리뷰 수정) ---
//
// install_kagent은 저장된 DeployTarget과 무관하게 호출자가 고른 컨텍스트에, 항상 고정
// 네임스페이스로 설치한다. 아래 테스트는 요약이 (a) 저장된 대상이 아니라 해석된 컨텍스트를
// 그대로 반영하고, (b) 네임스페이스를 install_kagent과 같은 KAGENT_NAMESPACE로 고정하는지
// 검증한다 — 리뷰가 지적한 축 혼동(다른 클러스터/네임스페이스를 보여주는 결함)의 재발 방지.

#[test]
fn kagent_install_summary_uses_the_resolved_context_verbatim() {
    // "저장된 배포 대상은 colima인데 KagentOpsView 드롭다운에서 외부 컨텍스트를 명시로
    // 골랐다"는 리뷰 시나리오 — resolved_context는 이미 그 선택을 반영한 값이고, 요약은
    // 저장된 대상이 아니라 이 값을 그대로 보여줘야 한다.
    let summary = build_kagent_install_summary("narwhal", KAGENT_NAMESPACE).unwrap();
    assert_eq!(summary.context, "narwhal");
    assert_eq!(summary.namespace, KAGENT_NAMESPACE);
    assert_eq!(summary.action, "install_kagent");
    assert_eq!(summary.risk_class, DeployRiskClass::External);
    assert!(summary.target_description.contains("외부"));
    assert!(summary.target_description.contains("narwhal"));
    assert!(summary.target_description.contains("kagent"));
    // 저장된 대상의 네임스페이스(DEFAULT_EXTERNAL_NAMESPACE)가 아니라 KAGENT_NAMESPACE여야 한다.
    assert_ne!(summary.namespace, DEFAULT_EXTERNAL_NAMESPACE);
}

#[test]
fn kagent_install_summary_is_local_for_colima_context() {
    let summary = build_kagent_install_summary(COLIMA_CONTEXT, KAGENT_NAMESPACE).unwrap();
    assert_eq!(summary.context, COLIMA_CONTEXT);
    assert_eq!(summary.namespace, KAGENT_NAMESPACE);
    assert_eq!(summary.risk_class, DeployRiskClass::Local);
    assert!(summary.target_description.contains("로컬 colima"));
}

#[test]
fn kagent_install_summary_rejects_empty_context() {
    let err = build_kagent_install_summary("   ", KAGENT_NAMESPACE)
        .expect_err("blank resolved context must be rejected");
    assert!(err.contains("install_kagent"));
}

#[test]
fn kagent_install_summary_rejects_empty_namespace() {
    let err =
        build_kagent_install_summary("narwhal", "").expect_err("blank namespace must be rejected");
    assert!(err.contains("install_kagent"));
}
