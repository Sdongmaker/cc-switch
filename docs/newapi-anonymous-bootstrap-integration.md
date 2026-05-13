# NewAPI 匿名 Bootstrap 对接文档

## 1. 背景与目标

本文档描述专有版 CC Switch 如何对接 `new-api` 的匿名 bootstrap 接口，实现“安装/启动即开户注册、自动写入唯一 NewAPI provider、严格一设备一账号”的客户端方案。

该方案只在专有构建中启用。默认开源/上游版 CC Switch 不应启用、不应调用 bootstrap，也不应改变现有 provider 管理能力。

### 目标

- 首次启动时自动调用 `POST /api/bootstrap/cc-switch`。
- 本地生成并持久化 `install_id`。
- 每次启动计算稳定的 `device_fingerprint`。
- bootstrap 成功后写入一个固定 NewAPI provider，并设为当前 provider。
- 后续启动幂等复用原账号和原 Token。
- 同设备重装时由服务端 fingerprint 恢复原账号，不重复领取首次额度。
- 专有版锁定为单提供商，禁止新增、导入、删除、切换、deep link provider 导入。

### 非目标

- 不在客户端保存或展示 new-api 控制台账号密码。
- 不创建 new-api 控制台登录态。
- 不改变默认上游版 CC Switch 的 provider 管理体验。
- 不把客户端内置签名密钥视为不可破解安全边界。

## 2. 构建开关与隔离原则

建议新增一个非默认构建能力，命名可采用：

- Rust feature：`proprietary-bootstrap`
- 前端构建变量：`VITE_CC_SWITCH_PROPRIETARY_BOOTSTRAP=true`
- Tauri/Rust 构建注入：
  - `CC_SWITCH_BOOTSTRAP_BASE_URL`
  - `CC_SWITCH_BOOTSTRAP_CLIENT_ID`
  - `CC_SWITCH_BOOTSTRAP_CLIENT_SECRET`

隔离要求：

- `proprietary-bootstrap` 不加入默认 feature。
- 所有专有逻辑集中在独立模块，例如 `src-tauri/src/proprietary_bootstrap/`。
- 前端只通过后端暴露的只读状态判断是否处于专有模式，不在前端硬编码密钥。
- 默认构建下：
  - 不生成 `install_id`。
  - 不调用 bootstrap。
  - 不锁定 provider。
  - 不跳过官方 provider seed/import。
  - 不改变 deep link、provider CRUD、universal provider 行为。

客户端密钥会进入桌面应用二进制，可能被逆向提取。它只用于基础风控和区分专有客户端来源，不应承担付费级防盗刷责任。

## 3. 本地 settings 字段建议

`AppSettings` 存储在设备级 `~/.cc-switch/settings.json`，不参与 WebDAV 同步，适合保存 bootstrap 状态。

建议新增字段：

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub proprietary_bootstrap: Option<ProprietaryBootstrapSettings>,
```

建议结构：

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

字段规则：

- `install_id` 首次启动生成 UUID v4，之后永不主动更换。
- `status` 建议值：`pending`、`ready`、`error`、`blocked`。
- 不在 settings 中单独保存 API Token 的副本；Token 只存在 provider 配置里。
- 重装后 settings 丢失时会生成新的 `install_id`，服务端用 `device_fingerprint` 恢复原账号。

## 4. 设备指纹设计

客户端请求必须同时提交：

- `install_id`：安装级随机标识，存本地。
- `device_fingerprint`：设备级稳定指纹，每次启动计算。

建议 fingerprint 形态：

```text
v1:{platform}:{stable_hash}
```

`stable_hash` 输入建议包含：

- OS 类型和架构。
- 主机名的规范化结果。
- 系统配置目录路径中的稳定部分。
- 可安全读取的系统机器标识；如果平台读取失败，则用可用字段降级。

注意：

- fingerprint 原文会发送给服务端，服务端只保存哈希。
- 不要把用户姓名、完整 home path、邮箱等明显隐私字段放入 fingerprint。
- 如果某平台无法获取机器级 ID，仍然提交 `install_id + 降级 fingerprint`，服务端风控会降低可信度。
- fingerprint 算法升级时使用版本前缀，例如 `v2:`；服务端可按版本记录风险。

## 5. Bootstrap 接口契约

接口由 `new-api` 提供：

```http
POST /api/bootstrap/cc-switch
```

请求 Header：

```http
Content-Type: application/json
X-CCS-Client-Id: cc-switch-proprietary
X-CCS-Timestamp: 1760000000
X-CCS-Nonce: 4f6b6c5c-9d87-4db7-bb5a-2c9bdebd81b4
X-CCS-Signature: hex(hmac-sha256(...))
```

请求 Body：

```json
{
  "install_id": "8e8b6a40-4214-44cb-b82e-4eecf09f42e8",
  "device_fingerprint": "v1:macos:stable-device-fingerprint",
  "client_version": "3.14.1-proprietary.1",
  "platform": "macos",
  "arch": "aarch64",
  "build_channel": "proprietary"
}
```

签名字符串：

```text
METHOD + "\n" +
PATH + "\n" +
X-CCS-Timestamp + "\n" +
X-CCS-Nonce + "\n" +
hex(sha256(raw_body))
```

其中：

- `METHOD` 固定为 `POST`。
- `PATH` 固定为 `/api/bootstrap/cc-switch`。
- `raw_body` 必须是实际发送的 JSON 字节。
- `X-CCS-Signature` 使用构建注入的 `CC_SWITCH_BOOTSTRAP_CLIENT_SECRET` 计算。

成功响应：

```json
{
  "success": true,
  "message": "",
  "data": {
    "action": "created",
    "provider": {
      "id": "managed-newapi",
      "name": "NewAPI",
      "base_url": "https://api.example.com",
      "api_key": "sk-xxxxxxxxxxxxxxxx",
      "models": {
        "claude": {
          "model": "claude-sonnet-4-6",
          "haiku_model": "claude-haiku-4-5-20251001",
          "sonnet_model": "claude-sonnet-4-6",
          "opus_model": "claude-opus-4-7"
        },
        "codex": {
          "model": "gpt-5.4",
          "reasoning_effort": "high"
        },
        "gemini": {
          "model": "gemini-3.1-pro"
        }
      }
    },
    "expires_at": 0
  }
}
```

`action` 枚举：

| 值 | 客户端处理 |
| --- | --- |
| `created` | 首次开户注册，写入 provider，状态设为 `ready` |
| `resumed` | 同一安装重复启动，刷新 provider，状态保持 `ready` |
| `restored` | 同设备重装恢复，写入 provider，状态设为 `ready` |
| `token_rotated` | 服务端补发 Token，覆盖本地 provider 的 API Key |

v1 响应只包含 API Token，不包含登录态、密码、控制台 access token。

## 6. 启动流程

专有版启动时机建议放在 Rust 后端初始化完成、provider 默认导入/官方 seed 之前。

推荐顺序：

1. 加载 `AppSettings`。
2. 如果未启用 `proprietary-bootstrap`，走现有启动流程。
3. 如果启用专有模式：
   - 确保 `proprietary_bootstrap.install_id` 存在；不存在则生成 UUID v4 并保存。
   - 计算 `device_fingerprint`。
   - 启动异步 bootstrap 任务。
   - 跳过 live config 自动导入和官方 provider seed。
4. bootstrap 成功：
   - 用响应的 `provider` 写入固定 provider。
   - 设置当前 provider。
   - 同步 live config。
   - 更新 bootstrap status 为 `ready`。
5. bootstrap 失败：
   - 如果本地已有固定 provider，保留本地 provider，记录错误，不阻断启动。
   - 如果本地没有 provider，状态设为 `error`，前端展示重试入口。
6. 之后每次启动仍调用 bootstrap，作为幂等恢复和 Token 更新机制。

伪流程：

```text
app start
  -> proprietary feature disabled? use normal startup
  -> load/create install_id
  -> compute device_fingerprint
  -> call bootstrap
       -> success: upsert managed provider, set current, sync live
       -> failure with cached provider: keep cached provider
       -> failure without cached provider: show bootstrap error state
```

## 7. Provider 写入规则

固定 provider ID 使用服务端响应的 `provider.id`，默认 `managed-newapi`。同一个 ID 可分别写入 Claude、Codex、Gemini，因为本地 providers 表主键是 `(id, app_type)`。

### Claude provider

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "https://api.example.com",
    "ANTHROPIC_AUTH_TOKEN": "sk-xxxxxxxxxxxxxxxx",
    "ANTHROPIC_MODEL": "claude-sonnet-4-6",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-haiku-4-5-20251001",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4-6",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-4-7"
  }
}
```

### Codex provider

Codex 使用 OpenAI 兼容接口，`base_url` 必须以 `/v1` 结尾：

```json
{
  "auth": {
    "OPENAI_API_KEY": "sk-xxxxxxxxxxxxxxxx"
  },
  "config": "model_provider = \"newapi\"\nmodel = \"gpt-5.4\"\nmodel_reasoning_effort = \"high\"\ndisable_response_storage = true\n\n[model_providers.newapi]\nname = \"NewAPI\"\nbase_url = \"https://api.example.com/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = true"
}
```

### Gemini provider

```json
{
  "env": {
    "GOOGLE_GEMINI_BASE_URL": "https://api.example.com",
    "GEMINI_API_KEY": "sk-xxxxxxxxxxxxxxxx",
    "GEMINI_MODEL": "gemini-3.1-pro"
  }
}
```

写入行为：

- provider 名称固定使用响应 `provider.name`。
- `website_url` 可使用服务端 base URL 或专有站点 URL。
- `category` 建议使用 `managed`。
- `meta.provider_type` 建议使用 `managed_newapi`，便于锁定判断。
- 每次 bootstrap 成功都覆盖本地固定 provider 的 `api_key`、`base_url` 和模型配置。
- 覆盖固定 provider 不视为用户编辑，不触发额外导入。

## 8. 单提供商锁定范围

专有模式下，锁定必须在 Rust 服务层执行，前端隐藏按钮只是体验优化。

必须锁定的入口：

| 入口 | 专有模式行为 |
| --- | --- |
| `ProviderService::add` | 只允许写入 `managed-newapi`，其他 ID 拒绝 |
| `ProviderService::update` | 只允许 bootstrap 模块更新 `managed-newapi` |
| `ProviderService::delete` | 拒绝删除 `managed-newapi` |
| `ProviderService::switch` | 只允许切换到 `managed-newapi` |
| `remove_from_live_config` | 拒绝移除 `managed-newapi` |
| `import_provider_from_deeplink` | provider 类型 deep link 直接拒绝 |
| `init_default_official_providers` | 专有模式跳过 |
| live config 自动导入 | 专有模式跳过或只作为迁移备份，不写入 provider 列表 |
| universal provider 管理 | 专有模式隐藏并拒绝保存 |

前端建议：

- 隐藏“新增供应商”“导入”“复制”“删除”“切换”按钮。
- provider 列表只展示固定 NewAPI provider。
- 不展示自定义 gateway preset。
- 首次网络失败且无 provider 时，展示“正在开户注册/重试”状态，而不是开放手动添加 provider。

## 9. 网络失败与恢复策略

| 场景 | 行为 |
| --- | --- |
| 首次启动无网络，且无本地 provider | 状态 `error`，保留 `install_id`，允许后台和手动重试 |
| 首次启动失败但稍后恢复 | 重试同一 `install_id`，服务端首次创建账号 |
| 后续启动无网络，已有 provider | 使用本地 provider，不弹破坏性错误 |
| 服务端返回 `blocked` | 状态 `blocked`，不继续重试，不展示 Token |
| 服务端返回 `token_rotated` | 覆盖本地 API Key 并同步 live config |
| 服务端返回 `409` | 状态 `error`，提示联系支持，不自动换 `install_id` |

重试建议：

- 启动时立即尝试一次。
- 失败后使用指数退避：1 分钟、5 分钟、15 分钟、1 小时。
- 用户手动点击重试时立即执行，但仍受服务端限流保护。
- 所有失败只记录错误摘要，不记录 API Token。

## 10. 与上游保持可合并

为降低长期维护成本，专有逻辑必须保持窄边界：

- 新模块集中放置 bootstrap 请求、签名、fingerprint、provider upsert。
- 现有 provider 服务只增加少量 `if proprietary_mode_enabled()` 守卫。
- 前端只增加专有模式状态判断，不复制整套 provider UI。
- 不修改现有默认 preset 的含义；专有 provider 由 bootstrap 响应驱动。
- 不把专有服务端地址、client secret、品牌配置提交到仓库默认配置。
- 所有新增测试应同时覆盖“专有模式开启”和“默认模式关闭”。

## 11. 测试清单

Rust/Tauri 测试建议覆盖：

- 默认构建下不生成 `install_id`，不调用 bootstrap，不锁定 provider。
- 专有模式首次启动生成并持久化 `install_id`。
- bootstrap 成功后为 Claude、Codex、Gemini 写入 `managed-newapi` provider。
- 第二次启动不重复创建 provider，只覆盖固定 provider。
- `token_rotated` 后本地 provider API Key 被更新。
- 启动无网络且已有 provider 时继续可用。
- 启动无网络且无 provider 时进入 `error` 状态并可重试。
- provider add/update/delete/switch/remove/deeplink 在专有模式下无法绕过锁定。
- 官方 provider seed 和 live import 在专有模式下不会引入额外 provider。
- 签名字符串与 `new-api` 文档一致，raw body hash 一致。

前端测试建议覆盖：

- 专有模式下不显示新增、导入、复制、删除、切换等入口。
- blocked/error/pending/ready 状态展示合理。
- 默认模式 UI 行为不变。

验收标准：

- v1 客户端只消费 API Token，不依赖 new-api 控制台登录态。
- 同设备重装后服务端返回 `restored` 时，本地能直接恢复唯一 provider。
- 默认上游版 CC Switch 完全不启用该能力。
