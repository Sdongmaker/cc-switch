use serde_json::json;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{Provider, ProviderMeta};
use crate::services::provider::{restore_live_snapshot, write_live_with_common_config};
use crate::store::AppState;

use super::client::BootstrapProvider;

pub(crate) fn provider_id() -> &'static str {
    super::guard::MANAGED_PROVIDER_ID
}

pub(crate) fn upsert_managed_provider(
    state: &AppState,
    provider: &BootstrapProvider,
) -> Result<(), AppError> {
    if provider.id != provider_id() {
        return Err(AppError::Message(format!(
            "Unexpected bootstrap provider id '{}'",
            provider.id
        )));
    }

    let base_url = provider.base_url.trim_end_matches('/').to_string();
    let claude = build_claude_provider(provider, &base_url);
    let codex = build_codex_provider(provider, &base_url);
    let gemini = build_gemini_provider(provider, &base_url);

    upsert_managed_provider_set(
        state,
        vec![
            ManagedProviderWrite::new(AppType::Claude, claude),
            ManagedProviderWrite::new(AppType::Codex, codex),
            ManagedProviderWrite::new(AppType::Gemini, gemini),
        ],
    )
}

struct ManagedProviderWrite {
    app_type: AppType,
    provider: Provider,
}

impl ManagedProviderWrite {
    fn new(app_type: AppType, provider: Provider) -> Self {
        Self { app_type, provider }
    }
}

#[derive(Clone)]
struct ProviderSnapshot {
    app_type: AppType,
    previous_provider: Option<Provider>,
    previous_db_current: Option<String>,
    previous_settings_current: Option<String>,
    previous_live: Option<serde_json::Value>,
}

fn upsert_managed_provider_set(
    state: &AppState,
    writes: Vec<ManagedProviderWrite>,
) -> Result<(), AppError> {
    let mut applied = Vec::new();

    for write in writes {
        let snapshot = match snapshot_app_state(state, &write.app_type) {
            Ok(snapshot) => snapshot,
            Err(err) => {
                let rollback_error = rollback_applied(state, applied);
                return Err(provider_upsert_error(err, rollback_error));
            }
        };
        match upsert_managed_provider_for_app(state, &write.app_type, &write.provider) {
            Ok(()) => applied.push(snapshot),
            Err(err) => {
                applied.push(snapshot);
                let rollback_error = rollback_applied(state, applied);
                return Err(provider_upsert_error(err, rollback_error));
            }
        }
    }

    Ok(())
}

fn snapshot_app_state(state: &AppState, app_type: &AppType) -> Result<ProviderSnapshot, AppError> {
    if !super::guard::is_supported_app(app_type) {
        return Err(AppError::Message(format!(
            "App {} does not support managed NewAPI bootstrap",
            app_type.as_str()
        )));
    }

    let previous_live = match crate::services::ProviderService::read_live_settings(app_type.clone())
    {
        Ok(value) => Some(value),
        Err(err) if live_missing_is_snapshot_none(&err) => None,
        Err(err) => return Err(err),
    };

    Ok(ProviderSnapshot {
        app_type: app_type.clone(),
        previous_provider: state
            .db
            .get_provider_by_id(provider_id(), app_type.as_str())?,
        previous_db_current: state.db.get_current_provider(app_type.as_str())?,
        previous_settings_current: crate::settings::get_current_provider(app_type),
        previous_live,
    })
}

fn upsert_managed_provider_for_app(
    state: &AppState,
    app_type: &AppType,
    provider: &Provider,
) -> Result<(), AppError> {
    if provider.id != super::guard::MANAGED_PROVIDER_ID {
        return Err(AppError::Message(format!(
            "Managed NewAPI provider id must be {}",
            super::guard::MANAGED_PROVIDER_ID
        )));
    }

    state.db.save_provider(app_type.as_str(), provider)?;
    crate::settings::set_current_provider(app_type, Some(provider.id.as_str()))?;
    state
        .db
        .set_current_provider(app_type.as_str(), &provider.id)?;
    write_live_with_common_config(state.db.as_ref(), app_type, provider)?;
    Ok(())
}

fn rollback_applied(state: &AppState, mut applied: Vec<ProviderSnapshot>) -> Option<String> {
    let mut errors = Vec::new();

    while let Some(snapshot) = applied.pop() {
        if let Err(err) = rollback_app(state, &snapshot) {
            errors.push(format!("{}: {err}", snapshot.app_type.as_str()));
        }
    }

    if errors.is_empty() {
        None
    } else {
        Some(errors.join("; "))
    }
}

fn rollback_app(state: &AppState, snapshot: &ProviderSnapshot) -> Result<(), AppError> {
    match &snapshot.previous_provider {
        Some(provider) => state
            .db
            .save_provider(snapshot.app_type.as_str(), provider)?,
        None => state
            .db
            .delete_provider(snapshot.app_type.as_str(), provider_id())?,
    }

    crate::settings::set_current_provider(
        &snapshot.app_type,
        snapshot.previous_settings_current.as_deref(),
    )?;

    restore_db_current(
        state,
        &snapshot.app_type,
        snapshot.previous_db_current.as_deref(),
    )?;
    restore_live_snapshot(&snapshot.app_type, snapshot.previous_live.as_ref())?;
    Ok(())
}

fn restore_db_current(
    state: &AppState,
    app_type: &AppType,
    previous_db_current: Option<&str>,
) -> Result<(), AppError> {
    if let Some(id) = previous_db_current {
        state.db.set_current_provider(app_type.as_str(), id)?;
        return Ok(());
    }

    state.db.clear_current_provider(app_type.as_str())
}

fn provider_upsert_error(err: AppError, rollback_error: Option<String>) -> AppError {
    match rollback_error {
        Some(rollback_error) => {
            AppError::Message(format!("{err}; rollback also failed: {rollback_error}"))
        }
        None => err,
    }
}

fn live_missing_is_snapshot_none(err: &AppError) -> bool {
    let message = err.to_string();
    message.contains("配置文件不存在")
        || message.contains("configuration missing")
        || message.contains("configuration file is missing")
        || message.contains("file not found")
        || message.contains("文件不存在")
}

fn base_provider(response: &BootstrapProvider, settings_config: serde_json::Value) -> Provider {
    Provider {
        id: provider_id().to_string(),
        name: response.name.clone(),
        settings_config,
        website_url: Some(response.base_url.clone()),
        category: Some("managed".to_string()),
        created_at: Some(chrono::Utc::now().timestamp_millis()),
        sort_index: Some(0),
        notes: None,
        meta: Some(ProviderMeta {
            provider_type: Some(super::guard::MANAGED_PROVIDER_TYPE.to_string()),
            ..ProviderMeta::default()
        }),
        icon: Some("newapi".to_string()),
        icon_color: None,
        in_failover_queue: false,
    }
}

fn build_claude_provider(response: &BootstrapProvider, base_url: &str) -> Provider {
    let models = response.models.claude.as_ref();
    let model = model_or(models.and_then(|m| m.model.as_deref()), "claude-sonnet-4-6");
    let haiku = model_or(
        models.and_then(|m| m.haiku_model.as_deref()),
        "claude-haiku-4-5-20251001",
    );
    let sonnet = model_or(models.and_then(|m| m.sonnet_model.as_deref()), &model);
    let opus = model_or(
        models.and_then(|m| m.opus_model.as_deref()),
        "claude-opus-4-7",
    );

    base_provider(
        response,
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": base_url,
                "ANTHROPIC_AUTH_TOKEN": response.api_key,
                "ANTHROPIC_MODEL": model,
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": haiku,
                "ANTHROPIC_DEFAULT_SONNET_MODEL": sonnet,
                "ANTHROPIC_DEFAULT_OPUS_MODEL": opus,
            }
        }),
    )
}

fn build_codex_provider(response: &BootstrapProvider, base_url: &str) -> Provider {
    let models = response.models.codex.as_ref();
    let model = model_or(models.and_then(|m| m.model.as_deref()), "gpt-5.4");
    let reasoning_effort = model_or(models.and_then(|m| m.reasoning_effort.as_deref()), "high");
    let codex_base_url = ensure_v1_base_url(base_url);
    let config = format!(
        r#"model_provider = "newapi"
model = "{model}"
model_reasoning_effort = "{reasoning_effort}"
disable_response_storage = true

[model_providers.newapi]
name = "NewAPI"
base_url = "{codex_base_url}"
wire_api = "responses"
requires_openai_auth = true"#
    );

    base_provider(
        response,
        json!({
            "auth": {
                "OPENAI_API_KEY": response.api_key
            },
            "config": config
        }),
    )
}

fn build_gemini_provider(response: &BootstrapProvider, base_url: &str) -> Provider {
    let models = response.models.gemini.as_ref();
    let model = model_or(models.and_then(|m| m.model.as_deref()), "gemini-3.1-pro");

    base_provider(
        response,
        json!({
            "env": {
                "GOOGLE_GEMINI_BASE_URL": base_url,
                "GEMINI_API_KEY": response.api_key,
                "GEMINI_MODEL": model
            }
        }),
    )
}

fn model_or(value: Option<&str>, fallback: &str) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn ensure_v1_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    }
}
