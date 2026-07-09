use crate::app::constants::{common::messages, paths};
use crate::app::storage::enhanced_storage_service::db_get_app_config;
use serde::Deserialize;
use serde_json;
use std::process::Command;
use tauri::{AppHandle, Runtime};
use tracing::{info, warn};

/// 默认 latest 版本 API 镜像列表（生产路径）。
pub(crate) fn default_latest_version_api_urls() -> &'static [&'static str] {
    &[
        "https://api.github.com/repos/SagerNet/sing-box/releases/latest",
        "https://v6.gh-proxy.com/https://api.github.com/repos/SagerNet/sing-box/releases/latest",
        "https://gh-proxy.com/https://api.github.com/repos/SagerNet/sing-box/releases/latest",
        "https://ghfast.top/https://api.github.com/repos/SagerNet/sing-box/releases/latest",
    ]
}

/// 默认 releases 列表 API 镜像（生产路径）。
pub(crate) fn default_releases_api_urls() -> &'static [&'static str] {
    &[
        "https://api.github.com/repos/SagerNet/sing-box/releases",
        "https://v6.gh-proxy.com/https://api.github.com/repos/SagerNet/sing-box/releases",
        "https://gh-proxy.com/https://api.github.com/repos/SagerNet/sing-box/releases",
        "https://ghfast.top/https://api.github.com/repos/SagerNet/sing-box/releases",
    ]
}

/// 从 tag 字符串剥离前缀 `v`（纯逻辑）。
pub(crate) fn strip_version_tag_prefix(tag_name: &str) -> String {
    if let Some(stripped) = tag_name.strip_prefix('v') {
        stripped.to_string()
    } else {
        tag_name.to_string()
    }
}

/// 过滤正式版 release 标签（排除 prerelease / rc / beta / alpha）。
pub(crate) fn filter_stable_release_tags(
    releases: impl IntoIterator<Item = (String, bool)>,
) -> Vec<String> {
    releases
        .into_iter()
        .filter(|(_, prerelease)| !prerelease)
        .map(|(tag, _)| strip_version_tag_prefix(&tag))
        .filter(|v| {
            let lower = v.to_lowercase();
            !lower.contains("rc") && !lower.contains("beta") && !lower.contains("alpha")
        })
        .collect()
}

/// 按镜像列表依次请求 latest 版本（可注入 URL，便于本地 mock）。
pub(crate) async fn fetch_latest_kernel_version_from_urls(
    api_urls: &[&str],
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    #[derive(Deserialize)]
    struct GitHubRelease {
        tag_name: String,
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("sing-box-windows/1.8.2")
        .build()?;

    for (index, api_url) in api_urls.iter().enumerate() {
        info!("尝试第 {} 个 API 源获取版本: {}", index + 1, api_url);

        match client.get(*api_url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    let release: GitHubRelease = response.json().await?;
                    let version = strip_version_tag_prefix(&release.tag_name);
                    info!("成功获取版本号: {} (来源: {})", version, api_url);
                    return Ok(version);
                } else {
                    warn!(
                        "API 返回错误状态: {} (来源: {})",
                        response.status(),
                        api_url
                    );
                }
            }
            Err(e) => {
                warn!("API 请求失败: {} (来源: {})", e, api_url);
            }
        }
    }

    Err("所有 API 源都获取版本失败".into())
}

/// 按镜像列表依次请求正式版 release 列表（可注入 URL）。
pub(crate) async fn fetch_kernel_releases_from_urls(
    api_urls: &[&str],
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    #[derive(Deserialize)]
    struct GitHubRelease {
        tag_name: String,
        prerelease: bool,
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("sing-box-windows/1.8.2")
        .build()?;

    for (index, api_url) in api_urls.iter().enumerate() {
        info!("尝试第 {} 个 API 源获取版本列表: {}", index + 1, api_url);

        match client.get(*api_url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    let releases: Vec<GitHubRelease> = response.json().await?;
                    let versions = filter_stable_release_tags(
                        releases
                            .into_iter()
                            .map(|r| (r.tag_name, r.prerelease)),
                    );

                    info!(
                        "成功获取版本列表（已过滤正式版），共 {} 个版本 (来源: {})",
                        versions.len(),
                        api_url
                    );
                    return Ok(versions);
                } else {
                    warn!(
                        "API 返回错误状态: {} (来源: {})",
                        response.status(),
                        api_url
                    );
                }
            }
            Err(e) => {
                warn!("API 请求失败: {} (来源: {})", e, api_url);
            }
        }
    }

    Err("所有 API 源都获取版本列表失败".into())
}

pub(super) async fn get_latest_kernel_version(
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    fetch_latest_kernel_version_from_urls(default_latest_version_api_urls()).await
}

pub(super) async fn get_kernel_releases(
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    fetch_kernel_releases_from_urls(default_releases_api_urls()).await
}

pub(crate) fn normalize_version_str(raw: &str) -> String {
    let mut cleaned = raw.trim();
    if cleaned.starts_with("sing-box") {
        cleaned = cleaned.trim_start_matches("sing-box").trim();
    }
    if cleaned.is_empty() {
        return String::new();
    }

    if let Some(token) = cleaned.split_whitespace().find(|part| {
        part.chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == 'v')
    }) {
        return token.trim_start_matches('v').to_string();
    }

    cleaned.trim_start_matches('v').to_string()
}

pub(crate) fn extract_clean_version(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(ver) = value.get("version").and_then(|v| v.as_str()) {
            return normalize_version_str(ver);
        }
    }

    if let Some(pos) = trimmed.find("version") {
        let after_version = trimmed[pos + "version".len()..]
            .trim_start_matches(|c: char| c == ':' || c.is_whitespace());

        if let Some(token) = after_version.split_whitespace().next() {
            if !token.is_empty() {
                return normalize_version_str(token);
            }
        }
    }

    if let Some(token) = trimmed.split_whitespace().find(|part| {
        part.chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == 'v')
    }) {
        return normalize_version_str(token);
    }

    normalize_version_str(trimmed.split("Environment").next().unwrap_or(trimmed))
}

/// 执行内核 `version` 并解析版本号（hermetic，无 AppHandle）。
pub(crate) async fn read_kernel_version_from_binary(
    kernel_path: &std::path::Path,
) -> Result<String, String> {
    if !kernel_path.exists() {
        return Err(messages::ERR_KERNEL_NOT_FOUND.to_string());
    }

    let mut cmd = tokio::process::Command::new(kernel_path);
    cmd.arg("version");

    #[cfg(target_os = "windows")]
    cmd.creation_flags(crate::app::constants::core::process::CREATE_NO_WINDOW);

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("{}: {}", messages::ERR_VERSION_CHECK_FAILED, e))?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{}: {}", messages::ERR_GET_VERSION_FAILED, error));
    }

    let version_info = String::from_utf8_lossy(&output.stdout);
    Ok(extract_clean_version(&version_info))
}

/// 用指定内核对配置执行 `check`（hermetic）。
pub(crate) async fn check_config_with_kernel(
    kernel_path: &std::path::Path,
    config_path: &std::path::Path,
) -> Result<(), String> {
    if !kernel_path.exists() {
        return Err(messages::ERR_KERNEL_NOT_FOUND.to_string());
    }
    if !config_path.exists() {
        return Err(format!(
            "配置文件不存在: {}",
            config_path.to_string_lossy()
        ));
    }

    let mut cmd = tokio::process::Command::new(kernel_path);
    cmd.arg("check").arg("--config").arg(config_path);

    #[cfg(target_os = "windows")]
    cmd.creation_flags(crate::app::constants::core::process::CREATE_NO_WINDOW);

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("执行配置检查命令失败: {}", e))?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(format!("配置检查失败: {}", error));
    }

    Ok(())
}

/// 解析配置校验路径：显式路径优先，否则 active_config，再回退默认 config.json。
pub(crate) fn resolve_config_path_for_validity(
    explicit_path: &str,
    active_config_path: Option<&str>,
    default_config_path: &std::path::Path,
) -> String {
    if !explicit_path.is_empty() {
        return explicit_path.to_string();
    }
    if let Some(path_str) = active_config_path {
        if !path_str.trim().is_empty() {
            return path_str.to_string();
        }
    }
    default_config_path.to_string_lossy().to_string()
}

/// 内核版本探测实现（任意 Runtime，便于 Mock）。
pub async fn check_kernel_version_impl<R: Runtime>(
    app_handle: &AppHandle<R>,
) -> Result<String, String> {
    // 1. 尝试从数据库读取缓存的版本号
    use crate::app::storage::enhanced_storage_service::db_get_app_config;
    if let Ok(config) = db_get_app_config(app_handle.clone()).await {
        if let Some(ver) = config.installed_kernel_version {
            if !ver.is_empty() {
                // optional: 验证一下文件是否存在，避免只是数据库有记录但文件没了
                let kernel_path = paths::get_kernel_path();
                if kernel_path.exists() {
                    info!("从数据库读取缓存的内核版本: {}", ver);
                    return Ok(ver);
                }
            }
        }
    }

    // 2. 如果数据库没有或文件不存在，回退到执行命令检查
    let kernel_path = paths::get_kernel_path();

    if !kernel_path.exists() {
        let _ = crate::app::core::kernel_service::embedded::ensure_embedded_kernel(app_handle).await;
    }

    let version = read_kernel_version_from_binary(&kernel_path).await?;

    // 3. 将查到的版本回写到数据库，下次就不用查了
    use crate::app::storage::enhanced_storage_service::db_save_app_config_internal;
    if let Ok(mut config) = db_get_app_config(app_handle.clone()).await {
        // 只有当如果不一致时才保存? 或者总是保存确保最新
        config.installed_kernel_version = Some(version.clone());
        let _ = db_save_app_config_internal(config, app_handle).await;
    }

    Ok(version)
}

#[tauri::command]
pub async fn check_kernel_version(app_handle: AppHandle) -> Result<String, String> {
    check_kernel_version_impl(&app_handle).await
}

/// 配置校验实现（任意 Runtime）。
pub async fn check_config_validity_impl<R: Runtime>(
    app_handle: AppHandle<R>,
    config_path: String,
) -> Result<(), String> {
    let kernel_path = paths::get_kernel_path();

    let active = if config_path.is_empty() {
        let app_config = db_get_app_config(app_handle)
            .await
            .map_err(|e| format!("获取应用配置失败: {}", e))?;
        app_config.active_config_path
    } else {
        None
    };

    let path = resolve_config_path_for_validity(
        &config_path,
        active.as_deref(),
        &paths::get_config_dir().join("config.json"),
    );

    check_config_with_kernel(&kernel_path, std::path::Path::new(&path)).await
}

#[tauri::command]
pub async fn check_config_validity(
    app_handle: AppHandle,
    config_path: String,
) -> Result<(), String> {
    check_config_validity_impl(app_handle, config_path).await
}

pub(super) fn get_system_arch() -> &'static str {
    if let Ok(force_arch) = std::env::var("SING_BOX_FORCE_ARCH") {
        info!("用户手动指定架构: {}", force_arch);
        return match force_arch.as_str() {
            "amd64" | "x86_64" => "amd64",
            "386" | "i386" => "386",
            "arm64" | "aarch64" => "arm64",
            "armv5" => "armv5",
            _ => "amd64",
        };
    }

    info!("Rust ARCH 常量: {}", std::env::consts::ARCH);

    if cfg!(target_os = "windows") {
        match std::env::consts::ARCH {
            "x86_64" => "amd64",
            "x86" => "386",
            "aarch64" => "arm64",
            _ => "amd64",
        }
    } else if cfg!(target_os = "linux") {
        let mut detected_arch = "amd64";

        if let Ok(output) = Command::new("uname").arg("-m").output() {
            if let Ok(arch_str) = String::from_utf8(output.stdout) {
                let arch = arch_str.trim();
                info!("uname -m 输出: '{}'", arch);

                detected_arch = match arch {
                    "x86_64" | "amd64" => "amd64",
                    "i386" | "i486" | "i586" | "i686" => "386",
                    "aarch64" | "arm64" => "arm64",
                    "armv7l" | "armv6l" => "armv5",
                    _ => match std::env::consts::ARCH {
                        "x86_64" => "amd64",
                        "x86" => "386",
                        "aarch64" => "arm64",
                        _ => "amd64",
                    },
                };
                info!("通过 uname 检测到的架构: {}", detected_arch);
            }
        } else {
            info!("uname 命令执行失败，使用 Rust ARCH 常量");
        }

        if detected_arch == "amd64" && std::env::consts::ARCH != "x86_64" {
            detected_arch = match std::env::consts::ARCH {
                "x86_64" => "amd64",
                "x86" => "386",
                "aarch64" => "arm64",
                "arm" => "armv5",
                _ => "amd64",
            };
            info!("通过 Rust ARCH 常量检测到的架构: {}", detected_arch);
        }

        detected_arch
    } else if cfg!(target_os = "macos") {
        let mut detected_arch = "amd64";

        if let Ok(output) = Command::new("uname").arg("-m").output() {
            if let Ok(arch_str) = String::from_utf8(output.stdout) {
                let arch = arch_str.trim();
                info!("uname -m 输出: '{}'", arch);

                detected_arch = match arch {
                    "x86_64" | "amd64" => "amd64",
                    "i386" | "i486" | "i586" | "i686" => "386",
                    "aarch64" | "arm64" => "arm64",
                    "armv7l" | "armv6l" => "armv5",
                    _ => match std::env::consts::ARCH {
                        "x86_64" => "amd64",
                        "x86" => "386",
                        "aarch64" => "arm64",
                        _ => "amd64",
                    },
                };
                info!("通过 uname 检测到的架构: {}", detected_arch);
            }
        } else {
            info!("uname 命令执行失败，使用 Rust ARCH 常量");
        }

        if detected_arch == "amd64" && std::env::consts::ARCH != "x86_64" {
            detected_arch = match std::env::consts::ARCH {
                "x86_64" => "amd64",
                "x86" => "386",
                "aarch64" => "arm64",
                "arm" => "armv5",
                _ => "amd64",
            };
            info!("通过 Rust ARCH 常量检测到的架构: {}", detected_arch);
        }

        detected_arch
    } else {
        info!("其他平台，使用默认架构 amd64");
        "amd64"
    }
}

#[tauri::command]
pub async fn get_latest_kernel_version_cmd() -> Result<String, String> {
    get_latest_kernel_version().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_kernel_releases_cmd() -> Result<Vec<String>, String> {
    get_kernel_releases().await.map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "versioning.tests.rs"]
mod tests;
