use tauri::State;

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
