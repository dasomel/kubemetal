//! #17: 서포트 번들 생성 Tauri IPC 커맨드

use tauri::AppHandle;

use crate::services::support_bundle::SupportBundleResult;

#[tauri::command]
pub async fn create_support_bundle(app: AppHandle) -> Result<SupportBundleResult, String> {
    crate::services::support_bundle::create_support_bundle_impl(&app).await
}
