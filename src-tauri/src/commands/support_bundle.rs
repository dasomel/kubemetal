//! #17: 서포트 번들 생성 및 무결성 검증 Tauri IPC 커맨드

use std::path::PathBuf;
use tauri::AppHandle;

use crate::services::support_bundle::{SupportBundleResult, SupportBundleVerificationReport};

#[tauri::command]
pub async fn create_support_bundle(app: AppHandle) -> Result<SupportBundleResult, String> {
    crate::services::support_bundle::create_support_bundle_impl(&app).await
}

#[tauri::command]
pub async fn verify_support_bundle(
    bundle_dir: String,
    expected_manifest_sha256: Option<String>,
) -> Result<SupportBundleVerificationReport, String> {
    let path = PathBuf::from(bundle_dir);
    tokio::task::spawn_blocking(move || {
        crate::services::support_bundle::verify_support_bundle_dir(
            &path,
            expected_manifest_sha256.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("Support bundle verification task failed: {e}"))?
}
