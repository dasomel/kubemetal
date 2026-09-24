use super::*;
use crate::services::deploy_target::{DeployTarget, COLIMA_CONTEXT, DEFAULT_EXTERNAL_NAMESPACE};

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
fn local_colima_install_kagent() {
    let target = DeployTarget::for_context(COLIMA_CONTEXT);
    let summary = build_operation_summary("install_kagent", Some(&target)).unwrap();
    assert_eq!(summary.context, COLIMA_CONTEXT);
    assert_eq!(summary.namespace, "default");
    assert_eq!(summary.action, "install_kagent");
    assert_eq!(summary.risk_class, DeployRiskClass::Local);
    assert!(summary.target_description.contains("로컬 colima"));
    assert!(summary.target_description.contains("kagent"));
}

#[test]
fn external_cluster_provision_mlops_stack() {
    let mut target = DeployTarget::for_context("narwhal");
    target.namespace = "team-ml".into();
    let summary = build_operation_summary("provision_mlops_stack", Some(&target)).unwrap();
    assert_eq!(summary.context, "narwhal");
    assert_eq!(summary.namespace, "team-ml");
    assert_eq!(summary.action, "provision_mlops_stack");
    assert_eq!(summary.risk_class, DeployRiskClass::External);
    assert!(summary.target_description.contains("외부"));
    assert!(summary.target_description.contains("team-ml"));
    assert!(summary.target_description.contains("MLOps 스택"));
}

#[test]
fn external_cluster_install_kagent() {
    let target = DeployTarget::for_context("remote-prod");
    let summary = build_operation_summary("install_kagent", Some(&target)).unwrap();
    assert_eq!(summary.context, "remote-prod");
    assert_eq!(summary.namespace, DEFAULT_EXTERNAL_NAMESPACE);
    assert_eq!(summary.action, "install_kagent");
    assert_eq!(summary.risk_class, DeployRiskClass::External);
    assert!(summary.target_description.contains("외부"));
    assert!(summary.target_description.contains("remote-prod"));
    assert!(summary.target_description.contains("kagent"));
}

#[test]
fn provision_summary_requires_a_target() {
    let err = build_operation_summary("provision_mlops_stack", None)
        .expect_err("provisioning without a target must be rejected");
    assert!(err.contains("provision_mlops_stack"));
}

#[test]
fn install_kagent_summary_requires_a_target() {
    let err = build_operation_summary("install_kagent", None)
        .expect_err("install_kagent without a target must be rejected");
    assert!(err.contains("install_kagent"));
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
    bad_target.context = "   ".into();
    let err = build_operation_summary("provision_mlops_stack", Some(&bad_target))
        .expect_err("blank context must be rejected");
    assert!(err.contains("empty context or namespace"));

    bad_target.context = "narwhal".into();
    bad_target.namespace = "".into();
    let err2 = build_operation_summary("install_kagent", Some(&bad_target))
        .expect_err("blank namespace must be rejected");
    assert!(err2.contains("empty context or namespace"));
}
