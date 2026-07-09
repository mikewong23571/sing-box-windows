use crate::app::constants::paths;
use crate::app::storage::enhanced_storage_service::db_get_app_config;
use crate::utils::http_client;
use serde_json::json;
use std::error::Error;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Runtime};
use tracing::{error, info, warn};

pub async fn toggle_proxy_mode_impl<R: Runtime>(
    app_handle: AppHandle<R>,
    mode: String,
) -> Result<String, String> {
    let mode = normalize_proxy_mode(&mode)
        .ok_or_else(|| format!("无效的代理模式: {}", mode))?
        .to_string();

    info!("正在切换代理模式为: {}", mode);

    let app_config = db_get_app_config(app_handle)
        .await
        .map_err(|e| format!("获取应用配置失败: {}", e))?;

    let path = resolve_proxy_mode_config_path(app_config.active_config_path.as_deref());

    if !path.exists() {
        return Err("配置文件不存在，请先添加订阅".to_string());
    }

    modify_default_mode(&path, mode.clone(), None).map_err(|e| {
        error!("切换代理模式失败: {}", e);
        format!("切换代理模式失败: {}", e)
    })?;

    let config_paths = collect_proxy_mode_config_paths(
        app_config.active_config_path.as_deref().map(Path::new),
        &paths::get_config_dir().join("config.json"),
    );

    match sync_running_proxy_mode(&config_paths, Some(app_config.api_port), &mode).await {
        Ok(()) => {
            info!("代理模式已切换并同步到运行时: {}", mode);
            Ok(format!("代理模式已切换为: {}", mode))
        }
        Err(e) => {
            warn!("运行时代理模式同步失败，已保留配置文件更新: {}", e);
            Ok(format!(
                "代理模式已保存为: {}，当前运行中的内核暂未同步，重启后生效",
                mode
            ))
        }
    }
}

pub async fn get_current_proxy_mode_impl<R: Runtime>(
    app_handle: AppHandle<R>,
) -> Result<String, String> {
    info!("正在获取当前代理模式");

    let app_config = db_get_app_config(app_handle)
        .await
        .map_err(|e| format!("获取应用配置失败: {}", e))?;
    let default_config_path = paths::get_config_dir().join("config.json");
    let config_paths = collect_proxy_mode_config_paths(
        app_config.active_config_path.as_deref().map(Path::new),
        &default_config_path,
    );

    let mode =
        read_current_proxy_mode_from_configs(&config_paths, Some(app_config.api_port)).await?;
    info!("当前代理模式为: {}", mode);
    Ok(mode)
}

pub fn modify_default_mode(
    config_path: &Path,
    mode: String,
    api_port: Option<u16>,
) -> Result<(), Box<dyn Error>> {
    let mut file = File::open(config_path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;

    let mut config: serde_json::Value = serde_json::from_str(&content)?;

    if let Some(config_obj) = config.as_object_mut() {
        if let Some(experimental) = config_obj.get_mut("experimental") {
            if let Some(clash_api) = experimental.get_mut("clash_api") {
                if let Some(clash_api_obj) = clash_api.as_object_mut() {
                    clash_api_obj.insert("default_mode".to_string(), json!(mode));

                    if let Some(port) = api_port {
                        clash_api_obj.insert(
                            "external_controller".to_string(),
                            json!(format!("127.0.0.1:{}", port)),
                        );
                    }

                    clash_api_obj.insert("external_ui".to_string(), json!("metacubexd"));
                } else {
                    return Err("clash_api 不是对象".into());
                }
            } else {
                let mut clash_api = serde_json::Map::new();
                clash_api.insert("default_mode".to_string(), json!(mode));
                clash_api.insert("external_ui".to_string(), json!("metacubexd"));

                if let Some(port) = api_port {
                    clash_api.insert(
                        "external_controller".to_string(),
                        json!(format!("127.0.0.1:{}", port)),
                    );
                }

                if let Some(exp_obj) = experimental.as_object_mut() {
                    exp_obj.insert("clash_api".to_string(), json!(clash_api));
                } else {
                    return Err("experimental 不是对象".into());
                }
            }
        } else {
            let mut experimental = serde_json::Map::new();

            let mut clash_api = serde_json::Map::new();
            clash_api.insert("default_mode".to_string(), json!(mode));
            clash_api.insert("external_ui".to_string(), json!("metacubexd"));

            if let Some(port) = api_port {
                clash_api.insert(
                    "external_controller".to_string(),
                    json!(format!("127.0.0.1:{}", port)),
                );
            }

            experimental.insert("clash_api".to_string(), json!(clash_api));

            config_obj.insert("experimental".to_string(), json!(experimental));
        }

        if let Some(experimental) = config_obj.get_mut("experimental") {
            if let Some(experimental_obj) = experimental.as_object_mut() {
                experimental_obj.insert(
                    "cache_file".to_string(),
                    json!({
                        "enabled": true
                    }),
                );
            }
        }

        let updated_content = serde_json::to_string_pretty(&config)?;
        let mut file = File::create(config_path)?;
        file.write_all(updated_content.as_bytes())?;

        info!("已成功更新代理模式为: {}", mode);
    } else {
        return Err("配置文件格式错误：根对象不是JSON对象".into());
    }

    Ok(())
}

pub(crate) fn read_proxy_mode_from_config(config_path: &Path) -> Result<String, Box<dyn Error>> {
    let mut file = File::open(config_path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;

    let json: serde_json::Value = serde_json::from_str(&content)?;

    if let Some(experimental) = json.get("experimental") {
        if let Some(clash_api) = experimental.get("clash_api") {
            if let Some(default_mode) = clash_api.get("default_mode") {
                if let Some(mode) = default_mode.as_str() {
                    return Ok(normalize_proxy_mode(mode).unwrap_or("rule").to_string());
                }
            }
        }
    }

    Ok("rule".to_string())
}

pub(crate) fn normalize_proxy_mode(mode: &str) -> Option<&'static str> {
    match mode.trim().to_ascii_lowercase().as_str() {
        "global" => Some("global"),
        "rule" => Some("rule"),
        _ => None,
    }
}

pub(crate) fn clash_api_mode_alias(mode: &str) -> Option<&'static str> {
    match normalize_proxy_mode(mode)? {
        "global" => Some("Global"),
        "rule" => Some("Rule"),
        _ => None,
    }
}

pub(crate) fn resolve_proxy_mode_config_path(active_config_path: Option<&str>) -> PathBuf {
    active_config_path
        .map(PathBuf::from)
        .unwrap_or_else(|| paths::get_config_dir().join("config.json"))
}

fn collect_proxy_mode_config_paths(
    active_config_path: Option<&Path>,
    default_config_path: &Path,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(path) = active_config_path {
        paths.push(path.to_path_buf());
    }

    if !paths.iter().any(|path| path == default_config_path) {
        paths.push(default_config_path.to_path_buf());
    }

    paths
}

pub(crate) fn read_api_port_from_config(config_path: &Path) -> Result<Option<u16>, Box<dyn Error>> {
    if !config_path.exists() {
        return Ok(None);
    }

    let mut file = File::open(config_path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;

    let json: serde_json::Value = serde_json::from_str(&content)?;
    let controller = json
        .get("experimental")
        .and_then(|value| value.get("clash_api"))
        .and_then(|value| value.get("external_controller"))
        .and_then(|value| value.as_str());

    Ok(controller.and_then(|value| value.rsplit(':').next()?.parse::<u16>().ok()))
}

fn collect_proxy_mode_api_ports(
    config_paths: &[PathBuf],
    fallback_api_port: Option<u16>,
) -> Vec<u16> {
    let mut api_ports = Vec::new();

    for config_path in config_paths {
        if !config_path.exists() {
            continue;
        }

        match read_api_port_from_config(config_path) {
            Ok(Some(port)) if !api_ports.contains(&port) => {
                api_ports.push(port);
                break;
            }
            Ok(_) => {}
            Err(e) => warn!(
                "读取 Clash API 端口失败，将继续尝试其他来源: {:?}, {}",
                config_path, e
            ),
        }
    }

    if let Some(port) = fallback_api_port {
        if !api_ports.contains(&port) {
            api_ports.push(port);
        }
    }

    api_ports
}

async fn query_clash_api_mode(api_port: u16) -> Result<String, String> {
    let response = http_client::get_client()
        .get(format!("http://127.0.0.1:{api_port}/configs"))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("请求 Clash API 失败: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Clash API 返回失败状态: {}", e))?;

    let payload: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("解析 Clash API 响应失败: {}", e))?;

    payload
        .get("mode")
        .and_then(|value| value.as_str())
        .and_then(normalize_proxy_mode)
        .map(str::to_string)
        .ok_or_else(|| "Clash API 响应中缺少 mode 字段".to_string())
}

async fn patch_clash_api_mode(api_port: u16, mode: &str) -> Result<(), String> {
    let normalized =
        normalize_proxy_mode(mode).ok_or_else(|| format!("无效的代理模式: {}", mode))?;
    let alias = clash_api_mode_alias(normalized).unwrap_or(normalized);

    match patch_clash_api_mode_once(api_port, normalized).await {
        Ok(()) => Ok(()),
        Err(first_error) if alias != normalized => {
            warn!(
                "使用小写模式同步失败(port={})，尝试兼容格式 {}: {}",
                api_port, alias, first_error
            );
            patch_clash_api_mode_once(api_port, alias)
                .await
                .map_err(|second_error| format!("{}; {}", first_error, second_error))
        }
        Err(first_error) => Err(first_error),
    }
}

async fn patch_clash_api_mode_once(api_port: u16, mode: &str) -> Result<(), String> {
    http_client::get_client()
        .patch(format!("http://127.0.0.1:{api_port}/configs"))
        .json(&json!({ "mode": mode }))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("请求 Clash API 失败: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Clash API 返回失败状态: {}", e))?;

    Ok(())
}

async fn read_current_proxy_mode_from_configs(
    config_paths: &[PathBuf],
    fallback_api_port: Option<u16>,
) -> Result<String, String> {
    for api_port in collect_proxy_mode_api_ports(config_paths, fallback_api_port) {
        match query_clash_api_mode(api_port).await {
            Ok(mode) => return Ok(mode),
            Err(e) => warn!(
                "从运行中 Clash API 读取代理模式失败(port={}): {}",
                api_port, e
            ),
        }
    }

    for config_path in config_paths {
        if !config_path.exists() {
            continue;
        }

        match read_proxy_mode_from_config(config_path) {
            Ok(mode) => return Ok(mode),
            Err(e) => warn!("从配置文件读取代理模式失败({:?}): {}", config_path, e),
        }
    }

    Ok("rule".to_string())
}

async fn sync_running_proxy_mode(
    config_paths: &[PathBuf],
    fallback_api_port: Option<u16>,
    mode: &str,
) -> Result<(), String> {
    let mut last_error = None;

    for api_port in collect_proxy_mode_api_ports(config_paths, fallback_api_port) {
        match patch_clash_api_mode(api_port, mode).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                warn!("同步运行态代理模式失败(port={}): {}", api_port, e);
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| "未找到可用的 Clash API 端口".to_string()))
}

#[cfg(test)]
#[path = "mode.tests.rs"]
mod tests;
