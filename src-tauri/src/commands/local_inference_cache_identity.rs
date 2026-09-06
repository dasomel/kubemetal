use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheIdentity {
    pub runtime: String,
    pub runtime_version: Option<String>,
    pub model_id: String,
    pub model_revision: Option<String>,
    pub model_digest: Option<String>,
    pub quantization: Option<String>,
    pub cache_format_version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CacheCompatibilityRequest {
    pub previous: Option<CacheIdentity>,
    pub current: CacheIdentity,
}

#[derive(Debug, Serialize)]
pub struct CacheCompatibilityResult {
    pub compatible: Option<bool>,
    pub reasons: Vec<String>,
    pub action: String,
}

fn compare(previous: Option<&CacheIdentity>, current: &CacheIdentity) -> CacheCompatibilityResult {
    let Some(previous) = previous else {
        return CacheCompatibilityResult {
            compatible: None,
            reasons: vec!["No previous cache identity evidence is available".into()],
            action: "benchmark-as-cold-or-unknown".into(),
        };
    };
    let mut reasons = Vec::new();
    if previous.runtime != current.runtime {
        reasons.push("runtime changed".into());
    }
    if previous.runtime_version != current.runtime_version {
        reasons.push("runtime version changed or is unknown".into());
    }
    if previous.model_id != current.model_id {
        reasons.push("model id changed".into());
    }
    if previous.model_revision != current.model_revision {
        reasons.push("model revision changed or is unknown".into());
    }
    if previous.model_digest != current.model_digest {
        reasons.push("model digest changed or is unknown".into());
    }
    if previous.quantization != current.quantization {
        reasons.push("quantization changed or is unknown".into());
    }
    if previous.cache_format_version != current.cache_format_version {
        reasons.push("cache format version changed or is unknown".into());
    }

    if reasons.is_empty() {
        CacheCompatibilityResult {
            compatible: Some(true),
            reasons: vec!["Runtime/model/cache identity is unchanged".into()],
            action: "reuse-may-be-benchmarked".into(),
        }
    } else {
        CacheCompatibilityResult {
            compatible: Some(false),
            reasons,
            action: "treat-as-cold-and-do-not-claim-reuse".into(),
        }
    }
}

#[tauri::command]
pub async fn evaluate_local_inference_cache_compatibility(
    request: CacheCompatibilityRequest,
) -> Result<CacheCompatibilityResult, String> {
    if request.current.runtime.trim().is_empty() || request.current.model_id.trim().is_empty() {
        return Err("runtime and model_id are required for cache identity".into());
    }
    Ok(compare(request.previous.as_ref(), &request.current))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(revision: &str) -> CacheIdentity {
        CacheIdentity {
            runtime: "omlx".into(),
            runtime_version: Some("1.0".into()),
            model_id: "qwen".into(),
            model_revision: Some(revision.into()),
            model_digest: Some("sha256:a".into()),
            quantization: Some("4bit".into()),
            cache_format_version: Some("v1".into()),
        }
    }

    #[test]
    fn revision_change_forces_cold_semantics() {
        let result = compare(Some(&identity("r1")), &identity("r2"));
        assert_eq!(result.compatible, Some(false));
        assert!(result.reasons.iter().any(|reason| reason.contains("revision")));
    }

    #[test]
    fn exact_identity_allows_reuse_benchmark_but_does_not_claim_hit() {
        let current = identity("r1");
        let result = compare(Some(&current), &current);
        assert_eq!(result.compatible, Some(true));
        assert_eq!(result.action, "reuse-may-be-benchmarked");
    }
}
