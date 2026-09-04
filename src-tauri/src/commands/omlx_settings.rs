use serde::Deserialize;

use crate::commands::local_inference::RuntimeActionResult;
use crate::services::local_inference::loopback_http_request;

#[derive(Debug, Clone, Deserialize)]
pub struct SafeOmlxModelSettingsPatch {
    pub model_alias: Option<String>,
    pub ttl_seconds: Option<u64>,
    pub is_pinned: Option<bool>,
    pub is_default: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct SafeOmlxModelSettingsRequest {
    pub endpoint: String,
    pub model_id: String,
    pub patch: SafeOmlxModelSettingsPatch,
    pub api_key: Option<String>,
}

fn validate_model_id(model_id: &str) -> Result<(), String> {
    if model_id.trim().is_empty()
        || model_id.contains('/')
        || model_id.contains('?')
        || model_id.contains('#')
        || model_id.contains('\r')
        || model_id.contains('\n')
    {
        return Err("Invalid oMLX model id".into());
    }
    Ok(())
}

fn sparse_settings_json(patch: &SafeOmlxModelSettingsPatch) -> Result<String, String> {
    let mut body = serde_json::Map::new();
    if let Some(alias) = patch.model_alias.as_deref() {
        if alias.contains('\r') || alias.contains('\n') || alias.len() > 128 {
            return Err("Invalid model alias".into());
        }
        body.insert("model_alias".into(), serde_json::Value::String(alias.to_string()));
    }
    if let Some(ttl) = patch.ttl_seconds {
        body.insert("ttl_seconds".into(), serde_json::Value::from(ttl));
    }
    if let Some(pinned) = patch.is_pinned {
        body.insert("is_pinned".into(), serde_json::Value::Bool(pinned));
    }
    if let Some(is_default) = patch.is_default {
        body.insert("is_default".into(), serde_json::Value::Bool(is_default));
    }
    if body.is_empty() {
        return Err("At least one model setting must be provided".into());
    }
    serde_json::to_string(&body).map_err(|e| e.to_string())
}

/// Sparse oMLX settings update. It intentionally has a distinct Rust/Tauri command name so
/// it can coexist with the earlier compatibility implementation without generating duplicate
/// Tauri command symbols. New UI code must call this command.
#[tauri::command]
pub async fn set_omlx_model_settings_sparse(
    request: SafeOmlxModelSettingsRequest,
) -> Result<RuntimeActionResult, String> {
    validate_model_id(&request.model_id)?;
    let body = sparse_settings_json(&request.patch)?;
    let response = loopback_http_request(
        &request.endpoint,
        "PUT",
        &format!("/admin/api/models/{}/settings", request.model_id),
        Some(&body),
        request.api_key.as_deref(),
    )
    .await?;
    Ok(RuntimeActionResult {
        ok: (200..300).contains(&response.status),
        status_code: Some(response.status),
        detail: (!response.body.trim().is_empty()).then_some(response.body),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_patch_does_not_emit_unset_fields() {
        let json = sparse_settings_json(&SafeOmlxModelSettingsPatch {
            model_alias: None,
            ttl_seconds: Some(600),
            is_pinned: None,
            is_default: None,
        })
        .expect("serialize sparse patch");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value.get("ttl_seconds").and_then(|v| v.as_u64()), Some(600));
        assert!(value.get("model_alias").is_none());
        assert!(value.get("is_pinned").is_none());
        assert!(value.get("is_default").is_none());
    }

    #[test]
    fn empty_patch_is_rejected() {
        assert!(sparse_settings_json(&SafeOmlxModelSettingsPatch {
            model_alias: None,
            ttl_seconds: None,
            is_pinned: None,
            is_default: None,
        })
        .is_err());
    }
}
