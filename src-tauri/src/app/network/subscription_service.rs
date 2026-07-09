pub mod auto_update;
pub mod helpers;
mod materializer;
mod mode;
mod parser;

use crate::app::constants::{messages, paths};
use crate::app::runtime::change::{RuntimeApplyOptions, RuntimeChange};
use crate::app::runtime::orchestrator::apply_runtime_change;
use crate::app::storage::enhanced_storage_service::{
    db_get_app_config, db_get_subscriptions, db_save_app_config_internal, db_save_subscriptions,
};
use crate::app::storage::state_model::AppConfig;
use crate::utils::http_client;
use helpers::resolve_target_config_path;
#[cfg(test)]
use materializer::try_decode_base64_to_text;
use materializer::{write_downloaded_subscription_config, write_manual_subscription_config};
#[cfg(test)]
use parser::extract_nodes_from_subscription;
use reqwest::header::{HeaderMap, USER_AGENT};
use serde::Serialize;
use std::error::Error;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionPersistResult {
    pub config_path: String,
    pub subscription_upload: Option<u64>,
    pub subscription_download: Option<u64>,
    pub subscription_total: Option<u64>,
    pub subscription_expire: Option<u64>,
}

#[derive(Debug, Clone)]
struct SubscriptionUserInfo {
    upload: Option<u64>,
    download: Option<u64>,
    total: Option<u64>,
    expire: Option<u64>,
}

#[derive(Debug, Clone)]
struct SubscriptionFetchResult {
    body: String,
    userinfo: Option<SubscriptionUserInfo>,
}

const SUBSCRIPTION_USERINFO_COMPAT_UAS: [&str; 2] = ["clash.meta", "clash-verge/1.7.7"];

fn normalized_active_config_path(path: &Option<String>) -> Option<&str> {
    path.as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn active_config_change_requires_restart(previous: &Option<String>, next: &Option<String>) -> bool {
    normalized_active_config_path(previous) != normalized_active_config_path(next)
}

fn parse_subscription_userinfo(raw: &str) -> Option<SubscriptionUserInfo> {
    let mut info = SubscriptionUserInfo {
        upload: None,
        download: None,
        total: None,
        expire: None,
    };

    let mut has_value = false;
    for segment in raw.split(';') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        let (key, value) = match segment.split_once('=') {
            Some(pair) => pair,
            None => continue,
        };

        let value = value.trim().parse::<u64>().ok();
        match key.trim().to_ascii_lowercase().as_str() {
            "upload" => {
                info.upload = value;
                has_value = true;
            }
            "download" => {
                info.download = value;
                has_value = true;
            }
            "total" => {
                info.total = value;
                has_value = true;
            }
            "expire" => {
                info.expire = value;
                has_value = true;
            }
            _ => {}
        }
    }

    if has_value {
        Some(info)
    } else {
        None
    }
}

fn extract_subscription_userinfo(headers: &HeaderMap) -> Option<SubscriptionUserInfo> {
    let header = headers
        .get("subscription-userinfo")
        .or_else(|| headers.get("Subscription-Userinfo"))?;
    let raw = header.to_str().ok()?;
    parse_subscription_userinfo(raw)
}

fn should_retry_subscription_userinfo(result: &SubscriptionFetchResult) -> bool {
    result.userinfo.is_none() && !result.body.trim().is_empty()
}

fn merge_subscription_fetch_result(
    primary: SubscriptionFetchResult,
    fallback_userinfo: Option<SubscriptionUserInfo>,
) -> SubscriptionFetchResult {
    SubscriptionFetchResult {
        body: primary.body,
        userinfo: primary.userinfo.or(fallback_userinfo),
    }
}

async fn fetch_subscription_content_with_user_agent(
    url: &str,
    user_agent: Option<&str>,
) -> Result<SubscriptionFetchResult, Box<dyn Error>> {
    let mut request = http_client::get_client().get(url);
    if let Some(user_agent) = user_agent {
        request = request.header(USER_AGENT, user_agent);
    }

    let response = request.send().await?;
    response.error_for_status_ref()?;
    let headers = response.headers().clone();
    let body = response.text().await?;
    let userinfo = extract_subscription_userinfo(&headers);
    Ok(SubscriptionFetchResult { body, userinfo })
}

async fn fetch_subscription_content(
    url: &str,
) -> Result<(String, Option<SubscriptionUserInfo>), Box<dyn Error>> {
    let primary = fetch_subscription_content_with_user_agent(url, None).await?;

    if !should_retry_subscription_userinfo(&primary) {
        return Ok((primary.body, primary.userinfo));
    }

    info!(
        "订阅响应缺少 subscription-userinfo，尝试使用兼容 User-Agent 重试: {}",
        url
    );

    let mut fallback_userinfo = None;
    for compat_user_agent in SUBSCRIPTION_USERINFO_COMPAT_UAS {
        match fetch_subscription_content_with_user_agent(url, Some(compat_user_agent)).await {
            Ok(result) => {
                if let Some(userinfo) = result.userinfo {
                    info!(
                        "使用兼容 User-Agent 获取到 subscription-userinfo: {}",
                        compat_user_agent
                    );
                    fallback_userinfo = Some(userinfo);
                    break;
                }

                info!(
                    "兼容 User-Agent 仍未返回 subscription-userinfo: {}",
                    compat_user_agent
                );
            }
            Err(err) => {
                warn!(
                    "兼容 User-Agent 重试订阅信息失败 ({}): {}",
                    compat_user_agent, err
                );
            }
        }
    }

    let merged = merge_subscription_fetch_result(primary, fallback_userinfo);
    Ok((merged.body, merged.userinfo))
}

async fn update_subscription_userinfo(
    app_handle: &AppHandle,
    target_path: &Path,
    url: &str,
    userinfo: Option<SubscriptionUserInfo>,
) -> Result<(), String> {
    let mut subscriptions = db_get_subscriptions(app_handle.clone())
        .await
        .map_err(|e| format!("读取订阅配置失败: {}", e))?;

    let trimmed_url = url.trim();
    let target_path = target_path.to_string_lossy();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("获取时间失败: {}", e))?
        .as_millis() as u64;

    let mut updated = false;
    for sub in subscriptions.iter_mut() {
        let path_match = sub
            .config_path
            .as_deref()
            .map(|path| path == target_path.as_ref())
            .unwrap_or(false);
        let url_match = !trimmed_url.is_empty() && sub.url.trim() == trimmed_url;

        if path_match || url_match {
            sub.last_update = Some(now_ms);
            if let Some(info) = &userinfo {
                sub.subscription_upload = info.upload;
                sub.subscription_download = info.download;
                sub.subscription_total = info.total;
                sub.subscription_expire = info.expire;
            } else {
                sub.subscription_upload = None;
                sub.subscription_download = None;
                sub.subscription_total = None;
                sub.subscription_expire = None;
            }
            updated = true;
        }
    }

    if updated {
        db_save_subscriptions(subscriptions, app_handle.clone())
            .await
            .map_err(|e| format!("保存订阅配置失败: {}", e))?;
    }

    Ok(())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri 接口需与前端参数保持一致
pub async fn download_subscription(
    url: String,
    use_original_config: bool,
    file_name: Option<String>,
    config_path: Option<String>,
    apply_runtime: Option<bool>,
    window: tauri::Window,
    proxy_port: Option<u16>,
    api_port: Option<u16>,
) -> Result<SubscriptionPersistResult, String> {
    let app_handle = window.app_handle();
    let apply_runtime = apply_runtime.unwrap_or(true);

    let mut app_config = db_get_app_config(app_handle.clone())
        .await
        .map_err(|e| format!("读取设置失败: {}", e))?;

    if let Some(port) = proxy_port {
        app_config.proxy_port = port;
    }
    if let Some(port) = api_port {
        app_config.api_port = port;
    }

    let target_path = resolve_target_config_path(file_name, config_path)?;
    let trimmed_url = url.trim();
    info!("开始下载订阅: {}", trimmed_url);
    let (response_text, userinfo) = fetch_subscription_content(trimmed_url)
        .await
        .map_err(|e| format!("{}: {}", messages::ERR_SUBSCRIPTION_FAILED, e))?;
    info!("订阅下载成功，内容长度: {} 字节", response_text.len());
    write_downloaded_subscription_config(
        &response_text,
        use_original_config,
        &app_config,
        &target_path,
    )
    .map_err(|e| format!("{}: {}", messages::ERR_SUBSCRIPTION_FAILED, e))?;

    if apply_runtime {
        let active_result = set_active_config_path_internal(
            app_handle,
            Some(target_path.to_string_lossy().to_string()),
        )
        .await;

        if let Err(e) = active_result {
            warn!("写入激活配置指针失败: {}", e);
        }

        let options = RuntimeApplyOptions::new("subscription-download")
            .patch_active_config(true)
            .force_restart(true)
            .use_original_config_hint(Some(use_original_config));
        if let Err(e) =
            apply_runtime_change(app_handle, RuntimeChange::SubscriptionApplied, options).await
        {
            warn!("应用订阅运行态失败: {}", e);
        }
    }

    if let Err(e) =
        update_subscription_userinfo(app_handle, &target_path, trimmed_url, userinfo.clone()).await
    {
        warn!("同步订阅信息失败: {}", e);
    }

    Ok(SubscriptionPersistResult {
        config_path: target_path.to_string_lossy().to_string(),
        subscription_upload: userinfo.as_ref().and_then(|info| info.upload),
        subscription_download: userinfo.as_ref().and_then(|info| info.download),
        subscription_total: userinfo.as_ref().and_then(|info| info.total),
        subscription_expire: userinfo.as_ref().and_then(|info| info.expire),
    })
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri 接口需与前端参数保持一致
pub async fn add_manual_subscription(
    content: String,
    use_original_config: bool,
    file_name: Option<String>,
    config_path: Option<String>,
    apply_runtime: Option<bool>,
    window: tauri::Window,
    proxy_port: Option<u16>,
    api_port: Option<u16>,
) -> Result<SubscriptionPersistResult, String> {
    let app_handle = window.app_handle();
    let apply_runtime = apply_runtime.unwrap_or(true);

    let mut app_config = db_get_app_config(app_handle.clone())
        .await
        .map_err(|e| format!("读取设置失败: {}", e))?;

    if let Some(port) = proxy_port {
        app_config.proxy_port = port;
    }
    if let Some(port) = api_port {
        app_config.api_port = port;
    }

    let target_path = resolve_target_config_path(file_name, config_path)?;

    write_manual_subscription_config(&content, use_original_config, &app_config, &target_path)
        .map_err(|e| format!("{}: {}", messages::ERR_PROCESS_SUBSCRIPTION_FAILED, e))?;

    if apply_runtime {
        let active_result = set_active_config_path_internal(
            app_handle,
            Some(target_path.to_string_lossy().to_string()),
        )
        .await;

        if let Err(e) = active_result {
            warn!("写入激活配置指针失败: {}", e);
        }

        let options = RuntimeApplyOptions::new("subscription-manual")
            .patch_active_config(true)
            .force_restart(true)
            .use_original_config_hint(Some(use_original_config));
        if let Err(e) =
            apply_runtime_change(app_handle, RuntimeChange::SubscriptionApplied, options).await
        {
            warn!("应用手动订阅运行态失败: {}", e);
        }
    }

    Ok(SubscriptionPersistResult {
        config_path: target_path.to_string_lossy().to_string(),
        subscription_upload: None,
        subscription_download: None,
        subscription_total: None,
        subscription_expire: None,
    })
}

#[tauri::command]
pub async fn get_current_config(app_handle: AppHandle) -> Result<String, String> {
    let app_config = db_get_app_config(app_handle)
        .await
        .map_err(|e| format!("获取应用配置失败: {}", e))?;

    let config_path = if let Some(path_str) = app_config.active_config_path {
        std::path::PathBuf::from(path_str)
    } else {
        paths::get_config_dir().join("config.json")
    };

    if !config_path.exists() {
        return Err(messages::ERR_CONFIG_READ_FAILED.to_string());
    }

    std::fs::read_to_string(config_path)
        .map_err(|e| format!("{}: {}", messages::ERR_CONFIG_READ_FAILED, e))
}

async fn set_active_config_path_internal(
    app_handle: &AppHandle,
    config_path: Option<String>,
) -> Result<(AppConfig, bool), String> {
    let mut app_config = db_get_app_config(app_handle.clone())
        .await
        .map_err(|e| format!("获取应用配置失败: {}", e))?;

    let previous = app_config.active_config_path.clone();
    app_config.active_config_path = config_path;
    info!(
        "设置 active_config_path: {:?} -> {:?}",
        previous, app_config.active_config_path
    );

    db_save_app_config_internal(app_config.clone(), app_handle)
        .await
        .map_err(|e| format!("保存配置路径失败: {}", e))?;

    let requires_restart =
        active_config_change_requires_restart(&previous, &app_config.active_config_path);

    Ok((app_config, requires_restart))
}

#[tauri::command]
pub async fn set_active_config_path(
    app_handle: AppHandle,
    config_path: Option<String>,
    use_original_config: Option<bool>,
) -> Result<(), String> {
    let (_, requires_restart) = set_active_config_path_internal(&app_handle, config_path).await?;

    let options = RuntimeApplyOptions::new("active-config-path-updated")
        .patch_active_config(true)
        .force_restart(requires_restart)
        .use_original_config_hint(use_original_config);
    apply_runtime_change(&app_handle, RuntimeChange::ActiveConfigChanged, options).await?;

    Ok(())
}

#[tauri::command]
pub fn delete_subscription_config(config_path: String) -> Result<(), String> {
    let path = PathBuf::from(&config_path);

    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("删除配置文件失败: {}", e))?;
    }

    let backup = path.with_extension("bak");
    if backup.exists() {
        let _ = std::fs::remove_file(&backup);
    }

    Ok(())
}

#[tauri::command]
pub fn rollback_subscription_config(config_path: String) -> Result<String, String> {
    let path = PathBuf::from(&config_path);
    let backup = path.with_extension("bak");

    if !backup.exists() {
        return Err("未找到可用于回滚的备份文件".to_string());
    }

    std::fs::copy(&backup, &path).map_err(|e| format!("回滚配置失败: {}", e))?;

    Ok(config_path)
}

#[tauri::command]
pub async fn toggle_proxy_mode(app_handle: AppHandle, mode: String) -> Result<String, String> {
    mode::toggle_proxy_mode_impl(app_handle, mode).await
}

#[tauri::command]
pub async fn get_current_proxy_mode(app_handle: AppHandle) -> Result<String, String> {
    mode::get_current_proxy_mode_impl(app_handle).await
}

#[cfg(test)]
#[path = "subscription_service.tests.rs"]
mod tests;
