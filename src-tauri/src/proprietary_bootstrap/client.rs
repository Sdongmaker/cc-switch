use hmac::{Hmac, Mac};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;
use uuid::Uuid;

use super::fingerprint::build_device_fingerprint;

pub const BOOTSTRAP_PATH: &str = "/api/bootstrap/cc-switch";
pub const CLAIM_LINK_PATH: &str = "/api/bootstrap/cc-switch/claim-link";
pub(crate) const BLOCKED_ERROR: &str = "blocked";
const ACCEPTED_ACTIONS: [&str; 4] = ["created", "resumed", "restored", "token_rotated"];

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone)]
pub(crate) struct BootstrapConfig {
    pub base_url: String,
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct BootstrapRequest {
    pub install_id: String,
    pub device_fingerprint: String,
    pub client_version: String,
    pub platform: String,
    pub arch: String,
    pub build_channel: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BootstrapResponse {
    pub success: bool,
    #[serde(default)]
    pub message: String,
    pub data: Option<BootstrapResponseData>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BootstrapResponseData {
    pub action: String,
    pub provider: BootstrapProvider,
    #[allow(dead_code)]
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BootstrapProvider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    #[serde(default)]
    pub models: BootstrapModels,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct BootstrapModels {
    pub claude: Option<ClaudeModels>,
    pub codex: Option<CodexModels>,
    pub gemini: Option<GeminiModels>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ClaudeModels {
    pub model: Option<String>,
    pub haiku_model: Option<String>,
    pub sonnet_model: Option<String>,
    pub opus_model: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CodexModels {
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GeminiModels {
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ClaimLinkRequest {
    pub install_id: String,
    pub device_fingerprint: String,
    pub client_version: String,
    pub platform: String,
    pub arch: String,
    pub build_channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ClaimLinkResponse {
    pub success: bool,
    #[serde(default)]
    pub message: String,
    pub data: Option<ClaimLinkResponseData>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ClaimLinkResponseData {
    pub claim_url: String,
    pub expires_at: i64,
}

pub(crate) fn load_config() -> Result<BootstrapConfig, String> {
    let base_url = read_required_config(
        "CC_SWITCH_BOOTSTRAP_BASE_URL",
        option_env!("CC_SWITCH_BOOTSTRAP_BASE_URL"),
    )?;
    let client_id = read_required_config(
        "CC_SWITCH_BOOTSTRAP_CLIENT_ID",
        option_env!("CC_SWITCH_BOOTSTRAP_CLIENT_ID"),
    )?;
    let client_secret = read_required_config(
        "CC_SWITCH_BOOTSTRAP_CLIENT_SECRET",
        option_env!("CC_SWITCH_BOOTSTRAP_CLIENT_SECRET"),
    )?;

    Ok(BootstrapConfig {
        base_url,
        client_id,
        client_secret,
    })
}

fn read_required_config(name: &str, compile_time: Option<&'static str>) -> Result<String, String> {
    let value = std::env::var(name)
        .ok()
        .or_else(|| compile_time.map(str::to_string))
        .unwrap_or_default()
        .trim()
        .to_string();

    if value.is_empty() {
        return Err(format!("{name} is required"));
    }

    Ok(value)
}

pub(crate) fn build_request(install_id: String) -> BootstrapRequest {
    BootstrapRequest {
        install_id,
        device_fingerprint: build_device_fingerprint(),
        client_version: format!("{}-proprietary.1", env!("CARGO_PKG_VERSION")),
        platform: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        build_channel: "proprietary".to_string(),
    }
}

pub(crate) fn build_claim_link_request(install_id: String, redirect_path: Option<String>) -> ClaimLinkRequest {
    ClaimLinkRequest {
        install_id,
        device_fingerprint: build_device_fingerprint(),
        client_version: format!("{}-proprietary.1", env!("CARGO_PKG_VERSION")),
        platform: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        build_channel: "proprietary".to_string(),
        redirect_path,
    }
}

pub fn signature_string(timestamp: i64, nonce: &str, raw_body: &[u8], path: &str) -> String {
    let body_hash = hex::encode(Sha256::digest(raw_body));
    format!("POST\n{path}\n{timestamp}\n{nonce}\n{body_hash}")
}

pub fn sign(secret: &str, message: &[u8]) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(message);
    hex::encode(mac.finalize().into_bytes())
}

pub(crate) async fn send(
    config: &BootstrapConfig,
    install_id: String,
) -> Result<BootstrapResponseData, String> {
    let request = build_request(install_id);
    let raw_body = serde_json::to_vec(&request).map_err(|err| err.to_string())?;
    let timestamp = chrono::Utc::now().timestamp();
    let nonce = Uuid::new_v4().to_string();
    let signature_text = signature_string(timestamp, &nonce, &raw_body, BOOTSTRAP_PATH);
    let signature = sign(&config.client_secret, signature_text.as_bytes());
    let base = config.base_url.trim_end_matches('/');
    let url = format!("{base}{BOOTSTRAP_PATH}");

    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|err| summarize_error(err.to_string()))?
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header("X-CCS-Client-Id", &config.client_id)
        .header("X-CCS-Timestamp", timestamp.to_string())
        .header("X-CCS-Nonce", nonce)
        .header("X-CCS-Signature", signature)
        .body(raw_body)
        .send()
        .await
        .map_err(|err| summarize_error(err.to_string()))?;

    if response.status().as_u16() == 403 {
        return Err(BLOCKED_ERROR.to_string());
    }

    if response.status().as_u16() == 423 {
        return Err(BLOCKED_ERROR.to_string());
    }

    if !response.status().is_success() {
        return Err(format!("bootstrap http {}", response.status().as_u16()));
    }

    let payload = response
        .json::<BootstrapResponse>()
        .await
        .map_err(|err| summarize_error(err.to_string()))?;

    if !payload.success {
        let message = payload.message.trim();
        if is_blocked_message(message) {
            return Err(BLOCKED_ERROR.to_string());
        }

        return Err(if message.is_empty() {
            "bootstrap failed".to_string()
        } else {
            summarize_error(message.to_string())
        });
    }

    let data = payload
        .data
        .ok_or_else(|| "bootstrap data missing".to_string())?;
    validate_response_data(&data)?;
    Ok(data)
}

fn validate_response_data(data: &BootstrapResponseData) -> Result<(), String> {
    if !ACCEPTED_ACTIONS.contains(&data.action.as_str()) {
        return Err(format!("unsupported bootstrap action '{}'", data.action));
    }

    if data.provider.id != super::guard::MANAGED_PROVIDER_ID {
        return Err(format!(
            "unexpected bootstrap provider id '{}'",
            data.provider.id
        ));
    }

    Ok(())
}

pub(crate) async fn send_claim_link(
    config: &BootstrapConfig,
    install_id: String,
    redirect_path: Option<String>,
) -> Result<ClaimLinkResponseData, String> {
    let request = build_claim_link_request(install_id, redirect_path);
    let raw_body = serde_json::to_vec(&request).map_err(|err| err.to_string())?;
    let timestamp = chrono::Utc::now().timestamp();
    let nonce = Uuid::new_v4().to_string();
    let signature_text = signature_string(timestamp, &nonce, &raw_body, CLAIM_LINK_PATH);
    let signature = sign(&config.client_secret, signature_text.as_bytes());
    let base = config.base_url.trim_end_matches('/');
    let url = format!("{base}{CLAIM_LINK_PATH}");

    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|err| summarize_error(err.to_string()))?
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header("X-CCS-Client-Id", &config.client_id)
        .header("X-CCS-Timestamp", timestamp.to_string())
        .header("X-CCS-Nonce", nonce)
        .header("X-CCS-Signature", signature)
        .body(raw_body)
        .send()
        .await
        .map_err(|err| summarize_error(err.to_string()))?;

    if response.status().as_u16() == 403 {
        return Err(BLOCKED_ERROR.to_string());
    }

    if response.status().as_u16() == 423 {
        return Err(BLOCKED_ERROR.to_string());
    }

    if !response.status().is_success() {
        return Err(format!("claim-link http {}", response.status().as_u16()));
    }

    let payload = response
        .json::<ClaimLinkResponse>()
        .await
        .map_err(|err| summarize_error(err.to_string()))?;

    if !payload.success {
        let message = payload.message.trim();
        if is_blocked_message(message) {
            return Err(BLOCKED_ERROR.to_string());
        }

        return Err(if message.is_empty() {
            "claim-link failed".to_string()
        } else {
            summarize_error(message.to_string())
        });
    }

    payload
        .data
        .ok_or_else(|| "claim-link data missing".to_string())
}

pub(crate) fn is_blocked_error(message: &str) -> bool {
    message == BLOCKED_ERROR
}

fn is_blocked_message(message: &str) -> bool {
    let normalized = message.trim().to_ascii_lowercase();
    normalized == BLOCKED_ERROR
        || normalized.contains("device blocked")
        || normalized.contains("device_blocked")
        || normalized.contains("account blocked")
}

pub fn summarize_error(message: String) -> String {
    let mut value = redact_secrets(&message).replace('\n', " ");
    if value.chars().count() > 240 {
        value = value.chars().take(240).collect();
    }
    value
}

fn redact_secrets(message: &str) -> String {
    let token_re = Regex::new(r"sk-[A-Za-z0-9_-]{6,}").expect("valid token regex");
    let value = token_re.replace_all(message, "sk-[redacted]");
    let bearer_re =
        Regex::new(r"(?i)Bearer\s+[A-Za-z0-9._~+/=-]+").expect("valid bearer regex");
    bearer_re.replace_all(&value, "Bearer [redacted]").into_owned()
}
