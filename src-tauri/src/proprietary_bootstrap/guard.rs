use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;

pub const MANAGED_PROVIDER_ID: &str = "managed-newapi";
pub const MANAGED_PROVIDER_TYPE: &str = "managed_newapi";

#[cfg(feature = "proprietary-bootstrap")]
pub(crate) fn supported_apps() -> [AppType; 3] {
    [AppType::Claude, AppType::Codex, AppType::Gemini]
}

pub(crate) fn is_supported_app(app_type: &AppType) -> bool {
    matches!(app_type, AppType::Claude | AppType::Codex | AppType::Gemini)
}

pub(crate) fn locked_error() -> AppError {
    AppError::Message("专有版由 NewAPI bootstrap 管理供应商，不能修改供应商配置".to_string())
}

pub(crate) fn provider_import_locked_error() -> AppError {
    AppError::Message(
        "专有版由 NewAPI bootstrap 管理供应商，不能通过 deep link 导入供应商".to_string(),
    )
}

pub(crate) fn is_managed_provider_id(id: &str) -> bool {
    id == MANAGED_PROVIDER_ID
}

pub(crate) fn is_managed_provider(provider: &Provider) -> bool {
    is_managed_provider_id(&provider.id)
        && provider.category.as_deref() == Some("managed")
        && provider
            .meta
            .as_ref()
            .and_then(|meta| meta.provider_type.as_deref())
            == Some(MANAGED_PROVIDER_TYPE)
}
