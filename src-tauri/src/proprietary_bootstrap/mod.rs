use serde::{Deserialize, Serialize};
#[cfg(feature = "proprietary-bootstrap")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "proprietary-bootstrap")]
use std::time::Duration;

use crate::settings::ProprietaryBootstrapSettings;
use crate::store::AppState;

#[cfg(feature = "proprietary-bootstrap")]
mod client;
#[cfg(feature = "proprietary-bootstrap")]
mod fingerprint;
pub(crate) mod guard;
#[cfg(feature = "proprietary-bootstrap")]
mod provider;

#[cfg(feature = "proprietary-bootstrap")]
const RETRY_DELAYS_SECS: [u64; 4] = [60, 300, 900, 3600];

#[cfg(feature = "proprietary-bootstrap")]
static BACKGROUND_RETRY_SCHEDULED: AtomicBool = AtomicBool::new(false);

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
static TEST_MODE_ENABLED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PublicState {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_base_url: Option<String>,
}

#[cfg(feature = "proprietary-bootstrap")]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimLinkPublicData {
    pub claim_url: String,
    pub expires_at: i64,
}

pub fn is_enabled() -> bool {
    if !cfg!(feature = "proprietary-bootstrap") {
        return false;
    }

    #[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
    {
        return TEST_MODE_ENABLED.load(Ordering::SeqCst);
    }

    #[cfg(not(all(feature = "proprietary-bootstrap", feature = "test-hooks")))]
    {
        true
    }
}

#[cfg(feature = "test-hooks")]
fn startup_is_disabled_for_tests() -> bool {
    true
}

#[cfg(not(feature = "test-hooks"))]
fn startup_is_disabled_for_tests() -> bool {
    false
}

fn from_settings(settings: Option<&ProprietaryBootstrapSettings>) -> PublicState {
    let enabled = is_enabled();
    if !enabled {
        return PublicState {
            enabled,
            ..Default::default()
        };
    }

    let Some(settings) = settings else {
        return PublicState {
            enabled,
            ..Default::default()
        };
    };

    PublicState {
        enabled,
        status: Some(settings.status.clone()),
        last_action: settings.last_action.clone(),
        last_success_at: settings.last_success_at,
        last_attempt_at: settings.last_attempt_at,
        last_error: settings.last_error.clone(),
        provider_base_url: settings.provider_base_url.clone(),
    }
}

pub fn public_state() -> PublicState {
    let settings = crate::settings::get_settings();
    from_settings(settings.proprietary_bootstrap.as_ref())
}

#[cfg(feature = "proprietary-bootstrap")]
fn ensure_install_id(settings: &mut ProprietaryBootstrapSettings) {
    if settings.install_id.trim().is_empty() {
        settings.install_id = uuid::Uuid::new_v4().to_string();
    }
}

#[cfg(feature = "proprietary-bootstrap")]
fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(feature = "proprietary-bootstrap")]
fn existing_or_pending_settings() -> ProprietaryBootstrapSettings {
    let mut settings = crate::settings::get_settings()
        .proprietary_bootstrap
        .unwrap_or_else(|| ProprietaryBootstrapSettings {
            status: "pending".to_string(),
            ..ProprietaryBootstrapSettings::default()
        });
    ensure_install_id(&mut settings);
    settings
}

#[cfg(feature = "proprietary-bootstrap")]
fn persist_bootstrap_settings(
    settings: ProprietaryBootstrapSettings,
) -> Result<PublicState, String> {
    let mut app_settings = crate::settings::get_settings();
    app_settings.proprietary_bootstrap = Some(settings);
    crate::settings::update_settings(app_settings).map_err(|err| err.to_string())?;
    Ok(public_state())
}

#[cfg(feature = "proprietary-bootstrap")]
fn has_cached_managed_provider(state: &AppState) -> bool {
    guard::supported_apps().into_iter().any(|app_type| {
        state
            .db
            .get_provider_by_id(guard::MANAGED_PROVIDER_ID, app_type.as_str())
            .ok()
            .flatten()
            .is_some_and(|provider| guard::is_managed_provider(&provider))
    })
}

#[cfg_attr(not(feature = "proprietary-bootstrap"), allow(unused_variables))]
pub async fn run_startup(state: &AppState) -> Result<(), String> {
    if !is_enabled() || startup_is_disabled_for_tests() {
        return Ok(());
    }

    #[cfg(feature = "proprietary-bootstrap")]
    {
        if matches!(run_attempt(state).await?, AttemptOutcome::RetryableFailure) {
            spawn_background_retries(state);
        }
    }

    Ok(())
}

#[cfg(feature = "proprietary-bootstrap")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttemptOutcome {
    Ready,
    Blocked,
    RetryableFailure,
}

#[cfg(feature = "proprietary-bootstrap")]
async fn run_attempt(state: &AppState) -> Result<AttemptOutcome, String> {
    let mut settings = existing_or_pending_settings();
    settings.status = "pending".to_string();
    settings.last_attempt_at = Some(now_ts());
    settings.last_error = None;
    persist_bootstrap_settings(settings.clone())?;

    match run_once(state, settings.install_id.clone()).await {
        Ok(data) => match provider::upsert_managed_provider(state, &data.provider) {
            Ok(()) => {
                settings.status = "ready".to_string();
                settings.last_action = Some(data.action);
                settings.last_success_at = Some(now_ts());
                settings.last_error = None;
                settings.provider_base_url = Some(data.provider.base_url);
                persist_bootstrap_settings(settings)?;
                Ok(AttemptOutcome::Ready)
            }
            Err(err) => {
                record_retryable_failure(state, settings, format!("provider upsert failed: {err}"))
            }
        },
        Err(err) if client::is_blocked_error(&err) => {
            settings.status = "blocked".to_string();
            settings.last_error = Some(client::BLOCKED_ERROR.to_string());
            persist_bootstrap_settings(settings)?;
            Ok(AttemptOutcome::Blocked)
        }
        Err(err) => record_retryable_failure(state, settings, err),
    }
}

#[cfg(feature = "proprietary-bootstrap")]
fn record_retryable_failure(
    state: &AppState,
    mut settings: ProprietaryBootstrapSettings,
    err: String,
) -> Result<AttemptOutcome, String> {
    let cached = has_cached_managed_provider(state);
    settings.status = "error".to_string();
    settings.last_error = Some(client::summarize_error(err));
    persist_bootstrap_settings(settings)?;
    if !cached {
        log::warn!("Proprietary bootstrap failed without cached provider");
    }
    Ok(AttemptOutcome::RetryableFailure)
}

#[cfg(feature = "proprietary-bootstrap")]
fn spawn_background_retries(state: &AppState) {
    if BACKGROUND_RETRY_SCHEDULED.swap(true, Ordering::SeqCst) {
        return;
    }

    let db = state.db.clone();
    tauri::async_runtime::spawn(async move {
        for delay_secs in RETRY_DELAYS_SECS {
            tokio::time::sleep(Duration::from_secs(delay_secs)).await;

            let status = crate::settings::get_settings()
                .proprietary_bootstrap
                .map(|settings| settings.status);
            if matches!(status.as_deref(), Some("ready" | "blocked")) {
                break;
            }

            let retry_state = AppState::new(db.clone());
            match run_attempt(&retry_state).await {
                Ok(AttemptOutcome::Ready | AttemptOutcome::Blocked) => break,
                Ok(AttemptOutcome::RetryableFailure) => {}
                Err(err) => log::warn!("Proprietary bootstrap retry failed: {err}"),
            }
        }

        BACKGROUND_RETRY_SCHEDULED.store(false, Ordering::SeqCst);
    });
}

#[cfg(feature = "proprietary-bootstrap")]
async fn run_once(
    _state: &AppState,
    install_id: String,
) -> Result<client::BootstrapResponseData, String> {
    let config = client::load_config()?;
    client::send(&config, install_id).await
}

#[cfg(feature = "proprietary-bootstrap")]
pub async fn request_claim_link() -> Result<ClaimLinkPublicData, String> {
    let settings = existing_or_pending_settings();
    let data = client::send_claim_link(
        &client::load_config()?,
        settings.install_id.clone(),
        Some("/console/topup".to_string()),
    )
    .await?;

    Ok(ClaimLinkPublicData {
        claim_url: data.claim_url,
        expires_at: data.expires_at,
    })
}

pub async fn retry_startup(state: &AppState) -> Result<PublicState, String> {
    run_startup(state).await?;
    Ok(public_state())
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
pub use client::{sign, signature_string, summarize_error};

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
pub fn set_enabled_for_test(enabled: bool) {
    TEST_MODE_ENABLED.store(enabled, Ordering::SeqCst);
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
pub async fn run_attempt_for_test(state: &AppState) -> Result<PublicState, String> {
    let _ = run_attempt(state).await?;
    Ok(public_state())
}
