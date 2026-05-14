#[path = "support.rs"]
mod support;

#[cfg(feature = "proprietary-bootstrap")]
use std::{
    ffi::OsString,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

#[cfg(feature = "proprietary-bootstrap")]
use serde_json::json;

use support::{ensure_test_home, reset_test_fs, test_mutex};

#[cfg(feature = "proprietary-bootstrap")]
use cc_switch_lib::{
    get_claude_settings_path, get_codex_auth_path, get_codex_config_path,
    import_provider_from_deeplink, parse_deeplink_url, read_json_file, update_settings,
    AppSettings, AppState, AppType, Database, Provider, ProviderMeta, ProviderService,
    ProviderSortUpdate, UniversalProvider,
};

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
use axum::{
    body::Bytes,
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
use serde_json::Value;

#[cfg(not(feature = "proprietary-bootstrap"))]
#[test]
fn default_build_reports_proprietary_bootstrap_disabled() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    assert!(!cc_switch_lib::proprietary_bootstrap::is_enabled());
    let state = cc_switch_lib::proprietary_bootstrap::public_state();
    assert!(!state.enabled);
    assert!(state.status.is_none());
}

#[cfg(feature = "proprietary-bootstrap")]
fn test_state() -> AppState {
    #[cfg(feature = "test-hooks")]
    enable_proprietary_mode_for_test();
    AppState::new(Arc::new(Database::memory().expect("create memory db")))
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
fn enable_proprietary_mode_for_test() {
    cc_switch_lib::proprietary_bootstrap::set_enabled_for_test(true);
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
#[derive(Clone)]
struct ServerResponse {
    status: StatusCode,
    body: Value,
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
#[derive(Clone)]
struct ObservedRequest {
    headers: HeaderMap,
    body: Vec<u8>,
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
#[derive(Clone)]
struct BootstrapTestServerState {
    response: Arc<Mutex<ServerResponse>>,
    observed: Arc<Mutex<Option<ObservedRequest>>>,
    request_count: Arc<AtomicUsize>,
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
struct BootstrapTestServer {
    base_url: String,
    state: BootstrapTestServerState,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    handle: tokio::task::JoinHandle<()>,
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
impl BootstrapTestServer {
    async fn start(status: StatusCode, body: Value) -> Self {
        let state = BootstrapTestServerState {
            response: Arc::new(Mutex::new(ServerResponse { status, body })),
            observed: Arc::new(Mutex::new(None)),
            request_count: Arc::new(AtomicUsize::new(0)),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test bootstrap server");
        let addr = listener
            .local_addr()
            .expect("read bootstrap server address");
        let app = Router::new()
            .route("/api/bootstrap/cc-switch", post(bootstrap_handler))
            .with_state(state.clone());
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await;
        });

        Self {
            base_url: format!("http://{addr}"),
            state,
            shutdown_tx: Some(shutdown_tx),
            handle,
        }
    }

    fn set_response(&self, status: StatusCode, body: Value) {
        *self.state.response.lock().expect("lock response") = ServerResponse { status, body };
    }

    fn request_count(&self) -> usize {
        self.state.request_count.load(Ordering::SeqCst)
    }

    fn observed_request(&self) -> ObservedRequest {
        self.state
            .observed
            .lock()
            .expect("lock observed request")
            .clone()
            .expect("bootstrap request should have been observed")
    }

    async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        let _ = self.handle.await;
    }
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
async fn bootstrap_handler(
    AxumState(state): AxumState<BootstrapTestServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    state.request_count.fetch_add(1, Ordering::SeqCst);
    *state.observed.lock().expect("lock observed request") = Some(ObservedRequest {
        headers,
        body: body.to_vec(),
    });

    let response = state.response.lock().expect("lock response").clone();
    (response.status, Json(response.body))
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
struct EnvGuard {
    saved: Vec<(&'static str, Option<OsString>)>,
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
impl EnvGuard {
    fn bootstrap(base_url: &str) -> Self {
        let values = [
            ("CC_SWITCH_BOOTSTRAP_BASE_URL", base_url),
            ("CC_SWITCH_BOOTSTRAP_CLIENT_ID", "cc-switch-test"),
            ("CC_SWITCH_BOOTSTRAP_CLIENT_SECRET", "secret"),
        ];
        let saved = values
            .iter()
            .map(|(key, _)| (*key, std::env::var_os(*key)))
            .collect::<Vec<_>>();
        for (key, value) in values {
            std::env::set_var(key, value);
        }
        Self { saved }
    }
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..) {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

#[cfg(feature = "proprietary-bootstrap")]
fn managed_provider() -> Provider {
    Provider {
        id: "managed-newapi".to_string(),
        name: "NewAPI".to_string(),
        settings_config: json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com",
                "ANTHROPIC_AUTH_TOKEN": "sk-managed"
            }
        }),
        website_url: Some("https://api.example.com".to_string()),
        category: Some("managed".to_string()),
        created_at: Some(1),
        sort_index: Some(0),
        notes: None,
        meta: Some(ProviderMeta {
            provider_type: Some("managed_newapi".to_string()),
            ..ProviderMeta::default()
        }),
        icon: Some("newapi".to_string()),
        icon_color: None,
        in_failover_queue: false,
    }
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
fn bootstrap_response(action: &str, base_url: &str, api_key: &str) -> Value {
    json!({
        "success": true,
        "message": "",
        "data": {
            "action": action,
            "provider": {
                "id": "managed-newapi",
                "name": "NewAPI",
                "base_url": base_url,
                "api_key": api_key,
                "models": {
                    "claude": {
                        "model": "claude-sonnet-test",
                        "haiku_model": "claude-haiku-test",
                        "sonnet_model": "claude-sonnet-test",
                        "opus_model": "claude-opus-test"
                    },
                    "codex": {
                        "model": "gpt-test",
                        "reasoning_effort": "high"
                    },
                    "gemini": {
                        "model": "gemini-test"
                    }
                }
            },
            "expires_at": 0
        }
    })
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
fn read_settings_json(home: &Path) -> Value {
    let path = home.join(".cc-switch").join("settings.json");
    let raw = std::fs::read_to_string(&path).expect("read settings.json");
    serde_json::from_str(&raw).expect("parse settings.json")
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
fn provider_json_contains_token(provider: &Provider, token: &str) -> bool {
    serde_json::to_string(&provider.settings_config)
        .expect("serialize provider settings")
        .contains(token)
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
fn provider_with_env(id: &str, name: &str, token: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        name.to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://old.example.com",
                "ANTHROPIC_AUTH_TOKEN": token
            }
        }),
        Some("https://old.example.com".to_string()),
    )
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
fn seed_existing_claude_state(state: &AppState, home: &Path) {
    let provider = provider_with_env("old-claude", "Old Claude", "sk-old-claude");
    state
        .db
        .save_provider(AppType::Claude.as_str(), &provider)
        .expect("seed old claude provider");
    state
        .db
        .set_current_provider(AppType::Claude.as_str(), "old-claude")
        .expect("set old claude current provider");
    let settings = AppSettings {
        current_provider_claude: Some("old-claude".to_string()),
        ..AppSettings::default()
    };
    update_settings(settings).expect("set old claude settings current provider");

    let claude_path = get_claude_settings_path();
    std::fs::create_dir_all(claude_path.parent().expect("claude settings parent"))
        .expect("create claude settings dir");
    std::fs::write(
        &claude_path,
        r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"sk-old-claude","ANTHROPIC_BASE_URL":"https://old.example.com"}}"#,
    )
    .expect("seed claude live config");

    assert!(
        home.join(".claude").join("settings.json").exists(),
        "seeded Claude live config should exist"
    );
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
fn seed_invalid_codex_live_config(home: &Path) {
    let codex_dir = home.join(".codex");
    std::fs::create_dir_all(&codex_dir).expect("create codex dir");
    std::fs::write(
        codex_dir.join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-old-codex"}"#,
    )
    .expect("seed codex auth");
    std::fs::write(codex_dir.join("config.toml"), "model_provider = [")
        .expect("seed invalid codex config");
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
fn assert_no_managed_provider_rows(state: &AppState) {
    for app in [AppType::Claude, AppType::Codex, AppType::Gemini] {
        assert!(
            state
                .db
                .get_provider_by_id("managed-newapi", app.as_str())
                .expect("read managed provider")
                .is_none(),
            "{} should not keep a managed provider after failed bootstrap",
            app.as_str()
        );
    }
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
fn settings_json_current_provider(home: &Path, key: &str) -> Option<String> {
    read_settings_json(home)
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
#[test]
fn signature_string_matches_newapi_contract() {
    let raw_body = br#"{"install_id":"i"}"#;
    let text = cc_switch_lib::proprietary_bootstrap::signature_string(
        1760000000,
        "nonce-1",
        raw_body,
        "/api/bootstrap/cc-switch",
    );
    assert_eq!(
        text,
        "POST\n/api/bootstrap/cc-switch\n1760000000\nnonce-1\n91c3ccf8292743b03ed71017029d30dcb35aee5c026f912cd12c40f1fe85f816"
    );
    let signature = cc_switch_lib::proprietary_bootstrap::sign("secret", text.as_bytes());
    assert_eq!(
        signature,
        "4b147714447bd999c9bd0db6a9c68eb36494dcc5e17eca15293bca7180827d07"
    );
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
#[test]
fn bootstrap_error_summary_redacts_api_tokens() {
    let summary = cc_switch_lib::proprietary_bootstrap::summarize_error(
        "upstream returned sk-old-secret and Bearer sk-new-secret".to_string(),
    );

    assert!(!summary.contains("sk-old-secret"));
    assert!(!summary.contains("sk-new-secret"));
    assert!(summary.contains("sk-[redacted]"));
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
#[tokio::test(flavor = "current_thread")]
async fn bootstrap_success_writes_managed_newapi_for_core_apps() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    enable_proprietary_mode_for_test();
    reset_test_fs();
    let home = ensure_test_home();
    let state = test_state();
    let server = BootstrapTestServer::start(
        StatusCode::OK,
        bootstrap_response("created", "https://api.example.com/", "sk-created"),
    )
    .await;
    let _env = EnvGuard::bootstrap(&server.base_url);

    let public_state = cc_switch_lib::proprietary_bootstrap::run_attempt_for_test(&state)
        .await
        .expect("bootstrap attempt should succeed");

    assert!(public_state.enabled);
    assert_eq!(public_state.status.as_deref(), Some("ready"));
    assert_eq!(public_state.last_action.as_deref(), Some("created"));
    assert_eq!(
        public_state.provider_base_url.as_deref(),
        Some("https://api.example.com/")
    );

    for app in [AppType::Claude, AppType::Codex, AppType::Gemini] {
        let providers = ProviderService::list(&state, app.clone()).expect("list providers");
        assert_eq!(
            providers.len(),
            1,
            "{} should have one provider",
            app.as_str()
        );
        let provider = providers
            .get("managed-newapi")
            .expect("managed NewAPI provider exists");
        assert_eq!(provider.category.as_deref(), Some("managed"));
        assert_eq!(
            provider.website_url.as_deref(),
            Some("https://api.example.com/")
        );
        assert_eq!(
            provider
                .meta
                .as_ref()
                .and_then(|meta| meta.provider_type.as_deref()),
            Some("managed_newapi")
        );
        assert_eq!(
            ProviderService::current(&state, app.clone()).expect("read current provider"),
            "managed-newapi"
        );
    }

    let claude = state
        .db
        .get_provider_by_id("managed-newapi", AppType::Claude.as_str())
        .expect("read claude provider")
        .expect("claude provider exists");
    assert_eq!(
        claude
            .settings_config
            .pointer("/env/ANTHROPIC_BASE_URL")
            .and_then(|value| value.as_str()),
        Some("https://api.example.com")
    );
    assert_eq!(
        claude
            .settings_config
            .pointer("/env/ANTHROPIC_AUTH_TOKEN")
            .and_then(|value| value.as_str()),
        Some("sk-created")
    );

    let codex = state
        .db
        .get_provider_by_id("managed-newapi", AppType::Codex.as_str())
        .expect("read codex provider")
        .expect("codex provider exists");
    assert_eq!(
        codex
            .settings_config
            .pointer("/auth/OPENAI_API_KEY")
            .and_then(|value| value.as_str()),
        Some("sk-created")
    );
    let codex_config = codex
        .settings_config
        .get("config")
        .and_then(|value| value.as_str())
        .expect("codex config text");
    assert!(codex_config.contains("base_url = \"https://api.example.com/v1\""));
    assert!(codex_config.contains("model = \"gpt-test\""));

    let gemini = state
        .db
        .get_provider_by_id("managed-newapi", AppType::Gemini.as_str())
        .expect("read gemini provider")
        .expect("gemini provider exists");
    assert_eq!(
        gemini
            .settings_config
            .pointer("/env/GOOGLE_GEMINI_BASE_URL")
            .and_then(|value| value.as_str()),
        Some("https://api.example.com")
    );
    assert_eq!(
        gemini
            .settings_config
            .pointer("/env/GEMINI_API_KEY")
            .and_then(|value| value.as_str()),
        Some("sk-created")
    );

    let settings = read_settings_json(home);
    assert_eq!(
        settings
            .pointer("/proprietaryBootstrap/status")
            .and_then(|value| value.as_str()),
        Some("ready")
    );
    let install_id = settings
        .pointer("/proprietaryBootstrap/installId")
        .and_then(|value| value.as_str())
        .expect("install_id persisted")
        .to_string();
    assert!(!install_id.is_empty());
    assert_eq!(
        settings
            .pointer("/currentProviderClaude")
            .and_then(|value| value.as_str()),
        Some("managed-newapi")
    );
    assert_eq!(
        settings
            .pointer("/currentProviderCodex")
            .and_then(|value| value.as_str()),
        Some("managed-newapi")
    );
    assert_eq!(
        settings
            .pointer("/currentProviderGemini")
            .and_then(|value| value.as_str()),
        Some("managed-newapi")
    );

    let observed = server.observed_request();
    assert_eq!(server.request_count(), 1);
    assert_eq!(
        observed
            .headers
            .get("x-ccs-client-id")
            .and_then(|value| value.to_str().ok()),
        Some("cc-switch-test")
    );
    let timestamp = observed
        .headers
        .get("x-ccs-timestamp")
        .and_then(|value| value.to_str().ok())
        .expect("timestamp header")
        .parse::<i64>()
        .expect("timestamp header should be an integer");
    let nonce = observed
        .headers
        .get("x-ccs-nonce")
        .and_then(|value| value.to_str().ok())
        .expect("nonce header");
    let expected_signature_text = cc_switch_lib::proprietary_bootstrap::signature_string(
        timestamp,
        nonce,
        &observed.body,
        "/api/bootstrap/cc-switch",
    );
    let expected_signature =
        cc_switch_lib::proprietary_bootstrap::sign("secret", expected_signature_text.as_bytes());
    assert_eq!(
        observed
            .headers
            .get("x-ccs-signature")
            .and_then(|value| value.to_str().ok()),
        Some(expected_signature.as_str())
    );
    let body: Value = serde_json::from_slice(&observed.body).expect("parse request body");
    assert_eq!(
        body.get("install_id").and_then(|value| value.as_str()),
        settings
            .pointer("/proprietaryBootstrap/installId")
            .and_then(|value| value.as_str())
    );
    assert!(body
        .get("device_fingerprint")
        .and_then(|value| value.as_str())
        .is_some_and(|value| value.starts_with("v1:")));
    assert_eq!(
        body.get("build_channel").and_then(|value| value.as_str()),
        Some("proprietary")
    );

    server.set_response(
        StatusCode::OK,
        bootstrap_response("resumed", "https://api.example.com/", "sk-created"),
    );
    cc_switch_lib::proprietary_bootstrap::run_attempt_for_test(&state)
        .await
        .expect("second bootstrap attempt should succeed");
    let settings_after_second_attempt = read_settings_json(home);
    assert_eq!(
        settings_after_second_attempt
            .pointer("/proprietaryBootstrap/installId")
            .and_then(|value| value.as_str()),
        Some(install_id.as_str())
    );
    assert_eq!(server.request_count(), 2);
    for app in [AppType::Claude, AppType::Codex, AppType::Gemini] {
        let providers = state
            .db
            .get_all_providers(app.as_str())
            .expect("read providers after second bootstrap");
        assert_eq!(providers.len(), 1);
        assert!(providers.contains_key("managed-newapi"));
    }

    server.shutdown().await;
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
#[tokio::test(flavor = "current_thread")]
async fn token_rotated_overwrites_existing_api_key() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    enable_proprietary_mode_for_test();
    reset_test_fs();
    let _home = ensure_test_home();
    let state = test_state();
    let server = BootstrapTestServer::start(
        StatusCode::OK,
        bootstrap_response("created", "https://api.example.com", "sk-old"),
    )
    .await;
    let _env = EnvGuard::bootstrap(&server.base_url);

    cc_switch_lib::proprietary_bootstrap::run_attempt_for_test(&state)
        .await
        .expect("first bootstrap attempt should succeed");
    server.set_response(
        StatusCode::OK,
        bootstrap_response("token_rotated", "https://api.example.com", "sk-new"),
    );
    let public_state = cc_switch_lib::proprietary_bootstrap::run_attempt_for_test(&state)
        .await
        .expect("token rotation bootstrap attempt should succeed");

    assert_eq!(public_state.status.as_deref(), Some("ready"));
    assert_eq!(public_state.last_action.as_deref(), Some("token_rotated"));
    assert_eq!(server.request_count(), 2);

    for app in [AppType::Claude, AppType::Codex, AppType::Gemini] {
        let provider = state
            .db
            .get_provider_by_id("managed-newapi", app.as_str())
            .expect("read managed provider")
            .expect("managed provider exists");
        assert!(provider_json_contains_token(&provider, "sk-new"));
        assert!(!provider_json_contains_token(&provider, "sk-old"));
    }

    assert!(!cc_switch_lib::proprietary_bootstrap::public_state()
        .last_error
        .unwrap_or_default()
        .contains("sk-new"));

    server.shutdown().await;
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
#[tokio::test(flavor = "current_thread")]
async fn bootstrap_upsert_rolls_back_prior_apps_when_later_live_read_fails() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    enable_proprietary_mode_for_test();
    reset_test_fs();
    let home = ensure_test_home();
    let state = test_state();
    seed_existing_claude_state(&state, home);
    seed_invalid_codex_live_config(home);

    let server = BootstrapTestServer::start(
        StatusCode::OK,
        bootstrap_response("created", "https://api.example.com", "sk-created"),
    )
    .await;
    let _env = EnvGuard::bootstrap(&server.base_url);

    let public_state = cc_switch_lib::proprietary_bootstrap::run_attempt_for_test(&state)
        .await
        .expect("failed provider upsert should be recorded as retryable state");

    assert_eq!(public_state.status.as_deref(), Some("error"));
    assert!(public_state
        .last_error
        .as_deref()
        .unwrap_or_default()
        .contains("provider upsert failed"));
    assert_no_managed_provider_rows(&state);
    assert_eq!(
        state
            .db
            .get_current_provider(AppType::Claude.as_str())
            .expect("read db claude current")
            .as_deref(),
        Some("old-claude")
    );
    assert_eq!(
        settings_json_current_provider(home, "currentProviderClaude").as_deref(),
        Some("old-claude")
    );

    let old_claude = state
        .db
        .get_provider_by_id("old-claude", AppType::Claude.as_str())
        .expect("read old claude provider")
        .expect("old claude provider should remain");
    assert!(provider_json_contains_token(&old_claude, "sk-old-claude"));
    let claude_live: Value = read_json_file(&get_claude_settings_path()).expect("read claude live");
    assert_eq!(
        claude_live
            .pointer("/env/ANTHROPIC_AUTH_TOKEN")
            .and_then(|value| value.as_str()),
        Some("sk-old-claude")
    );

    server.shutdown().await;
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
#[tokio::test(flavor = "current_thread")]
async fn bootstrap_upsert_rolls_back_db_when_codex_live_write_fails() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    enable_proprietary_mode_for_test();
    reset_test_fs();
    let home = ensure_test_home();
    let state = test_state();
    seed_existing_claude_state(&state, home);

    let codex_config_path = get_codex_config_path();
    std::fs::create_dir_all(codex_config_path.parent().expect("codex config parent"))
        .expect("create codex dir");
    std::fs::create_dir_all(&codex_config_path)
        .expect("create directory where codex config.toml should be");

    let server = BootstrapTestServer::start(
        StatusCode::OK,
        bootstrap_response("created", "https://api.example.com", "sk-created"),
    )
    .await;
    let _env = EnvGuard::bootstrap(&server.base_url);

    let public_state = cc_switch_lib::proprietary_bootstrap::run_attempt_for_test(&state)
        .await
        .expect("failed provider upsert should be recorded as retryable state");

    assert_eq!(public_state.status.as_deref(), Some("error"));
    assert_no_managed_provider_rows(&state);
    assert_eq!(
        settings_json_current_provider(home, "currentProviderClaude").as_deref(),
        Some("old-claude")
    );
    let claude_live: Value = read_json_file(&get_claude_settings_path()).expect("read claude live");
    assert_eq!(
        claude_live
            .pointer("/env/ANTHROPIC_AUTH_TOKEN")
            .and_then(|value| value.as_str()),
        Some("sk-old-claude")
    );
    assert!(
        !get_codex_auth_path().exists(),
        "failed codex live write should not leave a new auth.json behind"
    );

    server.shutdown().await;
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
#[tokio::test(flavor = "current_thread")]
async fn bootstrap_retry_recovers_after_rolled_back_partial_upsert() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    enable_proprietary_mode_for_test();
    reset_test_fs();
    let home = ensure_test_home();
    let state = test_state();
    seed_existing_claude_state(&state, home);
    seed_invalid_codex_live_config(home);

    let server = BootstrapTestServer::start(
        StatusCode::OK,
        bootstrap_response("created", "https://api.example.com", "sk-created"),
    )
    .await;
    let _env = EnvGuard::bootstrap(&server.base_url);

    let failed_state = cc_switch_lib::proprietary_bootstrap::run_attempt_for_test(&state)
        .await
        .expect("failed provider upsert should be recorded as retryable state");
    assert_eq!(failed_state.status.as_deref(), Some("error"));

    std::fs::write(
        get_codex_config_path(),
        r#"model_provider = "old"
model = "old-model"

[model_providers.old]
name = "Old"
base_url = "https://old.example.com/v1"
wire_api = "responses"
"#,
    )
    .expect("repair codex config");
    let ready_state = cc_switch_lib::proprietary_bootstrap::run_attempt_for_test(&state)
        .await
        .expect("retry should succeed after live config is repaired");

    assert_eq!(ready_state.status.as_deref(), Some("ready"));
    for app in [AppType::Claude, AppType::Codex, AppType::Gemini] {
        let provider = state
            .db
            .get_provider_by_id("managed-newapi", app.as_str())
            .expect("read managed provider")
            .expect("managed provider should exist after retry");
        assert!(provider_json_contains_token(&provider, "sk-created"));
        assert_eq!(
            ProviderService::current(&state, app.clone()).expect("read current provider"),
            "managed-newapi"
        );
    }

    server.shutdown().await;
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
#[tokio::test(flavor = "current_thread")]
async fn startup_failure_with_cached_provider_keeps_provider_and_records_error() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    enable_proprietary_mode_for_test();
    reset_test_fs();
    let _home = ensure_test_home();
    let state = test_state();
    let server = BootstrapTestServer::start(
        StatusCode::OK,
        bootstrap_response("created", "https://api.example.com", "sk-old"),
    )
    .await;
    let _env = EnvGuard::bootstrap(&server.base_url);

    cc_switch_lib::proprietary_bootstrap::run_attempt_for_test(&state)
        .await
        .expect("initial bootstrap should succeed");
    server.set_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        json!({
            "success": false,
            "message": "upstream failed with sk-new"
        }),
    );
    let public_state = cc_switch_lib::proprietary_bootstrap::run_attempt_for_test(&state)
        .await
        .expect("failed bootstrap attempt should record state without hard error");

    assert_eq!(public_state.status.as_deref(), Some("error"));
    let error = public_state.last_error.unwrap_or_default();
    assert!(!error.contains("sk-old"));
    assert!(!error.contains("sk-new"));
    assert!(error.contains("bootstrap http 500"));

    for app in [AppType::Claude, AppType::Codex, AppType::Gemini] {
        let provider = state
            .db
            .get_provider_by_id("managed-newapi", app.as_str())
            .expect("read cached provider")
            .expect("cached provider should remain");
        assert!(provider_json_contains_token(&provider, "sk-old"));
    }

    server.shutdown().await;
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
#[tokio::test(flavor = "current_thread")]
async fn startup_failure_without_cached_provider_records_error_status() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    enable_proprietary_mode_for_test();
    reset_test_fs();
    let _home = ensure_test_home();
    let state = test_state();
    let server = BootstrapTestServer::start(
        StatusCode::INTERNAL_SERVER_ERROR,
        json!({
            "success": false,
            "message": "upstream failed with sk-new"
        }),
    )
    .await;
    let _env = EnvGuard::bootstrap(&server.base_url);

    let public_state = cc_switch_lib::proprietary_bootstrap::run_attempt_for_test(&state)
        .await
        .expect("failed bootstrap attempt should record state without hard error");

    assert_eq!(public_state.status.as_deref(), Some("error"));
    let error = public_state.last_error.unwrap_or_default();
    assert!(!error.contains("sk-new"));
    assert!(error.contains("bootstrap http 500"));
    for app in [AppType::Claude, AppType::Codex, AppType::Gemini] {
        assert!(state
            .db
            .get_provider_by_id("managed-newapi", app.as_str())
            .expect("read provider")
            .is_none());
    }

    server.shutdown().await;
}

#[cfg(all(feature = "proprietary-bootstrap", feature = "test-hooks"))]
#[tokio::test(flavor = "current_thread")]
async fn server_blocked_response_records_blocked_without_provider() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    enable_proprietary_mode_for_test();
    reset_test_fs();
    let _home = ensure_test_home();
    let state = test_state();
    let server = BootstrapTestServer::start(
        StatusCode::FORBIDDEN,
        json!({
            "success": false,
            "message": "device blocked"
        }),
    )
    .await;
    let _env = EnvGuard::bootstrap(&server.base_url);

    let public_state = cc_switch_lib::proprietary_bootstrap::run_attempt_for_test(&state)
        .await
        .expect("blocked bootstrap attempt should record state");

    assert_eq!(public_state.status.as_deref(), Some("blocked"));
    assert_eq!(public_state.last_error.as_deref(), Some("blocked"));
    assert!(state
        .db
        .get_provider_by_id("managed-newapi", AppType::Claude.as_str())
        .expect("read provider")
        .is_none());

    server.shutdown().await;
}

#[cfg(feature = "proprietary-bootstrap")]
#[test]
fn proprietary_mode_locks_public_provider_mutations() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();
    let state = test_state();

    let provider = Provider::with_id(
        "custom".to_string(),
        "Custom".to_string(),
        json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-custom",
                "ANTHROPIC_BASE_URL": "https://api.example.com"
            }
        }),
        None,
    );

    assert!(ProviderService::add(&state, AppType::Claude, provider.clone(), true).is_err());
    assert!(ProviderService::update(&state, AppType::Claude, None, provider.clone()).is_err());
    assert!(ProviderService::delete(&state, AppType::Claude, "custom").is_err());
    assert!(ProviderService::remove_from_live_config(&state, AppType::OpenCode, "custom").is_err());
    assert!(ProviderService::add_custom_endpoint(
        &state,
        AppType::Claude,
        "custom",
        "https://api.backup.example.com".to_string(),
    )
    .is_err());
    assert!(ProviderService::remove_custom_endpoint(
        &state,
        AppType::Claude,
        "custom",
        "https://api.backup.example.com".to_string(),
    )
    .is_err());
    assert!(ProviderService::update_endpoint_last_used(
        &state,
        AppType::Claude,
        "custom",
        "https://api.backup.example.com".to_string(),
    )
    .is_err());
    assert!(ProviderService::sync_current_to_live(&state).is_err());
    assert!(ProviderService::sync_current_provider_for_app(&state, AppType::Claude).is_err());
    assert!(
        ProviderService::import_default_config(&state, AppType::Claude)
            .expect("live import should be skipped in proprietary mode")
            == false
    );
    assert!(ProviderService::update_sort_order(
        &state,
        AppType::Claude,
        vec![ProviderSortUpdate {
            id: "custom".to_string(),
            sort_index: 1,
        }],
    )
    .is_err());

    assert!(ProviderService::upsert_universal(
        &state,
        UniversalProvider::new(
            "u1".to_string(),
            "Universal".to_string(),
            "newapi".to_string(),
            "https://api.example.com".to_string(),
            "sk-universal".to_string(),
        ),
    )
    .is_err());
    assert!(ProviderService::delete_universal(&state, "u1").is_err());
    assert!(ProviderService::sync_universal_to_apps(&state, "u1").is_err());
}

#[cfg(feature = "proprietary-bootstrap")]
#[tokio::test(flavor = "current_thread")]
async fn proprietary_mode_locks_proxy_hot_switch_targets() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();
    let state = test_state();

    let err = state
        .proxy_service
        .hot_switch_provider(AppType::Claude.as_str(), "custom")
        .await
        .expect_err("custom hot switch should be locked");
    assert!(err.contains("NewAPI bootstrap"));

    let err = state
        .proxy_service
        .hot_switch_provider(AppType::OpenCode.as_str(), "managed-newapi")
        .await
        .expect_err("unsupported app hot switch should be locked");
    assert!(err.contains("NewAPI bootstrap"));

    state
        .db
        .save_provider(
            AppType::Claude.as_str(),
            &Provider::with_id(
                "managed-newapi".to_string(),
                "Spoofed Managed".to_string(),
                json!({}),
                None,
            ),
        )
        .expect("seed spoofed managed provider");
    let err = state
        .proxy_service
        .hot_switch_provider(AppType::Claude.as_str(), "managed-newapi")
        .await
        .expect_err("spoofed managed provider hot switch should be locked");
    assert!(err.contains("NewAPI bootstrap"));
}

#[cfg(feature = "proprietary-bootstrap")]
#[test]
fn proprietary_mode_allows_only_managed_switch_for_core_apps() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();
    let state = test_state();

    state
        .db
        .save_provider(AppType::Claude.as_str(), &managed_provider())
        .expect("seed managed provider");

    ProviderService::switch(&state, AppType::Claude, "managed-newapi")
        .expect("managed provider switch should be allowed");

    assert!(ProviderService::switch(&state, AppType::Claude, "other").is_err());
    assert!(ProviderService::switch(&state, AppType::OpenCode, "managed-newapi").is_err());
}

#[cfg(feature = "proprietary-bootstrap")]
#[test]
fn proprietary_mode_current_returns_managed_only_when_present() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();
    let state = test_state();

    state
        .db
        .save_provider(
            AppType::Claude.as_str(),
            &Provider::with_id("custom".to_string(), "Custom".to_string(), json!({}), None),
        )
        .expect("seed custom provider");
    state
        .db
        .set_current_provider(AppType::Claude.as_str(), "custom")
        .expect("set custom current provider");

    assert_eq!(
        ProviderService::current(&state, AppType::Claude).expect("read current provider"),
        ""
    );
    assert_eq!(
        ProviderService::current(&state, AppType::OpenCode).expect("read opencode current"),
        ""
    );

    state
        .db
        .save_provider(
            AppType::Claude.as_str(),
            &Provider::with_id(
                "managed-newapi".to_string(),
                "Spoofed Managed".to_string(),
                json!({}),
                None,
            ),
        )
        .expect("seed spoofed managed provider");
    assert_eq!(
        ProviderService::current(&state, AppType::Claude)
            .expect("spoofed managed provider should not be current"),
        ""
    );
    let claude = ProviderService::list(&state, AppType::Claude).expect("list claude");
    assert!(claude.is_empty());
    assert!(ProviderService::switch(&state, AppType::Claude, "managed-newapi").is_err());

    state
        .db
        .save_provider(AppType::Claude.as_str(), &managed_provider())
        .expect("seed managed provider");
    assert_eq!(
        ProviderService::current(&state, AppType::Claude).expect("read managed current provider"),
        "managed-newapi"
    );
}

#[cfg(feature = "proprietary-bootstrap")]
#[test]
fn proprietary_mode_filters_provider_list_to_managed_core_apps() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();
    let state = test_state();

    state
        .db
        .save_provider(AppType::Claude.as_str(), &managed_provider())
        .expect("seed managed provider");
    state
        .db
        .save_provider(
            AppType::Claude.as_str(),
            &Provider::with_id("custom".to_string(), "Custom".to_string(), json!({}), None),
        )
        .expect("seed custom provider");
    state
        .db
        .save_provider(AppType::OpenCode.as_str(), &managed_provider())
        .expect("seed opencode provider");

    let claude = ProviderService::list(&state, AppType::Claude).expect("list claude");
    assert_eq!(
        claude.keys().cloned().collect::<Vec<_>>(),
        vec!["managed-newapi".to_string()]
    );

    let opencode = ProviderService::list(&state, AppType::OpenCode).expect("list opencode");
    assert!(opencode.is_empty());
}

#[cfg(feature = "proprietary-bootstrap")]
#[test]
fn proprietary_mode_blocks_provider_deeplink_import() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();
    let state = test_state();

    let url = "ccswitch://v1/import?resource=provider&app=claude&name=DeepLink%20Claude&homepage=https%3A%2F%2Fexample.com&endpoint=https%3A%2F%2Fapi.example.com%2Fv1&apiKey=sk-test-claude-key&model=claude-sonnet-4";
    let request = parse_deeplink_url(url).expect("parse deeplink url");
    let err = import_provider_from_deeplink(&state, request)
        .expect_err("provider deeplink import should be locked");
    assert!(err.to_string().contains("NewAPI bootstrap"));
}
