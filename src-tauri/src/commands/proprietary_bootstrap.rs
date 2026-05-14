use tauri::State;

#[cfg(feature = "proprietary-bootstrap")]
use crate::proprietary_bootstrap::ClaimLinkPublicData;
use crate::store::AppState;

#[tauri::command]
pub async fn get_proprietary_bootstrap_state(
) -> Result<crate::proprietary_bootstrap::PublicState, String> {
    Ok(crate::proprietary_bootstrap::public_state())
}

#[tauri::command]
pub async fn retry_proprietary_bootstrap(
    state: State<'_, AppState>,
) -> Result<crate::proprietary_bootstrap::PublicState, String> {
    crate::proprietary_bootstrap::retry_startup(state.inner()).await
}

#[tauri::command]
pub async fn reset_proprietary_bootstrap(
) -> Result<crate::proprietary_bootstrap::PublicState, String> {
    crate::proprietary_bootstrap::reset_bootstrap_state()
}

#[tauri::command]
#[cfg(feature = "proprietary-bootstrap")]
pub async fn claim_account_link() -> Result<ClaimLinkPublicData, String> {
    crate::proprietary_bootstrap::request_claim_link().await
}

#[tauri::command]
#[cfg(not(feature = "proprietary-bootstrap"))]
pub async fn claim_account_link() -> Result<serde_json::Value, String> {
    Err("proprietary bootstrap is disabled".to_string())
}
