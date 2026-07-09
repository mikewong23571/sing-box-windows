use crate::app::constants::{api, messages};
use crate::app::network_config;
use crate::utils::app_util::get_work_dir_sync;
use semver::Version;
use serde_json::json;
use std::env;
use std::fs;
use std::path::Path;
use tauri::{Emitter, Manager};

const RELEASES_PAGE_URL: &str = "https://github.com/xinggaoya/sing-box-windows/releases";

// 获取当前平台标识符 - 使用 Rust 标准库，更准确
fn get_platform_identifier() -> &'static str {
    env::consts::OS
}

fn supports_in_app_update_for_platform(platform: &str) -> bool {
    platform == "windows"
}

fn supports_in_app_update() -> bool {
    supports_in_app_update_for_platform(get_platform_identifier())
}

fn resolve_release_page_url(release: &serde_json::Value) -> String {
    release["html_url"]
        .as_str()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or(RELEASES_PAGE_URL)
        .to_string()
}

#[cfg(test)]
#[path = "update_service.tests.rs"]
mod tests;
// 获取当前架构
fn get_current_arch() -> &'static str {
    env::consts::ARCH
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageKind {
    Exe,
    Msi,
    AppImage,
    Deb,
    Rpm,
    Dmg,
    AppTarGz,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinuxPackagePreference {
    Deb,
    Rpm,
    AppImage,
}

fn get_package_kind(filename: &str) -> PackageKind {
    let filename_lower = filename.to_ascii_lowercase();

    if filename_lower.ends_with(".app.tar.gz") {
        PackageKind::AppTarGz
    } else if filename_lower.ends_with(".appimage") {
        PackageKind::AppImage
    } else if filename_lower.ends_with(".msi") {
        PackageKind::Msi
    } else if filename_lower.ends_with(".exe") {
        PackageKind::Exe
    } else if filename_lower.ends_with(".deb") {
        PackageKind::Deb
    } else if filename_lower.ends_with(".rpm") {
        PackageKind::Rpm
    } else if filename_lower.ends_with(".dmg") {
        PackageKind::Dmg
    } else {
        PackageKind::Unknown
    }
}

fn extract_linux_release_identifiers(contents: &str) -> Vec<String> {
    contents
        .lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(key, _)| *key == "ID" || *key == "ID_LIKE")
        .flat_map(|(_, value)| {
            value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .split_whitespace()
                .map(|item| item.to_ascii_lowercase())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn detect_linux_package_preference_from_os_release(contents: &str) -> LinuxPackagePreference {
    let identifiers = extract_linux_release_identifiers(contents);

    if identifiers
        .iter()
        .any(|id| matches!(id.as_str(), "debian" | "ubuntu"))
    {
        LinuxPackagePreference::Deb
    } else if identifiers.iter().any(|id| {
        matches!(
            id.as_str(),
            "fedora"
                | "rhel"
                | "centos"
                | "suse"
                | "opensuse"
                | "opensuse-tumbleweed"
                | "opensuse-leap"
                | "rocky"
                | "almalinux"
        )
    }) {
        LinuxPackagePreference::Rpm
    } else {
        LinuxPackagePreference::AppImage
    }
}

fn detect_linux_package_preference() -> LinuxPackagePreference {
    if get_platform_identifier() != "linux" {
        return LinuxPackagePreference::AppImage;
    }

    ["/etc/os-release", "/usr/lib/os-release"]
        .into_iter()
        .find_map(|path| fs::read_to_string(path).ok())
        .map(|contents| detect_linux_package_preference_from_os_release(&contents))
        .unwrap_or(LinuxPackagePreference::AppImage)
}

fn get_platform_priority_for(
    filename: &str,
    platform: &str,
    arch: &str,
    linux_preference: LinuxPackagePreference,
) -> i32 {
    let filename_lower = filename.to_lowercase();
    let package_kind = get_package_kind(filename);

    let base_priority = match platform {
        "windows" => match package_kind {
            PackageKind::Exe => 20,
            PackageKind::Msi => 10,
            _ => 0,
        },
        "linux" => match linux_preference {
            LinuxPackagePreference::Deb => match package_kind {
                PackageKind::Deb => 20,
                PackageKind::AppImage => 15,
                PackageKind::Rpm => 10,
                _ => 0,
            },
            LinuxPackagePreference::Rpm => match package_kind {
                PackageKind::Rpm => 20,
                PackageKind::AppImage => 15,
                PackageKind::Deb => 10,
                _ => 0,
            },
            LinuxPackagePreference::AppImage => match package_kind {
                PackageKind::AppImage => 20,
                PackageKind::Rpm => 15,
                PackageKind::Deb => 10,
                _ => 0,
            },
        },
        "macos" => match package_kind {
            PackageKind::Dmg => 20,
            PackageKind::AppTarGz => 10,
            _ => 0,
        },
        _ => 0,
    };

    if base_priority == 0 {
        return 0;
    }

    let arch_bonus = match arch {
        "x86_64"
            if filename_lower.contains("x64")
                || filename_lower.contains("x86_64")
                || filename_lower.contains("amd64") =>
        {
            5
        }
        "x86_64" => 0,
        "aarch64" => {
            if filename_lower.contains("arm64") || filename_lower.contains("aarch64") {
                5
            } else if filename_lower.contains("universal") {
                4
            } else {
                0
            }
        }
        _ => 0,
    };

    let special_bonus = if filename_lower.contains("portable") {
        2
    } else if filename_lower.contains("installer") || filename_lower.contains("latest") {
        1
    } else {
        0
    };

    base_priority + arch_bonus + special_bonus
}

#[allow(dead_code)]
fn get_platform_priority(filename: &str) -> i32 {
    get_platform_priority_for(
        filename,
        get_platform_identifier(),
        get_current_arch(),
        detect_linux_package_preference(),
    )
}

fn resolve_update_filename(download_url: &str, platform: &str) -> &'static str {
    match get_package_kind(download_url) {
        PackageKind::Msi => "update.msi",
        PackageKind::Exe => "update.exe",
        PackageKind::AppImage => "update.AppImage",
        PackageKind::Deb => "update.deb",
        PackageKind::Rpm => "update.rpm",
        PackageKind::Dmg => "update.dmg",
        PackageKind::AppTarGz => "update.app.tar.gz",
        PackageKind::Unknown => match platform {
            "windows" => "update.exe",
            "linux" => "update.AppImage",
            "macos" => "update.dmg",
            _ => "update.bin",
        },
    }
}

fn resolve_install_message(platform: &str, download_url: &str) -> &'static str {
    match platform {
        "windows" => "安装程序已启动，请按照提示完成安装",
        "linux" => match get_package_kind(download_url) {
            PackageKind::AppImage => "正在启动新版本应用程序...",
            PackageKind::Deb | PackageKind::Rpm => "正在安装软件包，请根据提示输入密码...",
            _ => "正在启动更新程序...",
        },
        "macos" => match get_package_kind(download_url) {
            PackageKind::Dmg => "正在挂载安装镜像...",
            PackageKind::AppTarGz => "正在解压应用程序...",
            _ => "正在启动安装程序...",
        },
        _ => "正在启动安装程序...",
    }
}

fn command_exists_on_path(command: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|path| path.join(command).is_file()))
        .unwrap_or(false)
}

// 检查文件是否匹配当前平台
#[allow(dead_code)]
fn is_platform_compatible(filename: &str) -> bool {
    is_platform_compatible_for(filename, get_platform_identifier(), get_current_arch())
}

// 检查架构兼容性（仅桌面平台）
fn check_arch_compatibility(filename: &str, current_arch: &str) -> bool {
    let filename_lower = filename.to_lowercase();

    match current_arch {
        "x86_64" => {
            // x64 架构优先选择 x64 包，也接受通用包
            filename_lower.contains("x64")
                || filename_lower.contains("x86_64")
                || filename_lower.contains("amd64")
                || (!filename_lower.contains("arm") && !filename_lower.contains("aarch64"))
            // 没有架构标识时默认兼容
        }
        "aarch64" => {
            // ARM64 Mac 优先选择 ARM64 或 Universal 包
            filename_lower.contains("arm64")
                || filename_lower.contains("aarch64")
                || filename_lower.contains("universal")
        }
        "arm" | "armv7" => {
            // ARM32
            filename_lower.contains("arm32")
                || filename_lower.contains("armv7")
                || (filename_lower.contains("arm") && !filename_lower.contains("64"))
        }
        "x86" => {
            // 32位 x86
            filename_lower.contains("i386")
                || filename_lower.contains("386")
                || (filename_lower.contains("x86") && !filename_lower.contains("64"))
        }
        _ => true, // 其他架构保守处理
    }
}

// 更新信息结构体
#[derive(serde::Serialize, Debug)]
pub struct UpdateInfo {
    pub latest_version: String,
    pub download_url: String,
    pub release_page_url: String,
    pub has_update: bool,
    pub release_notes: Option<String>,
    pub release_date: Option<String>,
    pub file_size: Option<u64>,
    pub is_prerelease: bool,
    pub supports_in_app_update: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpdateChannel {
    Stable,
    Prerelease,
    Autobuild,
}

impl UpdateChannel {
    fn from_inputs(channel: Option<&str>, include_prerelease: bool) -> Self {
        match channel.map(|c| c.trim().to_ascii_lowercase()) {
            Some(ref c) if c == "stable" => Self::Stable,
            Some(ref c) if c == "prerelease" => Self::Prerelease,
            Some(ref c) if c == "autobuild" => Self::Autobuild,
            _ => {
                if include_prerelease {
                    Self::Prerelease
                } else {
                    Self::Stable
                }
            }
        }
    }

    fn uses_release_list(&self) -> bool {
        !matches!(self, Self::Stable)
    }
}

fn select_release_by_channel(
    releases: &[serde_json::Value],
    channel: UpdateChannel,
) -> Option<serde_json::Value> {
    match channel {
        UpdateChannel::Stable => releases
            .iter()
            .find(|release| !release["prerelease"].as_bool().unwrap_or(false))
            .cloned(),
        UpdateChannel::Prerelease => releases.first().cloned(),
        UpdateChannel::Autobuild => releases
            .iter()
            .find(|release| {
                let tag_name = release["tag_name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let release_name = release["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                tag_name.contains("autobuild") || release_name.contains("autobuild")
            })
            .cloned()
            .or_else(|| {
                releases
                    .iter()
                    .find(|release| release["prerelease"].as_bool().unwrap_or(false))
                    .cloned()
            })
            .or_else(|| releases.first().cloned()),
    }
}

// 版本比较函数
fn compare_versions(current: &str, latest: &str) -> bool {
    // 清理版本号，移除 'v' 前缀和其他非版本信息
    let clean_current = current
        .trim_start_matches('v')
        .split_whitespace()
        .next()
        .unwrap_or(current);
    let clean_latest = latest
        .trim_start_matches('v')
        .split_whitespace()
        .next()
        .unwrap_or(latest);

    // 尝试使用 semver 进行比较
    match (Version::parse(clean_current), Version::parse(clean_latest)) {
        (Ok(curr), Ok(lat)) => lat > curr,
        _ => {
            // 如果无法解析为语义版本，则进行字符串比较
            clean_latest != clean_current
        }
    }
}

/// 从 assets 中按平台/架构/发行版偏好挑选最优下载资源（纯逻辑）。
pub(crate) fn pick_best_download_asset(
    assets: &[serde_json::Value],
    platform: &str,
    arch: &str,
    linux_preference: LinuxPackagePreference,
) -> (String, Option<u64>, i32) {
    let mut download_url = String::new();
    let mut file_size: Option<u64> = None;
    let mut best_priority = 0;

    for asset in assets {
        let name = asset["name"].as_str().unwrap_or("");
        if !is_platform_compatible_for(name, platform, arch) {
            continue;
        }
        let priority = get_platform_priority_for(name, platform, arch, linux_preference);
        if priority > best_priority {
            download_url = asset["browser_download_url"]
                .as_str()
                .unwrap_or("")
                .to_string();
            file_size = asset["size"].as_u64();
            best_priority = priority;
            // 最高优先级约为 27（基础 20 + 架构 5 + 特殊 2）
            if priority >= 25 {
                break;
            }
        }
    }

    (download_url, file_size, best_priority)
}

fn is_platform_compatible_for(filename: &str, platform: &str, arch: &str) -> bool {
    let package_kind = get_package_kind(filename);
    let extension_match = match platform {
        "windows" => matches!(package_kind, PackageKind::Msi | PackageKind::Exe),
        "linux" => matches!(
            package_kind,
            PackageKind::AppImage | PackageKind::Deb | PackageKind::Rpm
        ),
        "macos" => matches!(package_kind, PackageKind::Dmg | PackageKind::AppTarGz),
        _ => false,
    };
    if !extension_match {
        return false;
    }
    check_arch_compatibility(filename, arch)
}

/// 从已解析的 release JSON 构造 UpdateInfo（纯逻辑，无网络）。
pub(crate) fn build_update_info_from_release(
    release: &serde_json::Value,
    current_version: &str,
    platform: &str,
    arch: &str,
    linux_preference: LinuxPackagePreference,
    supports_in_app: bool,
) -> Result<UpdateInfo, String> {
    let tag_name = release["tag_name"]
        .as_str()
        .ok_or_else(|| format!("{}: 无法解析版本号", messages::ERR_GET_VERSION_FAILED))
        .map(|v| v.trim_start_matches('v').to_string())?;

    let release_notes = release["body"].as_str().map(|s| s.to_string());
    let release_page_url = resolve_release_page_url(release);
    let release_date = release["published_at"].as_str().map(|s| s.to_string());
    let is_prerelease = release["prerelease"].as_bool().unwrap_or(false);

    let assets = release["assets"]
        .as_array()
        .ok_or_else(|| format!("{}: 无法获取下载资源", messages::ERR_GET_VERSION_FAILED))?;

    let (download_url, file_size, _) =
        pick_best_download_asset(assets, platform, arch, linux_preference);

    if supports_in_app && download_url.is_empty() {
        return Err(format!(
            "{}: 无法获取下载链接",
            messages::ERR_GET_VERSION_FAILED
        ));
    }

    let has_update = compare_versions(current_version, &tag_name);

    Ok(UpdateInfo {
        latest_version: tag_name,
        download_url,
        release_page_url,
        has_update,
        release_notes,
        release_date,
        file_size,
        is_prerelease,
        supports_in_app_update: supports_in_app,
    })
}

/// 稳定通道用 latest API，其它通道用 releases 列表（纯逻辑）。
pub(crate) fn default_check_update_api_url(uses_release_list: bool) -> &'static str {
    if uses_release_list {
        "https://api.github.com/repos/xinggaoya/sing-box-windows/releases"
    } else {
        api::GITHUB_API_URL
    }
}

/// 从 releases 列表按通道挑选一条 release（纯逻辑）。
pub(crate) fn select_release_json_for_channel(
    releases: &[serde_json::Value],
    channel: UpdateChannel,
) -> Result<serde_json::Value, String> {
    if releases.is_empty() {
        return Err(format!(
            "{}: 无法获取版本列表",
            messages::ERR_GET_VERSION_FAILED
        ));
    }
    select_release_by_channel(releases, channel)
        .ok_or_else(|| format!("{}: 未找到匹配通道的版本", messages::ERR_GET_VERSION_FAILED))
}

/// 从任意 API URL 拉取并构造 UpdateInfo（可注入本地 mock，生产 URL 不变）。
#[allow(clippy::too_many_arguments)]
pub(crate) async fn check_update_from_api_url(
    api_url: &str,
    uses_release_list: bool,
    current_version: &str,
    platform: &str,
    arch: &str,
    linux_preference: LinuxPackagePreference,
    supports_in_app: bool,
    channel: UpdateChannel,
) -> Result<UpdateInfo, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            network_config::HTTP_TIMEOUT_SECONDS,
        ))
        .no_proxy()
        .build()
        .map_err(|e| format!("{}: {}", messages::ERR_HTTP_CLIENT_FAILED, e))?;

    let response = client
        .get(api_url)
        .header("User-Agent", api::USER_AGENT)
        .send()
        .await
        .map_err(|e| format!("{}: {}", messages::ERR_GET_VERSION_FAILED, e))?;

    let release: serde_json::Value = if uses_release_list {
        let releases: Vec<serde_json::Value> = response
            .json()
            .await
            .map_err(|e| format!("{}: {}", messages::ERR_GET_VERSION_FAILED, e))?;
        select_release_json_for_channel(&releases, channel)?
    } else {
        response
            .json()
            .await
            .map_err(|e| format!("{}: {}", messages::ERR_GET_VERSION_FAILED, e))?
    };

    build_update_info_from_release(
        &release,
        current_version,
        platform,
        arch,
        linux_preference,
        supports_in_app,
    )
}

/// 应用内更新不支持时的错误文案（纯逻辑）。
pub(crate) fn in_app_update_unsupported_message() -> &'static str {
    "当前平台暂不支持应用内更新，请前往版本页面下载最新版本"
}

/// 下载文件缺失时的错误文案（纯逻辑）。
pub(crate) fn downloaded_update_missing_message() -> &'static str {
    "下载的文件不存在"
}

/// 根据工作目录与下载 URL 解析本地保存路径（纯逻辑）。
pub(crate) fn resolve_update_download_path(
    work_dir: &Path,
    download_url: &str,
    platform: &str,
) -> std::path::PathBuf {
    work_dir.join(resolve_update_filename(download_url, platform))
}

/// 构造 update-progress 事件负载（纯逻辑）。
pub(crate) fn build_update_progress_payload(
    status: &str,
    progress: u64,
    message: impl Into<String>,
) -> serde_json::Value {
    json!({
        "status": status,
        "progress": progress,
        "message": message.into()
    })
}

/// RPM 安装前置检查：系统是否具备 rpm 命令（可注入探测结果）。
pub(crate) fn rpm_install_precheck(rpm_command_exists: bool) -> Result<(), String> {
    if rpm_command_exists {
        Ok(())
    } else {
        Err("当前系统缺少 rpm 命令，无法安装 RPM 包".to_string())
    }
}

/// 应用内更新支持性前置检查（纯逻辑，可注入）。
pub(crate) fn ensure_in_app_update_supported(supports: bool) -> Result<(), String> {
    if supports {
        Ok(())
    } else {
        Err(in_app_update_unsupported_message().to_string())
    }
}

/// 校验已下载更新文件存在（纯逻辑）。
pub(crate) fn validate_downloaded_update_file(path: &Path) -> Result<(), String> {
    if path.exists() {
        Ok(())
    } else {
        Err(downloaded_update_missing_message().to_string())
    }
}

/// 无窗口下载更新包到 work_dir（hermetic：可注入 URL/平台）。
#[allow(dead_code)]
pub(crate) async fn download_update_package_to_work_dir(
    download_url: &str,
    work_dir: &Path,
    platform: &str,
    supports_in_app: bool,
) -> Result<std::path::PathBuf, String> {
    ensure_in_app_update_supported(supports_in_app)?;
    let download_path = resolve_update_download_path(work_dir, download_url, platform);
    crate::utils::file_util::download_with_fallback(
        download_url,
        download_path
            .to_str()
            .ok_or_else(|| "下载路径无效".to_string())?,
        |_progress| {},
    )
    .await
    .map_err(|e| format!("下载更新失败: {}", e))?;
    validate_downloaded_update_file(&download_path)?;
    Ok(download_path)
}

// 检查更新
#[tauri::command]
pub async fn check_update(
    current_version: String,
    include_prerelease: Option<bool>,
    update_channel: Option<String>,
) -> Result<UpdateInfo, String> {
    let include_prerelease = include_prerelease.unwrap_or(false);
    let channel = UpdateChannel::from_inputs(update_channel.as_deref(), include_prerelease);
    let uses_list = channel.uses_release_list();
    let api_url = default_check_update_api_url(uses_list);

    check_update_from_api_url(
        api_url,
        uses_list,
        &current_version,
        get_platform_identifier(),
        get_current_arch(),
        detect_linux_package_preference(),
        supports_in_app_update(),
        channel,
    )
    .await
}

// 下载更新
#[tauri::command]
pub async fn download_update(app_handle: tauri::AppHandle) -> Result<(), String> {
    let window = app_handle
        .get_webview_window("main")
        .ok_or("无法获取主窗口")?;

    // 这里可以实现实际的下载逻辑
    // 目前先发送一个模拟的完成事件
    let _ = window.emit(
        "update-progress",
        json!({
            "status": "completed",
            "progress": 100,
            "message": "下载功能待实现"
        }),
    );

    Ok(())
}

// 获取当前平台信息（简化版，兼容旧接口）
#[tauri::command]
pub async fn get_platform_info() -> Result<String, String> {
    Ok(get_platform_identifier().to_string())
}

// 获取详细的平台信息（包括操作系统和架构）
#[tauri::command]
pub async fn get_detailed_platform_info() -> Result<PlatformDetailedInfo, String> {
    Ok(PlatformDetailedInfo::current())
}

// 详细平台信息结构体
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlatformDetailedInfo {
    pub os: String,           // 操作系统：windows, linux, macos
    pub arch: String,         // 架构：x86_64, aarch64, etc.
    pub display_name: String, // 显示名称：Windows x64, macOS ARM64 等
}

impl PlatformDetailedInfo {
    pub fn current() -> Self {
        let os = env::consts::OS.to_string();
        let arch = env::consts::ARCH.to_string();

        // 生成友好的显示名称
        let display_name = match (os.as_str(), arch.as_str()) {
            ("windows", "x86_64") => "Windows x64".to_string(),
            ("windows", "x86") => "Windows x86".to_string(),
            ("windows", "aarch64") => "Windows ARM64".to_string(),
            ("linux", "x86_64") => "Linux x64".to_string(),
            ("linux", "x86") => "Linux x86".to_string(),
            ("linux", "aarch64") => "Linux ARM64".to_string(),
            ("linux", "arm") => "Linux ARM".to_string(),
            ("macos", "x86_64") => "macOS Intel".to_string(),
            ("macos", "aarch64") => "macOS Apple Silicon".to_string(),
            ("macos", "arm") => "macOS ARM".to_string(),
            _ => format!("{} ({})", os, arch),
        };

        Self {
            os,
            arch,
            display_name,
        }
    }
}

/// 描述安装动作（纯逻辑，不真正 spawn）。
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InstallAction {
    Msiexec,
    RunExe,
    ChmodAndRun,
    PkexecDpkg,
    PkexecRpm,
    OpenDmg,
    TarExtract,
    RunBinary,
    Unsupported(&'static str),
}

/// 根据平台与下载 URL 选择安装动作。
#[allow(dead_code)]
pub(crate) fn plan_install_action(platform: &str, download_url: &str) -> InstallAction {
    match platform {
        "windows" => match get_package_kind(download_url) {
            PackageKind::Msi => InstallAction::Msiexec,
            PackageKind::Exe => InstallAction::RunExe,
            _ => InstallAction::RunBinary,
        },
        "linux" => match get_package_kind(download_url) {
            PackageKind::AppImage => InstallAction::ChmodAndRun,
            PackageKind::Deb => InstallAction::PkexecDpkg,
            PackageKind::Rpm => InstallAction::PkexecRpm,
            _ => InstallAction::RunBinary,
        },
        "macos" => match get_package_kind(download_url) {
            PackageKind::Dmg => InstallAction::OpenDmg,
            PackageKind::AppTarGz => InstallAction::TarExtract,
            _ => InstallAction::RunBinary,
        },
        _ => InstallAction::Unsupported("当前平台暂不支持应用内更新"),
    }
}

// 下载并安装更新
#[tauri::command]
pub async fn download_and_install_update(
    app_handle: tauri::AppHandle,
    download_url: String,
) -> Result<(), String> {
    let window = app_handle
        .get_webview_window("main")
        .ok_or("无法获取主窗口")?;

    if let Err(error_msg) = ensure_in_app_update_supported(supports_in_app_update()) {
        let _ = window.emit(
            "update-progress",
            build_update_progress_payload("error", 0, error_msg.clone()),
        );
        return Err(error_msg);
    }

    let work_dir = get_work_dir_sync();

    // 根据下载链接和平台确定下载文件名
    let platform = get_platform_identifier();
    let download_path = resolve_update_download_path(Path::new(&work_dir), &download_url, platform);

    // 发送开始下载事件
    let _ = window.emit(
        "update-progress",
        build_update_progress_payload("downloading", 0, "开始下载更新..."),
    );

    // 下载更新文件
    let window_clone = window.clone();
    // 使用fallback下载函数
    if let Err(e) = crate::utils::file_util::download_with_fallback(
        &download_url,
        download_path.to_str().unwrap(),
        move |progress| {
            let _ = window_clone.emit(
                "update-progress",
                json!({
                    "status": "downloading",
                    "progress": progress,
                    "message": format!("正在下载: {}%", progress)
                }),
            );
        },
    )
    .await
    {
        let _ = window.emit(
            "update-progress",
            build_update_progress_payload("error", 0, format!("下载失败: {}", e)),
        );
        return Err(format!("下载更新失败: {}", e));
    }

    // 验证下载的文件
    if let Err(error_msg) = validate_downloaded_update_file(&download_path) {
        let _ = window.emit(
            "update-progress",
            build_update_progress_payload("error", 0, error_msg.clone()),
        );
        return Err(error_msg);
    }

    // 发送下载完成事件
    let _ = window.emit(
        "update-progress",
        build_update_progress_payload("completed", 100, "下载完成，准备安装..."),
    );

    // 启动安装程序（在后台运行）
    let install_result: Result<(), String> = match platform {
        "windows" => {
            // Windows: 根据文件类型选择不同的处理方式
            if matches!(get_package_kind(&download_url), PackageKind::Msi) {
                // MSI文件: 使用 msiexec 安装
                let mut cmd = tokio::process::Command::new("msiexec");
                cmd.arg("/i").arg(&download_path).arg("/passive");
                #[cfg(target_os = "windows")]
                cmd.creation_flags(crate::app::constants::core::process::CREATE_NO_WINDOW);
                cmd.spawn()
                    .map(|_| ())
                    .map_err(|e| format!("启动安装程序失败: {}", e))
            } else if matches!(get_package_kind(&download_url), PackageKind::Exe) {
                // EXE文件: 直接运行
                let mut cmd = tokio::process::Command::new(&download_path);
                #[cfg(target_os = "windows")]
                cmd.creation_flags(crate::app::constants::core::process::CREATE_NO_WINDOW);
                cmd.spawn()
                    .map(|_| ())
                    .map_err(|e| format!("启动安装程序失败: {}", e))
            } else {
                // 其他文件：尝试用默认方式运行
                let mut cmd = tokio::process::Command::new(&download_path);
                #[cfg(target_os = "windows")]
                cmd.creation_flags(crate::app::constants::core::process::CREATE_NO_WINDOW);
                cmd.spawn()
                    .map(|_| ())
                    .map_err(|e| format!("启动安装程序失败: {}", e))
            }
        }
        "linux" => {
            // Linux: 根据文件类型执行不同的安装逻辑
            match get_package_kind(&download_url) {
                PackageKind::AppImage => {
                    // AppImage: 添加执行权限并运行
                    let mut chmod_cmd = tokio::process::Command::new("chmod");
                    chmod_cmd.arg("+x").arg(&download_path);
                    chmod_cmd
                        .spawn()
                        .map_err(|e| format!("启动安装程序失败: {}", e))
                        .and_then(|_| {
                            let mut run_cmd = tokio::process::Command::new(&download_path);
                            run_cmd
                                .spawn()
                                .map(|_| ())
                                .map_err(|e| format!("启动安装程序失败: {}", e))
                        })
                }
                PackageKind::Deb => {
                    // DEB包: 使用pkexec安装（需要管理员权限）
                    let mut cmd = tokio::process::Command::new("pkexec");
                    cmd.arg("dpkg")
                        .arg("-i")
                        .arg(&download_path)
                        .arg("--force-architecture");
                    cmd.spawn()
                        .map(|_| ())
                        .map_err(|e| format!("启动安装程序失败: {}", e))
                }
                PackageKind::Rpm => {
                    if let Err(e) = rpm_install_precheck(command_exists_on_path("rpm")) {
                        Err(e)
                    } else {
                        let mut cmd = tokio::process::Command::new("pkexec");
                        cmd.arg("rpm").arg("-Uvh").arg(&download_path);
                        cmd.spawn()
                            .map(|_| ())
                            .map_err(|e| format!("启动安装程序失败: {}", e))
                    }
                }
                _ => {
                    // 其他二进制文件
                    let mut cmd = tokio::process::Command::new(&download_path);
                    cmd.spawn()
                        .map(|_| ())
                        .map_err(|e| format!("启动安装程序失败: {}", e))
                }
            }
        }
        "macos" => {
            // macOS: 根据文件类型执行不同的安装逻辑
            if matches!(get_package_kind(&download_url), PackageKind::Dmg) {
                // DMG: 使用open命令挂载
                let mut cmd = tokio::process::Command::new("open");
                cmd.arg(&download_path);
                cmd.spawn()
                    .map(|_| ())
                    .map_err(|e| format!("启动安装程序失败: {}", e))
            } else if matches!(get_package_kind(&download_url), PackageKind::AppTarGz) {
                // app.tar.gz: 解压并运行
                let mut cmd = tokio::process::Command::new("tar");
                cmd.arg("-xzf").arg(&download_path);
                cmd.spawn()
                    .map(|_| ())
                    .map_err(|e| format!("启动安装程序失败: {}", e))
            } else {
                let mut cmd = tokio::process::Command::new(&download_path);
                cmd.spawn()
                    .map(|_| ())
                    .map_err(|e| format!("启动安装程序失败: {}", e))
            }
        }
        _ => {
            // 其他平台：尝试直接运行
            let mut cmd = tokio::process::Command::new(&download_path);
            cmd.spawn()
                .map(|_| ())
                .map_err(|e| format!("启动安装程序失败: {}", e))
        }
    };

    match install_result {
        Ok(()) => {
            // 安装程序启动成功，发送安装开始事件
            let install_message = resolve_install_message(platform, &download_url);

            let _ = window.emit(
                "update-progress",
                build_update_progress_payload("installing", 100, install_message),
            );
            Ok(())
        }
        Err(error_msg) => {
            let _ = window.emit(
                "update-progress",
                build_update_progress_payload("error", 0, error_msg.clone()),
            );
            Err(error_msg)
        }
    }
}
