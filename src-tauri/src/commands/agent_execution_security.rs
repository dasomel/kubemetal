//! Vendor-neutral execution authorization contract for future KubeMetal ChatOps/remediation.
//!
//! This module intentionally does **not** expose a new Tauri command and does not make local
//! inference/tool-calling equivalent to execution authority.  It is the fail-closed boundary a
//! future L3 (approved action) executor must consume before any side effect is introduced.

use std::collections::HashSet;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const CANONICALIZATION_VERSION: &str = "kubemetal-json-c14n/v1";
pub const NORMALIZED_INVOCATION_VERSION: &str = "v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentRiskLevel {
    L0Query,
    L1Diagnostic,
    /// Recommendation/plan only.  It MUST NOT receive an executable child grant.
    L2ProposedAction,
    /// Predefined, allowlisted action.  Execution additionally requires exact approval.
    L3ApprovedAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionAuthorityGrant {
    pub grant_id: String,
    pub agent_identity: String,
    pub session_id: String,
    pub policy_version: String,
    pub allowed_tools: Vec<String>,
    pub allowed_target_prefixes: Vec<String>,
    pub issued_at_epoch_s: u64,
    pub expires_at_epoch_s: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedInvocation {
    pub resolution_id: String,
    pub agent_identity: String,
    pub session_id: String,
    pub policy_version: String,
    pub tool: String,
    pub tool_contract_version: String,
    pub resolved_target: String,
    pub normalized_resolved_arguments: Value,
    pub canonicalization_version: String,
    pub normalized_invocation_version: String,
    pub invocation_digest: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvocationGrant {
    pub invocation_grant_id: String,
    pub parent_session_grant_id: String,
    pub resolution_id: String,
    pub invocation_digest: String,
    pub agent_identity: String,
    pub session_id: String,
    pub policy_version: String,
    pub tool: String,
    pub tool_contract_version: String,
    pub resolved_target: String,
    pub risk_level: AgentRiskLevel,
    pub issued_at_epoch_s: u64,
    pub expires_at_epoch_s: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExactInvocationApproval {
    pub approval_id: String,
    pub invocation_grant_id: String,
    pub resolution_id: String,
    pub invocation_digest: String,
    pub approver: String,
    pub approved_at_epoch_s: u64,
    pub expires_at_epoch_s: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationError(pub String);

impl std::fmt::Display for AuthorizationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AuthorizationError {}

fn canonical_json(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(v) => out.push_str(if *v { "true" } else { "false" }),
        Value::Number(v) => out.push_str(&v.to_string()),
        Value::String(v) => out.push_str(&serde_json::to_string(v).expect("string serialization")),
        Value::Array(values) => {
            out.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                canonical_json(value, out);
            }
            out.push(']');
        }
        Value::Object(values) => {
            out.push('{');
            let mut keys: Vec<&String> = values.keys().collect();
            keys.sort_unstable();
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).expect("object key serialization"));
                out.push(':');
                canonical_json(&values[key], out);
            }
            out.push('}');
        }
    }
}

fn sha256_hex(input: &[u8]) -> String {
    let digest = Sha256::digest(input);
    let mut out = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut out, "{byte:02x}").expect("hex formatting");
    }
    out
}

pub fn resolve_invocation(
    resolution_id: impl Into<String>,
    agent_identity: impl Into<String>,
    session_id: impl Into<String>,
    policy_version: impl Into<String>,
    tool: impl Into<String>,
    tool_contract_version: impl Into<String>,
    resolved_target: impl Into<String>,
    normalized_resolved_arguments: Value,
) -> Result<ResolvedInvocation, AuthorizationError> {
    let resolution_id = resolution_id.into();
    let agent_identity = agent_identity.into();
    let session_id = session_id.into();
    let policy_version = policy_version.into();
    let tool = tool.into();
    let tool_contract_version = tool_contract_version.into();
    let resolved_target = resolved_target.into();

    for (name, value) in [
        ("resolution_id", &resolution_id),
        ("agent_identity", &agent_identity),
        ("session_id", &session_id),
        ("policy_version", &policy_version),
        ("tool", &tool),
        ("tool_contract_version", &tool_contract_version),
        ("resolved_target", &resolved_target),
    ] {
        if value.trim().is_empty() {
            return Err(AuthorizationError(format!("{name} is required")));
        }
    }

    let digest_input = serde_json::json!({
        "agent_identity": agent_identity,
        "session_id": session_id,
        "policy_version": policy_version,
        "tool": tool,
        "tool_contract_version": tool_contract_version,
        "resolved_target": resolved_target,
        "normalized_resolved_arguments": normalized_resolved_arguments,
        "canonicalization_version": CANONICALIZATION_VERSION,
        "normalized_invocation_version": NORMALIZED_INVOCATION_VERSION,
    });
    let mut canonical = String::new();
    canonical_json(&digest_input, &mut canonical);
    let invocation_digest = format!("sha256:{}", sha256_hex(canonical.as_bytes()));

    Ok(ResolvedInvocation {
        resolution_id,
        agent_identity,
        session_id,
        policy_version,
        tool,
        tool_contract_version,
        resolved_target,
        normalized_resolved_arguments,
        canonicalization_version: CANONICALIZATION_VERSION.into(),
        normalized_invocation_version: NORMALIZED_INVOCATION_VERSION.into(),
        invocation_digest,
    })
}

fn session_allows(
    session: &SessionAuthorityGrant,
    invocation: &ResolvedInvocation,
    now_epoch_s: u64,
) -> Result<(), AuthorizationError> {
    if now_epoch_s < session.issued_at_epoch_s || now_epoch_s >= session.expires_at_epoch_s {
        return Err(AuthorizationError("session-grant-expired".into()));
    }
    if session.agent_identity != invocation.agent_identity
        || session.session_id != invocation.session_id
        || session.policy_version != invocation.policy_version
    {
        return Err(AuthorizationError("session-binding-mismatch".into()));
    }
    if !session.allowed_tools.iter().any(|tool| tool == &invocation.tool) {
        return Err(AuthorizationError("tool-outside-session-authority".into()));
    }
    if !session
        .allowed_target_prefixes
        .iter()
        .any(|prefix| invocation.resolved_target.starts_with(prefix))
    {
        return Err(AuthorizationError("target-outside-session-authority".into()));
    }
    Ok(())
}

/// Attenuate a session authority ceiling into a short-lived invocation grant.
///
/// L2 is deliberately non-executable: a model can recommend a plan without receiving authority
/// to carry it out.  L3 receives only an exact invocation child grant and still needs approval.
pub fn issue_invocation_grant(
    invocation_grant_id: impl Into<String>,
    session: &SessionAuthorityGrant,
    invocation: &ResolvedInvocation,
    risk_level: AgentRiskLevel,
    now_epoch_s: u64,
    ttl_seconds: u64,
) -> Result<InvocationGrant, AuthorizationError> {
    session_allows(session, invocation, now_epoch_s)?;
    if risk_level == AgentRiskLevel::L2ProposedAction {
        return Err(AuthorizationError("proposal-only-no-execution-grant".into()));
    }
    if ttl_seconds == 0 {
        return Err(AuthorizationError("invocation-grant-ttl-required".into()));
    }
    let expires_at_epoch_s = now_epoch_s
        .saturating_add(ttl_seconds)
        .min(session.expires_at_epoch_s);
    if expires_at_epoch_s <= now_epoch_s {
        return Err(AuthorizationError("invocation-grant-expired".into()));
    }

    Ok(InvocationGrant {
        invocation_grant_id: invocation_grant_id.into(),
        parent_session_grant_id: session.grant_id.clone(),
        resolution_id: invocation.resolution_id.clone(),
        invocation_digest: invocation.invocation_digest.clone(),
        agent_identity: invocation.agent_identity.clone(),
        session_id: invocation.session_id.clone(),
        policy_version: invocation.policy_version.clone(),
        tool: invocation.tool.clone(),
        tool_contract_version: invocation.tool_contract_version.clone(),
        resolved_target: invocation.resolved_target.clone(),
        risk_level,
        issued_at_epoch_s: now_epoch_s,
        expires_at_epoch_s,
    })
}

pub fn validate_exact_approval(
    grant: &InvocationGrant,
    invocation: &ResolvedInvocation,
    approval: Option<&ExactInvocationApproval>,
    now_epoch_s: u64,
) -> Result<(), AuthorizationError> {
    if now_epoch_s < grant.issued_at_epoch_s || now_epoch_s >= grant.expires_at_epoch_s {
        return Err(AuthorizationError("invocation-grant-expired".into()));
    }
    if grant.resolution_id != invocation.resolution_id
        || grant.invocation_digest != invocation.invocation_digest
        || grant.agent_identity != invocation.agent_identity
        || grant.session_id != invocation.session_id
        || grant.tool != invocation.tool
        || grant.tool_contract_version != invocation.tool_contract_version
        || grant.resolved_target != invocation.resolved_target
    {
        return Err(AuthorizationError("invocation-grant-mismatch".into()));
    }

    if grant.risk_level != AgentRiskLevel::L3ApprovedAction {
        return Ok(());
    }

    let approval = approval.ok_or_else(|| AuthorizationError("approval-required".into()))?;
    if now_epoch_s < approval.approved_at_epoch_s || now_epoch_s >= approval.expires_at_epoch_s {
        return Err(AuthorizationError("approval-expired".into()));
    }
    if approval.invocation_grant_id != grant.invocation_grant_id
        || approval.resolution_id != invocation.resolution_id
        || approval.invocation_digest != invocation.invocation_digest
    {
        return Err(AuthorizationError("approval-invocation-mismatch".into()));
    }
    Ok(())
}

/// Process-local one-shot claim used by a future executor immediately before its side-effect
/// boundary.  Persistent/distributed executors must replace this with a durable atomic store.
#[derive(Default)]
pub struct ExecutionReplayGuard {
    consumed_approvals: Mutex<HashSet<String>>,
}

impl ExecutionReplayGuard {
    pub fn claim_l3_execution(
        &self,
        grant: &InvocationGrant,
        invocation: &ResolvedInvocation,
        approval: &ExactInvocationApproval,
        now_epoch_s: u64,
    ) -> Result<(), AuthorizationError> {
        validate_exact_approval(grant, invocation, Some(approval), now_epoch_s)?;
        if grant.risk_level != AgentRiskLevel::L3ApprovedAction {
            return Err(AuthorizationError("l3-grant-required".into()));
        }
        let mut consumed = self
            .consumed_approvals
            .lock()
            .map_err(|_| AuthorizationError("approval-replay-store-unavailable".into()))?;
        if !consumed.insert(approval.approval_id.clone()) {
            return Err(AuthorizationError("approval-replayed".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session() -> SessionAuthorityGrant {
        SessionAuthorityGrant {
            grant_id: "session-grant-1".into(),
            agent_identity: "kagent:k8s-agent".into(),
            session_id: "chat-1".into(),
            policy_version: "kubemetal-agent/v1".into(),
            allowed_tools: vec!["runbook.apply".into(), "k8s.get".into()],
            allowed_target_prefixes: vec!["cluster:colima/namespace:kubemetal".into()],
            issued_at_epoch_s: 100,
            expires_at_epoch_s: 500,
        }
    }

    fn resolved(args: Value) -> ResolvedInvocation {
        resolve_invocation(
            "resolution-1",
            "kagent:k8s-agent",
            "chat-1",
            "kubemetal-agent/v1",
            "runbook.apply",
            "v1",
            "cluster:colima/namespace:kubemetal/deployment:demo",
            args,
        )
        .unwrap()
    }

    #[test]
    fn digest_is_canonical_across_object_key_order() {
        let a = resolved(json!({"replicas": 2, "strategy": {"maxUnavailable": 0, "maxSurge": 1}}));
        let b = resolved(json!({"strategy": {"maxSurge": 1, "maxUnavailable": 0}, "replicas": 2}));
        assert_eq!(a.invocation_digest, b.invocation_digest);
    }

    #[test]
    fn changed_resolved_arguments_change_digest() {
        let a = resolved(json!({"replicas": 2}));
        let b = resolved(json!({"replicas": 3}));
        assert_ne!(a.invocation_digest, b.invocation_digest);
    }

    #[test]
    fn session_ceiling_rejects_ungranted_tool_and_target() {
        let mut invocation = resolved(json!({"replicas": 2}));
        invocation.tool = "kubectl.exec".into();
        assert_eq!(
            issue_invocation_grant("ig-1", &session(), &invocation, AgentRiskLevel::L1Diagnostic, 120, 30)
                .unwrap_err()
                .0,
            "tool-outside-session-authority"
        );

        let invocation = resolve_invocation(
            "resolution-2",
            "kagent:k8s-agent",
            "chat-1",
            "kubemetal-agent/v1",
            "runbook.apply",
            "v1",
            "cluster:prod/namespace:default/deployment:demo",
            json!({"replicas": 2}),
        )
        .unwrap();
        assert_eq!(
            issue_invocation_grant("ig-2", &session(), &invocation, AgentRiskLevel::L3ApprovedAction, 120, 30)
                .unwrap_err()
                .0,
            "target-outside-session-authority"
        );
    }

    #[test]
    fn proposed_action_never_receives_execution_authority() {
        let invocation = resolved(json!({"replicas": 2}));
        assert_eq!(
            issue_invocation_grant("ig-1", &session(), &invocation, AgentRiskLevel::L2ProposedAction, 120, 30)
                .unwrap_err()
                .0,
            "proposal-only-no-execution-grant"
        );
    }

    #[test]
    fn l3_requires_exact_approval_and_rejects_changed_invocation() {
        let invocation = resolved(json!({"replicas": 2}));
        let grant = issue_invocation_grant(
            "ig-1",
            &session(),
            &invocation,
            AgentRiskLevel::L3ApprovedAction,
            120,
            30,
        )
        .unwrap();
        assert_eq!(
            validate_exact_approval(&grant, &invocation, None, 125).unwrap_err().0,
            "approval-required"
        );

        let approval = ExactInvocationApproval {
            approval_id: "approval-1".into(),
            invocation_grant_id: grant.invocation_grant_id.clone(),
            resolution_id: invocation.resolution_id.clone(),
            invocation_digest: invocation.invocation_digest.clone(),
            approver: "operator@example.com".into(),
            approved_at_epoch_s: 121,
            expires_at_epoch_s: 140,
        };
        validate_exact_approval(&grant, &invocation, Some(&approval), 125).unwrap();

        let changed = resolved(json!({"replicas": 3}));
        assert_eq!(
            validate_exact_approval(&grant, &changed, Some(&approval), 125)
                .unwrap_err()
                .0,
            "invocation-grant-mismatch"
        );
    }

    #[test]
    fn l3_approval_is_one_shot() {
        let invocation = resolved(json!({"replicas": 2}));
        let grant = issue_invocation_grant(
            "ig-1",
            &session(),
            &invocation,
            AgentRiskLevel::L3ApprovedAction,
            120,
            30,
        )
        .unwrap();
        let approval = ExactInvocationApproval {
            approval_id: "approval-1".into(),
            invocation_grant_id: grant.invocation_grant_id.clone(),
            resolution_id: invocation.resolution_id.clone(),
            invocation_digest: invocation.invocation_digest.clone(),
            approver: "operator@example.com".into(),
            approved_at_epoch_s: 121,
            expires_at_epoch_s: 140,
        };
        let replay = ExecutionReplayGuard::default();
        replay
            .claim_l3_execution(&grant, &invocation, &approval, 125)
            .unwrap();
        assert_eq!(
            replay
                .claim_l3_execution(&grant, &invocation, &approval, 126)
                .unwrap_err()
                .0,
            "approval-replayed"
        );
    }

    #[test]
    fn expired_session_or_child_grant_fails_closed() {
        let invocation = resolved(json!({"replicas": 2}));
        assert_eq!(
            issue_invocation_grant("ig-1", &session(), &invocation, AgentRiskLevel::L1Diagnostic, 500, 30)
                .unwrap_err()
                .0,
            "session-grant-expired"
        );

        let grant = issue_invocation_grant(
            "ig-2",
            &session(),
            &invocation,
            AgentRiskLevel::L1Diagnostic,
            120,
            5,
        )
        .unwrap();
        assert_eq!(
            validate_exact_approval(&grant, &invocation, None, 125)
                .unwrap_err()
                .0,
            "invocation-grant-expired"
        );
    }
}
