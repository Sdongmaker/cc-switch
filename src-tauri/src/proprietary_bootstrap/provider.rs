use serde_json::json;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{Provider, ProviderMeta};
use crate::services::ProviderService;
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

    ProviderService::upsert_managed_newapi_provider(state, AppType::Claude, claude)?;
    ProviderService::upsert_managed_newapi_provider(state, AppType::Codex, codex)?;
    ProviderService::upsert_managed_newapi_provider(state, AppType::Gemini, gemini)?;

    Ok(())
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
    let opus = model_or(models.and_then(|m| m.opus_model.as_deref()), "claude-opus-4-7");

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
