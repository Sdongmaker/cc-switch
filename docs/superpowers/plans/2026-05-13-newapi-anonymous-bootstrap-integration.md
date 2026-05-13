# NewAPI Anonymous Bootstrap Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在专有构建中接入 NewAPI 匿名 bootstrap，实现首次启动自动开户注册、写入唯一 `managed-newapi` provider，并在默认开源构建中保持现有行为完全不变。

**Architecture:** 新增后端 `proprietary_bootstrap` 窄模块负责构建开关、settings 状态、设备指纹、签名请求、provider 映射和重试；默认构建走同名 stub，所有入口返回 disabled。启动链路在 provider live import / 官方 seed 之前运行 bootstrap；ProviderService 和 deep link / universal provider 命令在服务层执行专有锁定，前端只消费后端只读状态来隐藏管理入口。

**Tech Stack:** Rust/Tauri、reqwest、serde、uuid、sha2、hmac、hex、React、TypeScript、TanStack Query、Vitest。

---

## Task 1: Build Gate, Settings, and Public Bootstrap State

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/settings.rs`
- Modify: `src-tauri/src/commands/settings.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Create: `src-tauri/src/proprietary_bootstrap/mod.rs`
- Create: `src-tauri/src/commands/proprietary_bootstrap.rs`
- Test: `src-tauri/tests/proprietary_bootstrap.rs`

- [ ] **Step 1: Add the non-default Rust feature and signing dependencies**

Add `proprietary-bootstrap = []` under `[features]`. Add direct dependencies:

```toml
hmac = "0.12"
hex = "0.4"
```

Do not add `proprietary-bootstrap` to `default`.

- [ ] **Step 2: Add persisted settings types**

In `src-tauri/src/settings.rs`, add `proprietary_bootstrap` to `AppSettings` and initialize it to `None` in `Default`.

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProprietaryBootstrapSettings {
    pub install_id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_base_url: Option<String>,
}
```

Field on `AppSettings`:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub proprietary_bootstrap: Option<ProprietaryBootstrapSettings>,
```

- [ ] **Step 3: Preserve bootstrap state during normal settings saves**

In `src-tauri/src/commands/settings.rs`, update `merge_settings_for_save` so an incoming payload with `proprietary_bootstrap: None` keeps the existing value. This mirrors the existing WebDAV-preservation pattern and prevents settings UI saves from deleting `install_id`.

```rust
if incoming.proprietary_bootstrap.is_none() {
    incoming.proprietary_bootstrap = existing.proprietary_bootstrap.clone();
}
```

Add a unit test beside existing settings merge tests:

```rust
#[test]
fn save_settings_should_preserve_existing_proprietary_bootstrap_when_payload_omits_it() {
    let existing = AppSettings {
        proprietary_bootstrap: Some(ProprietaryBootstrapSettings {
            install_id: "install-123".to_string(),
            status: "ready".to_string(),
            ..ProprietaryBootstrapSettings::default()
        }),
        ..AppSettings::default()
    };

    let merged = merge_settings_for_save(AppSettings::default(), &existing);
    assert_eq!(
        merged.proprietary_bootstrap.as_ref().map(|s| s.install_id.as_str()),
        Some("install-123")
    );
}
```

- [ ] **Step 4: Add a backend public state command**

Create `src-tauri/src/commands/proprietary_bootstrap.rs` with two commands:

```rust
#[tauri::command]
pub async fn get_proprietary_bootstrap_state() -> Result<crate::proprietary_bootstrap::PublicState, String>;

#[tauri::command]
pub async fn retry_proprietary_bootstrap(
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<crate::proprietary_bootstrap::PublicState, String>;
```

Register the command module in `commands/mod.rs` and both commands in `lib.rs` `generate_handler!`. Declare `pub mod proprietary_bootstrap;` in `lib.rs` so integration tests can verify default-build isolation. Default builds must compile and return `enabled: false`; feature builds return `enabled: true` and the persisted status fields without exposing API tokens.

- [ ] **Step 5: Add default-build tests**

Create `src-tauri/tests/proprietary_bootstrap.rs` with tests compiled in the default feature set:

```rust
#[test]
fn default_build_reports_proprietary_bootstrap_disabled() {
    assert!(!cc_switch_lib::proprietary_bootstrap::is_enabled());
    let state = cc_switch_lib::proprietary_bootstrap::public_state();
    assert!(!state.enabled);
    assert!(state.status.is_none());
}
```

Run:

```bash
cd src-tauri
cargo test default_build_reports_proprietary_bootstrap_disabled
```

Expected: the test passes without creating `~/.cc-switch/settings.json` bootstrap fields.

---

## Task 2: Bootstrap Client, Provider Mapping, and Startup Flow

**Files:**
- Create: `src-tauri/src/proprietary_bootstrap/client.rs`
- Create: `src-tauri/src/proprietary_bootstrap/fingerprint.rs`
- Create: `src-tauri/src/proprietary_bootstrap/provider.rs`
- Create: `src-tauri/src/proprietary_bootstrap/guard.rs`
- Modify: `src-tauri/src/proprietary_bootstrap/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/services/provider/mod.rs`
- Test: `src-tauri/tests/proprietary_bootstrap.rs`

- [ ] **Step 1: Implement the feature-enabled configuration loader**

In `proprietary_bootstrap::config`, read `CC_SWITCH_BOOTSTRAP_BASE_URL`, `CC_SWITCH_BOOTSTRAP_CLIENT_ID`, and `CC_SWITCH_BOOTSTRAP_CLIENT_SECRET` from runtime env first, then `option_env!`. When the feature is enabled and any value is missing, return a typed error and record `status = "error"` with a short message.

- [ ] **Step 2: Implement deterministic signing over actual request bytes**

The signing helper must hash the same JSON bytes sent by reqwest. Keep production callers inside the module; expose `signature_string` and `sign` to integration tests when `test-hooks` is enabled.

```rust
pub fn signature_string(timestamp: i64, nonce: &str, raw_body: &[u8]) -> String {
    let body_hash = hex::encode(sha2::Sha256::digest(raw_body));
    format!("POST\n/api/bootstrap/cc-switch\n{timestamp}\n{nonce}\n{body_hash}")
}
```

Add this test:

```rust
#[test]
fn signature_string_matches_newapi_contract() {
    let raw_body = br#"{"install_id":"i"}"#;
    let text = cc_switch_lib::proprietary_bootstrap::signature_string(1760000000, "nonce-1", raw_body);
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
```

- [ ] **Step 3: Implement privacy-preserving device fingerprinting**

Fingerprint format is `v1:{platform}:{stable_hash}`. Hash inputs are OS, arch, normalized hostname, app config directory path components after `.cc-switch`, and platform machine ID when available:

```rust
#[cfg(target_os = "linux")]
// read /etc/machine-id, fallback to /var/lib/dbus/machine-id

#[cfg(target_os = "windows")]
// read HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid using winreg

#[cfg(target_os = "macos")]
// run /usr/sbin/ioreg -rd1 -c IOPlatformExpertDevice and parse IOPlatformUUID
```

Never include the raw home directory, user name, email, or API key in the fingerprint source string. If all machine ID sources fail, still submit the hash of platform, arch, hostname fallback, and app config stable suffix.

- [ ] **Step 4: Implement bootstrap request and response handling**

POST to `{base_url}/api/bootstrap/cc-switch` with headers from the integration doc. Request body fields:

```json
{
  "install_id": "...",
  "device_fingerprint": "v1:macos:...",
  "client_version": "3.14.1-proprietary.1",
  "platform": "macos",
  "arch": "aarch64",
  "build_channel": "proprietary"
}
```

Accepted success actions: `created`, `resumed`, `restored`, `token_rotated`. A response with `provider.id != "managed-newapi"` is a protocol error and must not write providers.

- [ ] **Step 5: Map the response into exactly three managed providers**

Add an internal `ProviderService::upsert_managed_newapi_provider` used only by `proprietary_bootstrap::provider`. It writes `managed-newapi` for `Claude`, `Codex`, and `Gemini`, sets it current in DB and local settings, and syncs each live config.

Provider invariants:

```rust
id = "managed-newapi"
category = Some("managed".to_string())
meta.provider_type = Some("managed_newapi".to_string())
name = response.provider.name
website_url = Some(response.provider.base_url.clone())
```

Codex base URL must end with `/v1`. Claude and Gemini use the base URL exactly as returned after trimming trailing slashes.

- [ ] **Step 6: Insert startup bootstrap before provider import/seed**

In `src-tauri/src/lib.rs`, after `AppState::new(db)` and proxy handle setup, run:

```rust
let proprietary_mode = crate::proprietary_bootstrap::is_enabled();
if proprietary_mode {
    if let Err(err) = crate::proprietary_bootstrap::run_startup(&app_state) {
        log::warn!("Proprietary bootstrap startup failed: {err}");
    }
}
```

Wrap existing provider live import, official provider seed, OpenCode/OpenClaw/Hermes import, and OMO import sections so they only run when `!proprietary_mode`. Skill repo initialization, skill migration, database migration, logs, proxy state, and non-provider systems keep their current startup behavior.

- [ ] **Step 7: Implement failure and retry semantics**

Startup attempts once immediately. On failure:

- if `managed-newapi` exists for any supported app, keep it and set `status = "error"` with `last_error`;
- if no managed provider exists, set `status = "error"` and expose retry to the frontend;
- if the server returns blocked semantics, set `status = "blocked"` and skip background retry;
- record only short error summaries and never log or persist the API token outside provider config.

Feature builds spawn background retries after failures using fixed delays: 60s, 300s, 900s, 3600s. `retry_proprietary_bootstrap` runs immediately and then refreshes the public state.

---

## Task 3: Service-Layer Locking and Frontend Locked Mode

**Files:**
- Modify: `src-tauri/src/services/provider/mod.rs`
- Modify: `src-tauri/src/deeplink/provider.rs`
- Modify: `src-tauri/src/commands/provider.rs`
- Modify: `src-tauri/src/commands/deeplink.rs`
- Modify: `src/types.ts`
- Modify: `src/lib/schemas/settings.ts`
- Create: `src/lib/api/proprietaryBootstrap.ts`
- Create: `src/lib/proprietaryBootstrap.ts`
- Modify: `src/App.tsx`
- Modify: `src/components/providers/ProviderList.tsx`
- Modify: `src/components/providers/ProviderActions.tsx`
- Modify: `src/components/providers/ProviderEmptyState.tsx`
- Modify: `src/components/providers/AddProviderDialog.tsx`
- Modify: `src/components/universal/UniversalProviderPanel.tsx`
- Modify: `src/components/DeepLinkImportDialog.tsx`
- Modify: `src/i18n/locales/zh.json`
- Modify: `src/i18n/locales/en.json`
- Modify: `src/i18n/locales/ja.json`
- Test: `src/lib/proprietaryBootstrap.test.ts`

- [ ] **Step 1: Lock all provider mutation surfaces in Rust**

In proprietary mode, public service methods must behave as follows:

| Method | Behavior |
| --- | --- |
| `ProviderService::list` | For Claude/Codex/Gemini return only `managed-newapi`; for other apps return an empty map. |
| `ProviderService::add` | Reject all public adds. Bootstrap uses only the internal upsert method from Task 2. |
| `ProviderService::update` | Reject all public updates, including attempts to spoof `managed_newapi`. |
| `ProviderService::delete` | Reject all deletes. |
| `ProviderService::switch` | Allow only `managed-newapi` for Claude/Codex/Gemini; reject all other switches. |
| `ProviderService::remove_from_live_config` | Reject all removals. |
| `ProviderService::update_sort_order` | Reject updates containing any id other than `managed-newapi`; otherwise no-op success. |
| custom endpoint mutation methods | Reject add/remove/update endpoint mutations. |
| universal provider upsert/delete/sync | Reject with a proprietary-mode message. |
| live import methods | Return `Ok(false)` or `Ok(0)` without writing providers. |

Use a single helper in `proprietary_bootstrap::guard` so the error message and supported app list have one source of truth.

- [ ] **Step 2: Block provider deep links on the Rust side**

In `import_provider_from_deeplink`, return an error before merging remote configs when proprietary mode is enabled:

```rust
if crate::proprietary_bootstrap::is_enabled() {
    return Err(AppError::Message(
        "专有版由 NewAPI bootstrap 管理供应商，不能通过 deep link 导入供应商".to_string(),
    ));
}
```

`parse_deeplink` and `merge_deeplink_config` may still parse for display, but `import_from_deeplink` and `import_from_deeplink_unified` must reject provider imports.

- [ ] **Step 3: Add frontend state types and pure selectors**

Extend `Settings` and `settingsSchema` with optional `proprietaryBootstrap`. Add `src/lib/api/proprietaryBootstrap.ts`:

```ts
export interface ProprietaryBootstrapState {
  enabled: boolean;
  status?: "pending" | "ready" | "error" | "blocked" | string;
  lastAction?: string | null;
  lastSuccessAt?: number | null;
  lastAttemptAt?: number | null;
  lastError?: string | null;
  providerBaseUrl?: string | null;
}
```

Add pure helpers in `src/lib/proprietaryBootstrap.ts`:

```ts
export const PROPRIETARY_SUPPORTED_APPS = ["claude", "codex", "gemini"] as const;
export function isProprietaryMode(state?: ProprietaryBootstrapState | null): boolean {
  return state?.enabled === true;
}
export function isProprietarySupportedApp(appId: AppId): boolean {
  return PROPRIETARY_SUPPORTED_APPS.includes(appId as any);
}
```

Add Vitest coverage for supported apps and disabled/default state.

- [ ] **Step 4: Hide provider management UI while preserving read-only use**

In `App.tsx`, fetch `get_proprietary_bootstrap_state` with TanStack Query. When enabled:

- if `activeApp` is not Claude/Codex/Gemini, switch to `claude`;
- hide the universal provider view and any route/button that opens it;
- do not open `AddProviderDialog`;
- pass `isProprietaryLocked` and `bootstrapState` into provider components;
- keep settings, prompts, skills, MCP, usage read-only surfaces unchanged unless they directly mutate providers.

In `ProviderList`, disable drag sorting in proprietary mode, show only backend-filtered providers, pass no `onCreate` and no `onImport`, and show the bootstrap empty/error state when no provider exists.

In `ProviderActions`, hide edit, duplicate, delete, remove-from-config, configure-usage, failover, and switch buttons. Keep model test and provider website open if available. For the managed current provider, show a disabled “已托管”/“Managed” state.

- [ ] **Step 5: Add retry and status UI**

In `ProviderEmptyState`, when `bootstrapState.enabled` is true:

- `pending`: show “正在开户注册” and a disabled retry button;
- `error`: show short `lastError` and an active retry button calling `retry_proprietary_bootstrap`;
- `blocked`: show “设备暂不可用，请联系支持” with no automatic retry;
- `ready` with no provider: show “正在同步供应商” and active retry.

Add locale strings under a `proprietaryBootstrap` namespace in `zh.json`, `en.json`, and `ja.json`.

---

## Task 4: Verification and Acceptance Tests

**Files:**
- Test: `src-tauri/tests/proprietary_bootstrap.rs`
- Test: `src-tauri/tests/provider_service.rs`
- Test: `src-tauri/tests/provider_commands.rs`
- Test: `src-tauri/tests/deeplink_import.rs`
- Test: `src/lib/proprietaryBootstrap.test.ts`

- [ ] **Step 1: Cover default build isolation**

Run:

```bash
cd src-tauri
cargo test provider_service provider_commands deeplink_import
```

Expected: existing tests keep passing without `proprietary-bootstrap`; no test creates `proprietaryBootstrap` in settings unless it explicitly constructs the value.

- [ ] **Step 2: Cover feature build bootstrap success**

Using an in-test Axum server, return a `created` response with `managed-newapi`. The test binds a local port, sets `CC_SWITCH_BOOTSTRAP_BASE_URL`, `CC_SWITCH_BOOTSTRAP_CLIENT_ID`, and `CC_SWITCH_BOOTSTRAP_CLIENT_SECRET` in-process, then runs the bootstrap.

```bash
cd src-tauri
cargo test --features proprietary-bootstrap,test-hooks bootstrap_success_writes_managed_newapi_for_core_apps
```

Assertions:

- `settings.proprietary_bootstrap.install_id` is generated and reused;
- Claude/Codex/Gemini each contain exactly one `managed-newapi` provider;
- `currentProviderClaude`, `currentProviderCodex`, and `currentProviderGemini` are all `managed-newapi`;
- Codex provider config uses `/v1`;
- no official providers are seeded.

- [ ] **Step 3: Cover token rotation and cached failure**

Add feature-build tests with these exact scenarios:

- `token_rotated_overwrites_existing_api_key`: seed `managed-newapi` with `sk-old`, return action `token_rotated` with `sk-new`, assert all three app providers contain `sk-new`.
- `startup_failure_with_cached_provider_keeps_provider_and_records_error`: seed `managed-newapi`, make the test server return HTTP 500, assert providers remain and `status == "error"`.
- `startup_failure_without_cached_provider_records_error_status`: start from an empty database, make the test server return HTTP 500, assert no provider is created and `status == "error"`.

Each test must assert that `last_error` does not contain `sk-old`, `sk-new`, or any response token.

- [ ] **Step 4: Cover locked provider entry points**

Add feature-build assertions for `ProviderService::add`, `update`, `delete`, `switch`, `remove_from_live_config`, `upsert_universal`, and `import_provider_from_deeplink`. All must return an error except switching to existing `managed-newapi` for Claude/Codex/Gemini.

- [ ] **Step 5: Run frontend verification**

Run:

```bash
pnpm test:unit src/lib/proprietaryBootstrap.test.ts
pnpm typecheck
pnpm build:renderer
```

Expected: selectors pass, TypeScript accepts the new settings/bootstrap types, and the renderer builds.

---

## Assumptions and Defaults

- 专有锁定范围覆盖整个 provider 管理面：Claude/Codex/Gemini 只展示 `managed-newapi`，OpenCode/OpenClaw/Hermes/Claude Desktop provider 管理和 universal provider 管理在专有模式下隐藏并在后端拒绝写入。
- `managed-newapi` 是唯一允许的专有 provider ID；服务端返回其他 ID 时客户端视为协议错误。
- 默认构建不生成 `install_id`、不调用 bootstrap、不跳过 live import/official seed、不锁定 provider CRUD/deep link/universal provider。
- 前端不使用 `VITE_CC_SWITCH_PROPRIETARY_BOOTSTRAP` 作为运行时判断来源；运行时只信任后端 `get_proprietary_bootstrap_state`。
- API token 只写入 provider 配置，不复制到 bootstrap settings、日志、toast、错误详情或前端 bootstrap 状态。
