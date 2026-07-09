use crate::app::constants::{config, messages, network_config, paths};
use crate::app::core::tun_profile::{TunProfile, TunProxyOptions};
use crate::app::system::config_service;
use crate::entity::config_model;
use crate::utils::config_util::ConfigUtil;
use crate::utils::http_client;
use crate::utils::proxy_util::{disable_system_proxy, enable_system_proxy, DEFAULT_BYPASS_LIST};

/// 系统代理端口（D-PROXY）：生产用 OS 实现，测试用 Recording。
pub trait SystemProxyPort: Send + Sync {
    fn enable(&self, host: &str, port: u16, bypass: Option<&str>) -> Result<(), String>;
    fn disable(&self) -> Result<(), String>;
}

/// 默认系统代理适配器（委托 `proxy_util`）。
pub struct OsSystemProxy;

impl SystemProxyPort for OsSystemProxy {
    fn enable(&self, host: &str, port: u16, bypass: Option<&str>) -> Result<(), String> {
        enable_system_proxy(host, port, bypass).map_err(|e| e.to_string())
    }

    fn disable(&self) -> Result<(), String> {
        disable_system_proxy().map_err(|e| e.to_string())
    }
}

/// 可记录调用的假系统代理（测试/E2E hermetic）。
#[derive(Debug, Default)]
pub struct RecordingSystemProxy {
    pub enables: std::sync::Mutex<Vec<(String, u16, Option<String>)>>,
    pub disables: std::sync::Mutex<u32>,
}

impl SystemProxyPort for RecordingSystemProxy {
    fn enable(&self, host: &str, port: u16, bypass: Option<&str>) -> Result<(), String> {
        self.enables
            .lock()
            .unwrap()
            .push((host.to_string(), port, bypass.map(|s| s.to_string())));
        Ok(())
    }

    fn disable(&self) -> Result<(), String> {
        *self.disables.lock().unwrap() += 1;
        Ok(())
    }
}
use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::time::Duration;
use tracing::{error, info, warn};
use url::Url;

#[derive(Debug, Clone)]
pub struct ProxyRuntimeState {
    pub proxy_port: u16,
    pub allow_lan_access: bool,
    pub system_proxy_enabled: bool,
    pub tun_enabled: bool,
    pub system_proxy_bypass: String,
    pub tun_options: TunProxyOptions,
}

impl ProxyRuntimeState {
    pub fn derived_mode(&self) -> String {
        if self.tun_enabled {
            "tun".to_string()
        } else if self.system_proxy_enabled {
            "system".to_string()
        } else {
            "manual".to_string()
        }
    }
}

pub(crate) fn resolve_proxy_listen_address(state: &ProxyRuntimeState) -> &'static str {
    if state.allow_lan_access {
        network_config::DEFAULT_LISTEN_ADDRESS
    } else {
        network_config::DEFAULT_CLASH_API_ADDRESS
    }
}

pub(crate) fn build_inbounds_for_state(state: &ProxyRuntimeState) -> Vec<config_model::Inbound> {
    if state.tun_enabled {
        let mut inbounds =
            TunProfile::from_options(&state.tun_options, None).to_inbounds(state.proxy_port);
        if let Some(mixed) = inbounds.get_mut(0) {
            mixed.listen = Some(resolve_proxy_listen_address(state).to_string());
            mixed.set_system_proxy = Some(state.system_proxy_enabled);
        }
        return inbounds;
    }

    vec![config_model::Inbound {
        r#type: config::DEFAULT_INBOUND_TYPE.to_string(),
        tag: config::DEFAULT_INBOUND_TAG.to_string(),
        listen: Some(resolve_proxy_listen_address(state).to_string()),
        interface_name: None,
        listen_port: Some(state.proxy_port),
        address: None,
        auto_route: None,
        strict_route: None,
        stack: None,
        mtu: None,
        route_address: None,
        route_exclude_address: None,
        set_system_proxy: Some(state.system_proxy_enabled),
    }]
}

use crate::app::storage::enhanced_storage_service::db_get_app_config;
use tauri::{AppHandle, Runtime};

async fn load_allow_lan_access<R: Runtime>(app_handle: &AppHandle<R>) -> bool {
    db_get_app_config(app_handle.clone())
        .await
        .map(|config| config.allow_lan_access)
        .unwrap_or(false)
}

/// 构造系统代理模式运行态（纯逻辑，供命令与单测复用）。
pub(crate) fn build_system_proxy_runtime_state(
    port: u16,
    allow_lan_access: bool,
    system_proxy_bypass: String,
) -> ProxyRuntimeState {
    ProxyRuntimeState {
        proxy_port: port,
        allow_lan_access,
        system_proxy_enabled: true,
        tun_enabled: false,
        system_proxy_bypass,
        tun_options: TunProxyOptions::default(),
    }
}

/// 构造手动代理模式运行态（纯逻辑）。
pub(crate) fn build_manual_proxy_runtime_state(
    port: u16,
    allow_lan_access: bool,
) -> ProxyRuntimeState {
    ProxyRuntimeState {
        proxy_port: port,
        allow_lan_access,
        system_proxy_enabled: false,
        tun_enabled: false,
        system_proxy_bypass: DEFAULT_BYPASS_LIST.to_string(),
        tun_options: TunProxyOptions::default(),
    }
}

/// 构造 TUN 模式运行态（纯逻辑）。
pub(crate) fn build_tun_proxy_runtime_state(
    port: u16,
    allow_lan_access: bool,
    tun_options: TunProxyOptions,
) -> ProxyRuntimeState {
    ProxyRuntimeState {
        proxy_port: port,
        allow_lan_access,
        system_proxy_enabled: false,
        tun_enabled: true,
        system_proxy_bypass: DEFAULT_BYPASS_LIST.to_string(),
        tun_options,
    }
}

/// 将 runtime state 的 inbounds 写入已有 sing-box 配置文件（纯 FS，无系统代理副作用）。
pub fn write_inbounds_for_state(config_path: &Path, state: &ProxyRuntimeState) -> Result<(), String> {
    let config_path_str = config_path
        .to_str()
        .ok_or_else(|| "配置文件路径包含无效字符".to_string())?;

    let mut json_util = ConfigUtil::new(config_path_str)
        .map_err(|e| format!("{}: {}", messages::ERR_CONFIG_READ_FAILED, e))?;

    let inbounds = build_inbounds_for_state(state);
    json_util.update_key(
        vec!["inbounds"],
        serde_json::to_value(inbounds).map_err(|e| format!("序列化配置失败: {}", e))?,
    );
    json_util
        .save_to_file()
        .map_err(|e| format!("{}: {}", messages::ERR_CONFIG_READ_FAILED, e))?;
    Ok(())
}

/// 应用系统代理开关（可注入端口，默认 OS）。
pub fn apply_system_proxy_for_state(
    state: &ProxyRuntimeState,
    proxy: &dyn SystemProxyPort,
) -> Result<(), String> {
    if state.system_proxy_enabled {
        let bypass = state.system_proxy_bypass.trim();
        let normalized_bypass = if bypass.is_empty() {
            DEFAULT_BYPASS_LIST.to_string()
        } else {
            bypass.to_string()
        };
        proxy
            .enable(
                network_config::DEFAULT_CLASH_API_ADDRESS,
                state.proxy_port,
                Some(normalized_bypass.as_str()),
            )
            .map_err(|e| format!("设置系统代理失败: {}", e))?;
        info!(
            "系统代理已启用，端口 {}，绕过列表: {}",
            state.proxy_port, normalized_bypass
        );
    } else if let Err(err) = proxy.disable() {
        warn!("关闭系统代理失败: {}", err);
    }
    Ok(())
}

pub async fn apply_proxy_runtime_state<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    state: &ProxyRuntimeState,
) -> Result<(), String> {
    apply_proxy_runtime_state_with(app_handle, state, &OsSystemProxy).await
}

/// 可注入 SystemProxy 的运行态应用（E2E/单测 hermetic）。
pub async fn apply_proxy_runtime_state_with<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    state: &ProxyRuntimeState,
    proxy: &dyn SystemProxyPort,
) -> Result<(), String> {
    config_service::ensure_singbox_config(app_handle)
        .await
        .map_err(|e| format!("准备配置失败: {}", e))?;

    // 从数据库获取配置路径
    let app_config = db_get_app_config(app_handle.clone())
        .await
        .map_err(|e| format!("获取应用配置失败: {}", e))?;

    let config_path = if let Some(path_str) = app_config.active_config_path {
        std::path::PathBuf::from(path_str)
    } else {
        paths::get_config_dir().join("config.json")
    };

    write_inbounds_for_state(&config_path, state)?;
    apply_system_proxy_for_state(state, proxy)?;
    Ok(())
}

// 修改代理模式为系统代理
#[tauri::command]
pub async fn set_system_proxy<R: Runtime>(
    app_handle: AppHandle<R>,
    port: u16,
    system_proxy_bypass: Option<String>,
) -> Result<(), String> {
    let allow_lan_access = load_allow_lan_access(&app_handle).await;
    let runtime_state = build_system_proxy_runtime_state(
        port,
        allow_lan_access,
        system_proxy_bypass.unwrap_or_else(|| DEFAULT_BYPASS_LIST.to_string()),
    );
    apply_proxy_runtime_state(&app_handle, &runtime_state).await
}

// 设置手动代理模式（不自动设置系统代理）
#[tauri::command]
pub async fn set_manual_proxy<R: Runtime>(app_handle: AppHandle<R>, port: u16) -> Result<(), String> {
    let allow_lan_access = load_allow_lan_access(&app_handle).await;
    let runtime_state = build_manual_proxy_runtime_state(port, allow_lan_access);
    apply_proxy_runtime_state(&app_handle, &runtime_state).await
}

// 修改TUN 模式为代理模式
#[tauri::command]
pub async fn set_tun_proxy<R: Runtime>(
    app_handle: AppHandle<R>,
    port: u16,
    tun_options: Option<TunProxyOptions>,
) -> Result<(), String> {
    let allow_lan_access = load_allow_lan_access(&app_handle).await;
    let runtime_state =
        build_tun_proxy_runtime_state(port, allow_lan_access, tun_options.unwrap_or_default());
    apply_proxy_runtime_state(&app_handle, &runtime_state).await
}

/// 纯逻辑：在 sing-box 配置 JSON 上写入 DNS strategy（无 FS / AppHandle）。
pub(crate) fn apply_dns_strategy_to_config(config: &mut Value, prefer_ipv6: bool) -> Result<(), String> {
    let strategy_value = if prefer_ipv6 {
        "prefer_ipv6"
    } else {
        "ipv4_only"
    };

    // 兼容不同用户的配置：如果没有 dns 区块则补充一个基础结构
    let dns_object = if let Some(obj) = config
        .as_object_mut()
        .and_then(|obj| obj.get_mut("dns"))
        .and_then(|dns| dns.as_object_mut())
    {
        obj
    } else {
        let dns_value = json!({
            "servers": [],
            "strategy": strategy_value
        });
        config
            .as_object_mut()
            .ok_or_else(|| "配置文件结构异常，无法写入DNS配置".to_string())?
            .insert("dns".to_string(), dns_value);
        config
            .as_object_mut()
            .and_then(|obj| obj.get_mut("dns"))
            .and_then(|dns| dns.as_object_mut())
            .ok_or_else(|| "创建DNS配置失败".to_string())?
    };

    dns_object.insert(
        "strategy".to_string(),
        Value::String(strategy_value.to_string()),
    );

    // 同步更新所有 DNS 服务器的 strategy，确保优先级实时生效
    if let Some(servers) = dns_object.get_mut("servers").and_then(|s| s.as_array_mut()) {
        for server in servers.iter_mut() {
            if let Some(server_obj) = server.as_object_mut() {
                // 只在存在 address 的条目上更新，避免污染特殊类型（如 rcode）
                if server_obj.get("address").is_some() {
                    server_obj.insert(
                        "strategy".to_string(),
                        Value::String(strategy_value.to_string()),
                    );
                }
            }
        }
    }

    Ok(())
}

/// 纯 FS：对指定配置文件应用 DNS strategy。
pub(crate) fn update_dns_strategy_on_path(config_path: &Path, prefer_ipv6: bool) -> Result<(), String> {
    let content =
        fs::read_to_string(config_path).map_err(|e| format!("读取配置文件失败: {}", e))?;
    let mut config: Value =
        serde_json::from_str(&content).map_err(|e| format!("解析配置文件失败: {}", e))?;
    apply_dns_strategy_to_config(&mut config, prefer_ipv6)?;
    let serialized =
        serde_json::to_string_pretty(&config).map_err(|e| format!("序列化配置失败: {}", e))?;
    fs::write(config_path, serialized).map_err(|e| format!("保存配置文件失败: {}", e))?;
    Ok(())
}

pub async fn update_dns_strategy<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    prefer_ipv6: bool,
) -> Result<(), String> {
    // 从数据库获取配置路径
    let app_config = db_get_app_config(app_handle.clone())
        .await
        .map_err(|e| format!("获取应用配置失败: {}", e))?;

    let config_path = if let Some(path_str) = app_config.active_config_path {
        std::path::PathBuf::from(path_str)
    } else {
        paths::get_config_dir().join("config.json")
    };

    update_dns_strategy_on_path(&config_path, prefer_ipv6)
}

// 切换 IPV6版本模式
#[tauri::command]
pub async fn toggle_ip_version<R: Runtime>(
    app_handle: AppHandle<R>,
    prefer_ipv6: bool,
) -> Result<(), String> {
    info!(
        "开始切换IP版本模式: {}",
        if prefer_ipv6 { "IPv6优先" } else { "仅IPv4" }
    );

    update_dns_strategy(&app_handle, prefer_ipv6).await?;

    info!(
        "✅ IP版本模式已成功切换为: {}",
        if prefer_ipv6 { "IPv6优先" } else { "仅IPv4" }
    );

    Ok(())
}

// 获取API令牌
#[tauri::command]
pub fn get_api_token() -> String {
    // 目前返回空字符串
    "".to_string()
}

pub(crate) fn build_controller_url(port: u16, path: &str) -> String {
    format!("http://127.0.0.1:{}/{}", port, path.trim_start_matches('/'))
}

async fn fetch_controller_json(port: u16, path: &str) -> Result<Value, String> {
    let url = build_controller_url(port, path);
    http_client::get_json::<Value>(&url)
        .await
        .map_err(|e| format!("请求 {} 失败: {}", path, e))
}

async fn put_controller(port: u16, path: &str) -> Result<(), String> {
    let url = build_controller_url(port, path);
    let client = http_client::get_client();
    let response = client
        .put(&url)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("请求 {} 失败: {}", path, e))?;

    if response.status().is_success() {
        return Ok(());
    }

    Err(format!(
        "请求 {} 失败，HTTP状态码: {}",
        path,
        response.status()
    ))
}

async fn patch_controller_json(port: u16, path: &str, data: &Value) -> Result<(), String> {
    let url = build_controller_url(port, path);
    let client = http_client::get_client();
    let response = client
        .patch(&url)
        .json(data)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("请求 {} 失败: {}", path, e))?;

    if response.status().is_success() {
        return Ok(());
    }

    Err(format!(
        "请求 {} 失败，HTTP状态码: {}",
        path,
        response.status()
    ))
}

async fn delete_controller(port: u16, path: &str) -> Result<(), String> {
    let url = build_controller_url(port, path);
    let client = http_client::get_client();
    let response = client
        .delete(&url)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("请求 {} 失败: {}", path, e))?;

    if response.status().is_success() {
        return Ok(());
    }

    Err(format!(
        "请求 {} 失败，HTTP状态码: {}",
        path,
        response.status()
    ))
}

// 获取代理节点列表
#[tauri::command]
pub async fn get_proxies(port: u16) -> Result<Value, String> {
    match fetch_controller_json(port, "proxies").await {
        Ok(data) => Ok(data),
        Err(e) => {
            error!("获取代理列表失败: {}", e);
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn get_proxy_providers(port: u16) -> Result<Value, String> {
    match fetch_controller_json(port, "providers/proxies").await {
        Ok(data) => Ok(data),
        Err(e) => {
            error!("获取代理 provider 列表失败: {}", e);
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn update_proxy_provider(provider: String, port: u16) -> Result<(), String> {
    let path = format!("providers/proxies/{}", urlencoding::encode(&provider));
    put_controller(port, &path).await
}

#[tauri::command]
pub async fn get_rule_providers(port: u16) -> Result<Value, String> {
    match fetch_controller_json(port, "providers/rules").await {
        Ok(data) => Ok(data),
        Err(e) => {
            error!("获取规则 provider 列表失败: {}", e);
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn update_rule_provider(provider: String, port: u16) -> Result<(), String> {
    let path = format!("providers/rules/{}", urlencoding::encode(&provider));
    put_controller(port, &path).await
}

#[tauri::command]
pub async fn toggle_rule_disabled(index: usize, disabled: bool, port: u16) -> Result<(), String> {
    let payload = json!({ index.to_string(): disabled });
    patch_controller_json(port, "rules/disable", &payload).await
}

#[tauri::command]
pub async fn close_all_connections(port: u16) -> Result<(), String> {
    delete_controller(port, "connections").await
}

#[tauri::command]
pub async fn close_connection(id: String, port: u16) -> Result<(), String> {
    let path = format!("connections/{}", urlencoding::encode(&id));
    delete_controller(port, &path).await
}

// 切换代理节点
#[tauri::command]
pub async fn change_proxy(group: String, proxy: String, port: u16) -> Result<(), String> {
    let url = format!("http://127.0.0.1:{}/proxies/{}", port, group);
    let data = json!({
        "name": proxy
    });

    let client = http_client::get_client();
    match client
        .put(&url)
        .json(&data)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(response) => {
            if response.status().is_success() {
                info!("代理节点已切换: {} -> {}", group, proxy);
                Ok(())
            } else {
                let error_msg = format!("切换代理节点失败，HTTP状态码: {}", response.status());
                error!("{}", error_msg);
                Err(error_msg)
            }
        }
        Err(e) => {
            let error_msg = format!("切换代理节点请求失败: {}", e);
            error!("{}", error_msg);
            Err(error_msg)
        }
    }
}

pub(crate) async fn resolve_group_nodes(port: u16, group: &str) -> Result<Vec<String>, String> {
    let data = get_proxies(port).await?;

    let group_value = data
        .get("proxies")
        .and_then(|v| v.get(group))
        .ok_or_else(|| format!("未找到代理组: {}", group))?;

    let nodes = group_value
        .get("all")
        .and_then(|v| v.as_array())
        .ok_or_else(|| format!("代理组 {} 不包含可测试节点列表", group))?
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect::<Vec<_>>();

    if nodes.is_empty() {
        return Err(format!("代理组 {} 节点为空", group));
    }

    Ok(nodes)
}

/// 测试代理组延迟（兼容旧接口名）。
///
/// 说明：早期实现使用 Clash API 的 `/group/{group}/delay`，该接口在部分内核/配置下可能返回不完整，
/// 导致前端出现“有些节点没有数据也没有错误”的体验。
/// 这里改为：先从 `/proxies` 获取组内节点列表，再逐个调用 `/proxies/{name}/delay` 并汇总结果。
#[tauri::command]
pub async fn test_group_delay(
    app_handle: AppHandle,
    group: String,
    server: Option<String>,
    port: u16,
    options: Option<DelayTestOptions>,
) -> Result<Vec<ProxyDelayTestResult>, String> {
    let proxies = resolve_group_nodes(port, &group).await?;

    // 兼容旧参数名 server：作为 URL 覆盖来源
    let mut merged_options = options.unwrap_or(DelayTestOptions {
        timeout_ms: None,
        url: None,
        concurrency: None,
        samples: None,
    });
    if merged_options.url.is_none() {
        merged_options.url = server;
    }

    test_nodes_delay(app_handle, proxies, Some(merged_options), port).await
}

// 获取规则信息
#[tauri::command]
pub async fn get_rules(port: u16) -> Result<Value, String> {
    let url = format!("http://127.0.0.1:{}/rules", port);

    match http_client::get_json::<Value>(&url).await {
        Ok(data) => Ok(data),
        Err(e) => {
            error!("Failed to fetch rules: {}", e);
            Err(format!("Failed to fetch rules: {}", e))
        }
    }
}

// -----------------------------
// 延迟测试（测速）
// -----------------------------

/// sing-box 开启 `experimental.clash_api` 后，会提供 Clash RESTful API 兼容接口。
/// 其中 `/proxies/{name}/delay?timeout=xxxx&url=...` 用于对指定节点发起 URLTest 并返回延迟（单位 ms）。
///
/// 这里做了两层改进：
/// 1) 默认使用数据库中的 `singbox_urltest_url`（Setting 页面可配置），避免使用不带 204 的 URL 导致误差/超时。
/// 2) 统一实现“多节点测速”，供单测/组测/批测复用，并返回每个节点的失败原因，避免 UI 出现“无数据也无错误”。
const DEFAULT_DELAY_TEST_URL: &str = "https://connectivitycheck.gstatic.com/generate_204";
const DEFAULT_DELAY_TEST_TIMEOUT_MS: u64 = 8000;
const DEFAULT_DELAY_TEST_CONCURRENCY: usize = 6;
const DEFAULT_DELAY_TEST_SAMPLES: u8 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelayTestOptions {
    pub timeout_ms: Option<u64>,
    pub url: Option<String>,
    pub concurrency: Option<usize>,
    /// 每个节点的采样次数（取中位数以降低抖动）
    pub samples: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyDelayTestResult {
    pub proxy: String,
    /// 延迟（ms）。当 `ok=false` 时返回 0。
    pub delay: u64,
    pub ok: bool,
    /// 失败原因（用于前端展示）。
    pub error: Option<String>,
    /// 实际成功采样次数（可能小于 options.samples）
    pub success_samples: u8,
}

pub(crate) fn normalize_test_url(candidate: &str) -> String {
    // 允许用户输入 http(s) URL；其它情况直接回退默认值，避免构造出无效的 query 导致测速失真。
    if let Ok(parsed) = Url::parse(candidate) {
        if parsed.scheme() == "http" || parsed.scheme() == "https" {
            return candidate.to_string();
        }
    }
    DEFAULT_DELAY_TEST_URL.to_string()
}

async fn resolve_delay_test_url<R: Runtime>(
    app_handle: &AppHandle<R>,
    override_url: Option<String>,
) -> String {
    if let Some(url) = override_url {
        return normalize_test_url(&url);
    }

    match db_get_app_config(app_handle.clone()).await {
        Ok(cfg) => normalize_test_url(&cfg.singbox_urltest_url),
        Err(_) => DEFAULT_DELAY_TEST_URL.to_string(),
    }
}

pub(crate) fn build_clash_delay_url(
    port: u16,
    proxy: &str,
    timeout_ms: u64,
    test_url: &str,
) -> Result<Url, String> {
    let mut url = Url::parse(&format!("http://127.0.0.1:{}/", port))
        .map_err(|e| format!("构造 Clash API 地址失败: {}", e))?;

    // 使用 path_segments 自动处理需要转义的字符，避免节点名包含空格/emoji 导致请求失败。
    url.path_segments_mut()
        .map_err(|_| "无法修改 URL path_segments".to_string())?
        .extend(["proxies", proxy, "delay"]);

    url.query_pairs_mut()
        .append_pair("timeout", &timeout_ms.to_string())
        .append_pair("url", test_url);

    Ok(url)
}

async fn fetch_single_delay(
    port: u16,
    proxy: &str,
    timeout_ms: u64,
    test_url: &str,
) -> Result<u64, String> {
    let url = build_clash_delay_url(port, proxy, timeout_ms, test_url)?;

    // 使用专用短超时客户端，外层请求超时要略大于 Clash API 的 timeout，避免“核心还在测，HTTP 已被我们断开”。
    let request_timeout = Duration::from_millis(timeout_ms).saturating_add(Duration::from_secs(3));
    let client = http_client::get_proxy_client();

    let response = client
        .get(url)
        .timeout(request_timeout)
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }

    let data = response
        .json::<Value>()
        .await
        .map_err(|e| format!("解析响应失败: {}", e))?;

    let delay = data.get("delay").and_then(|d| d.as_u64()).unwrap_or(0);
    if delay == 0 {
        return Err("delay=0".to_string());
    }
    Ok(delay)
}

pub(crate) fn median_u64(mut values: Vec<u64>) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(values[values.len() / 2])
}

pub(crate) async fn measure_proxy_delay(
    port: u16,
    proxy: String,
    timeout_ms: u64,
    test_url: &str,
    samples: u8,
) -> ProxyDelayTestResult {
    let mut ok_values: Vec<u64> = Vec::new();
    let mut last_error: Option<String> = None;

    let samples = samples.max(1);
    for _ in 0..samples {
        match fetch_single_delay(port, &proxy, timeout_ms, test_url).await {
            Ok(delay) => ok_values.push(delay),
            Err(err) => last_error = Some(err),
        }
        // 轻微间隔，避免极端情况下对同一节点瞬时并发过高。
        tokio::time::sleep(Duration::from_millis(80)).await;
    }

    if let Some(delay) = median_u64(ok_values.clone()) {
        ProxyDelayTestResult {
            proxy,
            delay,
            ok: true,
            error: None,
            success_samples: ok_values.len() as u8,
        }
    } else {
        ProxyDelayTestResult {
            proxy,
            delay: 0,
            ok: false,
            error: last_error.or_else(|| Some("无可用结果".to_string())),
            success_samples: 0,
        }
    }
}

/// 测试多个节点延迟（批量/组测速统一入口）。
#[tauri::command]
pub async fn test_nodes_delay<R: Runtime>(
    app_handle: AppHandle<R>,
    proxies: Vec<String>,
    options: Option<DelayTestOptions>,
    port: u16,
) -> Result<Vec<ProxyDelayTestResult>, String> {
    let options = options.unwrap_or(DelayTestOptions {
        timeout_ms: None,
        url: None,
        concurrency: None,
        samples: None,
    });

    let timeout_ms = options.timeout_ms.unwrap_or(DEFAULT_DELAY_TEST_TIMEOUT_MS);
    let samples = options.samples.unwrap_or(DEFAULT_DELAY_TEST_SAMPLES);
    let concurrency = options
        .concurrency
        .unwrap_or(DEFAULT_DELAY_TEST_CONCURRENCY)
        .max(1);
    let test_url = resolve_delay_test_url(&app_handle, options.url).await;

    // 去重，避免重复节点浪费测试资源；保留原顺序。
    let mut seen = std::collections::HashSet::new();
    let proxies: Vec<String> = proxies
        .into_iter()
        .filter(|p| seen.insert(p.clone()))
        .collect();

    let results = stream::iter(proxies.into_iter().map(|proxy| {
        let test_url = test_url.clone();
        async move { measure_proxy_delay(port, proxy, timeout_ms, &test_url, samples).await }
    }))
    .buffer_unordered(concurrency)
    .collect::<Vec<_>>()
    .await;

    Ok(results)
}

// 测试单个节点延迟（兼容旧接口名）
#[tauri::command]
pub async fn test_node_delay<R: Runtime>(
    app_handle: AppHandle<R>,
    proxy: String,
    server: Option<String>,
    port: u16,
) -> Result<ProxyDelayTestResult, String> {
    let test_url = resolve_delay_test_url(&app_handle, server).await;
    Ok(measure_proxy_delay(
        port,
        proxy,
        DEFAULT_DELAY_TEST_TIMEOUT_MS,
        &test_url,
        DEFAULT_DELAY_TEST_SAMPLES.max(3),
    )
    .await)
}

// ===================== 用户自定义规则（issue #62）=====================
//
// 生命周期说明（与内核默认规则截然不同）：
// - 自定义规则持久化在 generic_config 表（key = STORAGE_KEY），重启保留；
// - 写入“活动 sing-box 配置文件”的 route.rules，由内核下次启动读取；
// - 命令只做 CRUD + 注入到磁盘配置，不实时热更内核（与全局设置一致：改完重启内核生效）。
//
// 注入策略：读取 AppConfig.active_config_path 指向的文件，调用 inject_custom_rules，
// 写回磁盘。若该文件是“用户原始订阅配置”（use_original_config），则跳过注入避免破坏。

use crate::app::singbox::common::normalize_default_outbound;
use crate::app::singbox::config_generator::{inject_custom_rules, strip_custom_rules};
use crate::app::storage::custom_rule::{CustomRule, CustomRuleAction, CustomRuleMatchType, STORAGE_KEY};
use crate::app::storage::enhanced_storage_service::get_enhanced_storage;
use chrono::Utc;

/// 读取所有自定义规则（按创建时间升序）。
#[tauri::command]
pub async fn list_custom_rules<R: Runtime>(
    app_handle: AppHandle<R>,
) -> Result<Vec<CustomRule>, String> {
    let storage = get_enhanced_storage(&app_handle)
        .await
        .map_err(|e| format!("初始化存储失败: {}", e))?;
    let rules: Option<Vec<CustomRule>> = storage
        .load_generic_config(STORAGE_KEY)
        .await
        .map_err(|e| format!("读取自定义规则失败: {}", e))?;
    let mut rules = rules.unwrap_or_default();
    rules.sort_by_key(|r| r.created_at);
    Ok(rules)
}

/// 新增一条自定义规则。payload/action/match_type 由前端传入。
#[tauri::command]
pub async fn add_custom_rule<R: Runtime>(
    app_handle: AppHandle<R>,
    match_type: CustomRuleMatchType,
    payload: String,
    action: CustomRuleAction,
    note: Option<String>,
) -> Result<CustomRule, String> {
    if payload.trim().is_empty() {
        return Err("匹配内容不能为空".to_string());
    }
    let storage = get_enhanced_storage(&app_handle)
        .await
        .map_err(|e| format!("初始化存储失败: {}", e))?;
    let mut rules: Vec<CustomRule> = storage
        .load_generic_config(STORAGE_KEY)
        .await
        .map_err(|e| format!("读取自定义规则失败: {}", e))?
        .unwrap_or_default();

    let now = Utc::now();
    let rule = CustomRule {
        id: uuid_v4(),
        enabled: true,
        match_type,
        payload,
        action,
        outbound: None,
        note,
        created_at: now,
        updated_at: now,
    };
    rules.push(rule.clone());
    storage
        .save_generic_config(STORAGE_KEY, &rules)
        .await
        .map_err(|e| format!("保存自定义规则失败: {}", e))?;
    inject_into_active_config(&app_handle).await;
    Ok(rule)
}

/// 更新一条规则（按 id 定位）。
#[tauri::command]
pub async fn update_custom_rule<R: Runtime>(
    app_handle: AppHandle<R>,
    id: String,
    match_type: CustomRuleMatchType,
    payload: String,
    action: CustomRuleAction,
    note: Option<String>,
) -> Result<(), String> {
    if payload.trim().is_empty() {
        return Err("匹配内容不能为空".to_string());
    }
    let storage = get_enhanced_storage(&app_handle)
        .await
        .map_err(|e| format!("初始化存储失败: {}", e))?;
    let mut rules: Vec<CustomRule> = storage
        .load_generic_config(STORAGE_KEY)
        .await
        .map_err(|e| format!("读取自定义规则失败: {}", e))?
        .unwrap_or_default();

    let target = rules
        .iter_mut()
        .find(|r| r.id == id)
        .ok_or_else(|| "未找到对应的规则".to_string())?;
    target.match_type = match_type;
    target.payload = payload;
    target.action = action;
    target.note = note;
    target.updated_at = Utc::now();

    storage
        .save_generic_config(STORAGE_KEY, &rules)
        .await
        .map_err(|e| format!("保存自定义规则失败: {}", e))?;
    inject_into_active_config(&app_handle).await;
    Ok(())
}

/// 删除一条规则（按 id）。
#[tauri::command]
pub async fn delete_custom_rule<R: Runtime>(
    app_handle: AppHandle<R>,
    id: String,
) -> Result<(), String> {
    let storage = get_enhanced_storage(&app_handle)
        .await
        .map_err(|e| format!("初始化存储失败: {}", e))?;
    let mut rules: Vec<CustomRule> = storage
        .load_generic_config(STORAGE_KEY)
        .await
        .map_err(|e| format!("读取自定义规则失败: {}", e))?
        .unwrap_or_default();
    let before = rules.len();
    rules.retain(|r| r.id != id);
    if rules.len() == before {
        return Err("未找到对应的规则".to_string());
    }
    storage
        .save_generic_config(STORAGE_KEY, &rules)
        .await
        .map_err(|e| format!("保存自定义规则失败: {}", e))?;
    inject_into_active_config(&app_handle).await;
    Ok(())
}

/// 切换规则启用/禁用（按 id）。
#[tauri::command]
pub async fn toggle_custom_rule<R: Runtime>(
    app_handle: AppHandle<R>,
    id: String,
) -> Result<(), String> {
    let storage = get_enhanced_storage(&app_handle)
        .await
        .map_err(|e| format!("初始化存储失败: {}", e))?;
    let mut rules: Vec<CustomRule> = storage
        .load_generic_config(STORAGE_KEY)
        .await
        .map_err(|e| format!("读取自定义规则失败: {}", e))?
        .unwrap_or_default();
    let target = rules
        .iter_mut()
        .find(|r| r.id == id)
        .ok_or_else(|| "未找到对应的规则".to_string())?;
    target.enabled = !target.enabled;
    target.updated_at = Utc::now();
    storage
        .save_generic_config(STORAGE_KEY, &rules)
        .await
        .map_err(|e| format!("保存自定义规则失败: {}", e))?;
    inject_into_active_config(&app_handle).await;
    Ok(())
}

/// 把当前所有启用规则注入活动配置文件（失败仅记录，不阻断 CRUD）。
///
/// 实现要点：
/// - 仅对“本程序生成的订阅配置”注入；用户原始订阅（use_original_config）跳过，避免破坏其结构。
/// - 先 [`strip_custom_rules`] 再 [`inject_custom_rules`]，按 per-rule marker 幂等重写。
/// - 订阅重生 / 切换活动配置后应调用 [`inject_custom_rules_into_config_file`]，否则磁盘配置会丢规则。
async fn inject_into_active_config<R: Runtime>(app_handle: &AppHandle<R>) {
    if let Err(e) = inject_into_active_config_inner(app_handle).await {
        warn!("自定义规则注入活动配置失败（不影响持久化）: {}", e);
    }
}

async fn inject_into_active_config_inner<R: Runtime>(
    app_handle: &AppHandle<R>,
) -> Result<(), String> {
    let storage = get_enhanced_storage(app_handle)
        .await
        .map_err(|e| format!("初始化存储失败: {}", e))?;
    let app_config = crate::app::storage::enhanced_storage_service::db_get_app_config(
        app_handle.clone(),
    )
    .await
    .map_err(|e| format!("读取应用配置失败: {}", e))?;

    // 用户原始订阅配置：不注入，避免破坏其结构。
    if is_config_path_use_original(&storage, app_config.active_config_path.as_deref()).await {
        info!("当前活动订阅为原始配置，跳过自定义规则注入");
        return Ok(());
    }

    let config_path = match &app_config.active_config_path {
        Some(p) => std::path::PathBuf::from(p),
        None => return Ok(()),
    };
    inject_custom_rules_into_config_file(app_handle, &config_path).await
}

/// 将持久化的自定义规则幂等写入指定配置文件。
///
/// 用于：CRUD 后写活动配置、订阅 materialize 后写目标配置、切换活动配置后补注。
/// 调用方负责判断是否应跳过（例如 `use_original_config`）。
pub async fn inject_custom_rules_into_config_file<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    config_path: &Path,
) -> Result<(), String> {
    let storage = get_enhanced_storage(app_handle)
        .await
        .map_err(|e| format!("初始化存储失败: {}", e))?;
    let app_config = crate::app::storage::enhanced_storage_service::db_get_app_config(
        app_handle.clone(),
    )
    .await
    .map_err(|e| format!("读取应用配置失败: {}", e))?;
    inject_custom_rules_into_config_file_with_storage(storage.as_ref(), &app_config, config_path)
        .await
}

/// Hermetic 注入入口：不依赖 AppHandle，便于单测/E2E。
pub async fn inject_custom_rules_into_config_file_with_storage(
    storage: &crate::app::storage::enhanced_storage_service::EnhancedStorageService,
    app_config: &crate::app::storage::state_model::AppConfig,
    config_path: &Path,
) -> Result<(), String> {
    if !config_path.exists() {
        return Ok(());
    }

    let rules: Vec<CustomRule> = storage
        .load_generic_config(STORAGE_KEY)
        .await
        .map_err(|e| format!("读取自定义规则失败: {}", e))?
        .unwrap_or_default();

    let content = std::fs::read_to_string(config_path)
        .map_err(|e| format!("读取配置文件失败: {}", e))?;
    let mut config: Value =
        serde_json::from_str(&content).map_err(|e| format!("解析配置失败: {}", e))?;

    // 先剔除上一次注入的自定义规则（per-rule marker），保证幂等。
    strip_custom_rules(&mut config);

    let default_outbound = normalize_default_outbound(app_config);
    let injected = if rules.is_empty() {
        0
    } else {
        inject_custom_rules(&mut config, &rules, default_outbound)
    };

    let updated =
        serde_json::to_string_pretty(&config).map_err(|e| format!("序列化配置失败: {}", e))?;
    std::fs::write(config_path, updated).map_err(|e| format!("写入配置失败: {}", e))?;
    info!(
        "已把 {} 条自定义规则注入配置: {:?}",
        injected, config_path
    );
    Ok(())
}

/// 判断给定配置路径对应的订阅是否为“原始配置”（原始配置不注入）。
async fn is_config_path_use_original(
    storage: &std::sync::Arc<crate::app::storage::enhanced_storage_service::EnhancedStorageService>,
    path: Option<&str>,
) -> bool {
    let path = match path {
        Some(p) => p,
        None => return false,
    };
    let subscriptions = match storage.get_subscriptions().await {
        Ok(s) => s,
        Err(_) => return false,
    };
    subscriptions
        .iter()
        .any(|s| s.config_path.as_deref() == Some(path) && s.use_original_config)
}

/// 生成简单 uuid（不引入 uuid crate 依赖：用时间戳 + 进程 id 组合）。
pub(crate) fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:016x}-{:08x}", nanos, std::process::id())
}

#[cfg(test)]
#[path = "proxy_service.tests.rs"]
mod tests;
