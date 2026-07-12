use crate::app::constants::paths;
use crate::app::storage::enhanced_storage_service::{
    db_get_app_config, db_save_app_config_internal,
};
use crate::utils::http_client;
use semver::Version;
use std::io::Cursor;
use std::path::Path;
use tauri::{AppHandle, Manager, Runtime};
use tracing::{info, warn};

/// 内嵌内核平台目录名（与资源布局一致）。
pub(crate) fn embedded_platform_id() -> Option<&'static str> {
    if cfg!(target_os = "windows") {
        Some("windows")
    } else if cfg!(target_os = "linux") {
        Some("linux")
    } else if cfg!(target_os = "macos") {
        Some("macos")
    } else {
        None
    }
}

/// 当前平台内嵌内核可执行文件名。
pub(crate) fn embedded_executable_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "sing-box.exe"
    } else {
        "sing-box"
    }
}

/// 在 resource_dir 下定位内嵌内核目录与二进制（纯 FS）。
pub(crate) fn find_embedded_kernel_paths(
    resource_dir: &Path,
    platform: &str,
    arch: &str,
    executable_name: &str,
) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let candidate_bases = [
        resource_dir.join("kernel"),
        resource_dir.join("resources").join("kernel"),
    ];
    for base in candidate_bases {
        let dir = base.join(platform).join(arch);
        let path = dir.join(executable_name);
        if path.exists() {
            return Some((dir, path));
        }
    }
    None
}

/// 是否应覆盖安装内嵌内核（纯逻辑）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EmbeddedInstallDecision {
    Install,
    #[allow(dead_code)]
    SkipNoLocalAndNoResource,
    SkipLocalMissingEmbeddedVersion,
    SkipLocalUnknownVersion,
    SkipLocalNotOlder,
    SkipVersionUncomparable,
}

/// 根据本地/内嵌版本决定是否安装。
pub(crate) fn decide_embedded_install(
    local_kernel_exists: bool,
    embedded_version: Option<&str>,
    installed_version: Option<&str>,
) -> EmbeddedInstallDecision {
    if !local_kernel_exists {
        return EmbeddedInstallDecision::Install;
    }
    let Some(target) = embedded_version else {
        return EmbeddedInstallDecision::SkipLocalMissingEmbeddedVersion;
    };
    let Some(current) = installed_version else {
        return EmbeddedInstallDecision::SkipLocalUnknownVersion;
    };
    match is_embedded_newer(current, target) {
        Some(true) => EmbeddedInstallDecision::Install,
        Some(false) => EmbeddedInstallDecision::SkipLocalNotOlder,
        None => EmbeddedInstallDecision::SkipVersionUncomparable,
    }
}

/// 复制内嵌内核到工作目录并设置执行权限（无 AppHandle）。
pub(crate) async fn copy_embedded_kernel_binary(
    embedded_kernel_path: &Path,
    kernel_path: &Path,
) -> Result<(), String> {
    if let Some(parent) = kernel_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建内核目录失败: {}", e))?;
    }
    tokio::fs::copy(embedded_kernel_path, kernel_path)
        .await
        .map_err(|e| format!("复制内嵌内核失败: {}", e))?;
    if cfg!(target_os = "linux") || cfg!(target_os = "macos") {
        if let Err(e) = set_executable_permission(kernel_path) {
            warn!("设置内核执行权限失败: {}", e);
        }
    }
    Ok(())
}

pub async fn ensure_embedded_kernel<R: Runtime>(
    app_handle: &AppHandle<R>,
) -> Result<Option<String>, String> {
    let kernel_path = paths::get_kernel_path();

    let resource_dir = match app_handle.path().resource_dir() {
        Ok(dir) => dir,
        Err(e) => {
            warn!("无法获取资源目录，跳过内嵌内核检查: {}", e);
            return Ok(None);
        }
    };

    let Some(platform) = embedded_platform_id() else {
        warn!("当前平台不支持内嵌内核安装");
        return Ok(None);
    };

    let arch = super::versioning::get_system_arch();
    let executable_name = embedded_executable_name();

    let Some((embedded_dir, embedded_kernel_path)) =
        find_embedded_kernel_paths(&resource_dir, platform, arch, executable_name)
    else {
        info!("未找到内嵌内核资源文件，跳过安装");
        return Ok(None);
    };
    let embedded_version = read_embedded_version(&embedded_dir).await;

    let installed_version = if kernel_path.exists() {
        resolve_installed_version(app_handle, &kernel_path).await
    } else {
        None
    };

    match decide_embedded_install(
        kernel_path.exists(),
        embedded_version.as_deref(),
        installed_version.as_deref(),
    ) {
        EmbeddedInstallDecision::Install => {
            if kernel_path.exists() {
                info!(
                    "检测到内嵌内核版本更新，将覆盖安装: {:?} -> {:?}",
                    installed_version, embedded_version
                );
            } else {
                info!("未检测到本地内核，准备安装内嵌内核");
            }
        }
        EmbeddedInstallDecision::SkipLocalMissingEmbeddedVersion => {
            info!("当前已存在本地内核，且内嵌资源缺少版本信息，跳过覆盖更新");
            return Ok(None);
        }
        EmbeddedInstallDecision::SkipLocalUnknownVersion => {
            warn!("当前已存在本地内核，但无法识别版本，跳过覆盖更新");
            return Ok(None);
        }
        EmbeddedInstallDecision::SkipLocalNotOlder => {
            info!(
                "本地内核版本不低于内嵌版本，跳过覆盖: 本地={:?}, 内嵌={:?}",
                installed_version, embedded_version
            );
            if let Some(current_version) = installed_version {
                let _ = save_installed_version(app_handle, current_version).await;
            }
            return Ok(None);
        }
        EmbeddedInstallDecision::SkipVersionUncomparable => {
            warn!(
                "无法比较版本，保守跳过覆盖更新: 本地={:?}, 内嵌={:?}",
                installed_version, embedded_version
            );
            return Ok(None);
        }
        EmbeddedInstallDecision::SkipNoLocalAndNoResource => return Ok(None),
    }

    copy_embedded_kernel_binary(&embedded_kernel_path, &kernel_path).await?;

    if let Some(version) = embedded_version.clone() {
        let _ = save_installed_version(app_handle, version).await;
    }

    info!("内嵌内核已安装: {:?}", kernel_path);
    Ok(embedded_version)
}

#[cfg(unix)]
fn set_executable_permission(file_path: &std::path::Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;

    let mut perms = std::fs::metadata(file_path)?.permissions();
    perms.set_mode(perms.mode() | 0o755);
    std::fs::set_permissions(file_path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable_permission(_file_path: &std::path::Path) -> Result<(), std::io::Error> {
    Ok(())
}

async fn read_embedded_version(embedded_dir: &Path) -> Option<String> {
    let version_path = embedded_dir.join("version.txt");
    match tokio::fs::read_to_string(&version_path).await {
        Ok(content) => {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Err(_) => None,
    }
}

async fn resolve_installed_version<R: Runtime>(
    app_handle: &AppHandle<R>,
    kernel_path: &Path,
) -> Option<String> {
    if let Some(version) = read_kernel_version_from_binary(kernel_path).await {
        return Some(version);
    }

    if let Ok(config) = db_get_app_config(app_handle.clone()).await {
        if let Some(version) = config.installed_kernel_version {
            let normalized = normalize_version_string(&version);
            if !normalized.is_empty() {
                return Some(normalized);
            }
        }
    }

    None
}

async fn save_installed_version<R: Runtime>(
    app_handle: &AppHandle<R>,
    version: String,
) -> Result<(), String> {
    let normalized = normalize_version_string(&version);
    if normalized.is_empty() {
        return Ok(());
    }

    match db_get_app_config(app_handle.clone()).await {
        Ok(mut config) => {
            if config.installed_kernel_version.as_deref() != Some(normalized.as_str()) {
                config.installed_kernel_version = Some(normalized);
                db_save_app_config_internal(config, app_handle).await?;
            }
            Ok(())
        }
        Err(e) => {
            warn!("读取应用配置失败，无法保存内核版本信息: {}", e);
            Ok(())
        }
    }
}

async fn read_kernel_version_from_binary(kernel_path: &Path) -> Option<String> {
    let mut cmd = tokio::process::Command::new(kernel_path);
    cmd.arg("version");

    #[cfg(target_os = "windows")]
    cmd.creation_flags(crate::app::constants::core::process::CREATE_NO_WINDOW);

    let output = cmd.output().await.ok()?;
    if !output.status.success() {
        return None;
    }

    extract_version_from_output(&String::from_utf8_lossy(&output.stdout))
}

pub(crate) fn extract_version_from_output(output: &str) -> Option<String> {
    for token in output.split_whitespace() {
        let cleaned =
            token.trim_matches(|c: char| c == ':' || c == ',' || c == ';' || c == ')' || c == '(');
        let normalized = normalize_version_string(cleaned);
        if normalized.is_empty() {
            continue;
        }
        if normalized.chars().any(|c| c.is_ascii_digit()) {
            return Some(normalized);
        }
    }
    None
}

pub(crate) fn normalize_version_string(raw: &str) -> String {
    raw.trim().trim_start_matches('v').to_string()
}

pub(crate) fn is_embedded_newer(current: &str, embedded: &str) -> Option<bool> {
    let current = normalize_version_string(current);
    let embedded = normalize_version_string(embedded);

    if current.is_empty() || embedded.is_empty() {
        return None;
    }

    match (Version::parse(&current), Version::parse(&embedded)) {
        (Ok(current_ver), Ok(embedded_ver)) => Some(embedded_ver > current_ver),
        _ if current == embedded => Some(false),
        _ => None,
    }
}

/// 确保 metacubexd 外部 UI 已就绪。
/// 首次启动时 sing-box 会从 GitHub 下载 metacubexd，此下载在 API 启动前执行，
/// 可能导致稳定性校验超时。此函数在内核启动前预下载，消除该阻塞。
const METACUBEXD_URL: &str =
    "https://github.com/MetaCubeX/metacubexd/archive/refs/heads/gh-pages.zip";
const METACUBEXD_DIR: &str = "metacubexd";
const METACUBEXD_DOWNLOAD_TIMEOUT_SECS: u64 = 120;

/// UI 目录是否已就绪（以 index.html 为标志）。
pub(crate) fn external_ui_ready(work_dir: &Path) -> bool {
    work_dir.join(METACUBEXD_DIR).join("index.html").exists()
}

/// 从 zip 字节安装 metacubexd 到 work_dir（无网络）。
pub(crate) async fn install_external_ui_from_zip_bytes(
    bytes: &[u8],
    work_dir: &Path,
) -> Result<(), String> {
    let ui_dir = work_dir.join(METACUBEXD_DIR);
    let temp_dir = work_dir.join(format!("{}.tmp", METACUBEXD_DIR));
    if temp_dir.exists() {
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }

    extract_zip_to_dir(bytes, &temp_dir)?;

    // GitHub zip 内顶层目录名形如 "metacubexd-gh-pages"，需提取其内容
    let extracted_content = find_single_subdirectory(&temp_dir);
    let source_dir = extracted_content.as_ref().unwrap_or(&temp_dir);

    if ui_dir.exists() {
        let _ = tokio::fs::remove_dir_all(&ui_dir).await;
    }
    tokio::fs::rename(source_dir, &ui_dir)
        .await
        .map_err(|e| format!("移动 metacubexd 目录失败: {}", e))?;

    if temp_dir.exists() {
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }

    info!("metacubexd UI 预下载安装完成: {:?}", ui_dir);
    Ok(())
}

/// 从任意 URL 下载 zip 并安装 UI（可注入本地 mock）。
pub(crate) async fn download_and_install_external_ui_from_url(
    url: &str,
    work_dir: &Path,
    timeout_secs: u64,
) -> Result<(), String> {
    if external_ui_ready(work_dir) {
        return Ok(());
    }

    info!("metacubexd UI 不存在，开始预下载: {}", url);

    let client = http_client::get_client();
    let response = client
        .get(url)
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .send()
        .await
        .map_err(|e| format!("下载 metacubexd 失败: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "下载 metacubexd 失败，HTTP 状态码: {}",
            response.status()
        ));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("读取 metacubexd 响应体失败: {}", e))?;

    info!("metacubexd 下载完成 ({} 字节)，开始解压", bytes.len());

    install_external_ui_from_zip_bytes(&bytes, work_dir).await
}

pub async fn ensure_external_ui() -> Result<(), String> {
    let work_dir = paths::get_kernel_work_dir();
    download_and_install_external_ui_from_url(
        METACUBEXD_URL,
        &work_dir,
        METACUBEXD_DOWNLOAD_TIMEOUT_SECS,
    )
    .await
}

/// 读取内嵌 version.txt（可测）。
#[allow(dead_code)]
pub(crate) async fn read_embedded_version_public(embedded_dir: &Path) -> Option<String> {
    read_embedded_version(embedded_dir).await
}

/// 从二进制运行 version 并解析（可测）。
#[allow(dead_code)]
pub(crate) async fn read_kernel_version_from_binary_public(kernel_path: &Path) -> Option<String> {
    read_kernel_version_from_binary(kernel_path).await
}

/// 将 zip 数据解压到指定目录
pub(crate) fn extract_zip_to_dir(bytes: &[u8], target_dir: &Path) -> Result<(), String> {
    use std::fs;
    fs::create_dir_all(target_dir).map_err(|e| format!("创建临时目录失败: {}", e))?;

    let reader = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| format!("解析 zip 失败: {}", e))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("读取 zip 条目失败: {}", e))?;

        let out_path = match file.enclosed_name() {
            Some(path) => target_dir.join(path),
            None => continue,
        };

        if file.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| format!("创建目录失败: {}", e))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("创建父目录失败: {}", e))?;
            }
            let mut outfile =
                fs::File::create(&out_path).map_err(|e| format!("创建文件失败: {}", e))?;
            std::io::copy(&mut file, &mut outfile).map_err(|e| format!("写入文件失败: {}", e))?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = file.unix_mode() {
                    let perms = fs::Permissions::from_mode(mode);
                    fs::set_permissions(&out_path, perms)
                        .map_err(|e| format!("set permissions failed: {}", e))?;
                }
            }
        }
    }

    Ok(())
}

/// 查找 zip 解压后是否只有一个子目录（GitHub zip 通常如此）
pub(crate) fn find_single_subdirectory(dir: &Path) -> Option<std::path::PathBuf> {
    let mut entries = std::fs::read_dir(dir).ok()?;
    let first = entries.next()?.ok()?;
    // 如果只有一个条目且是目录，使用它作为源目录
    if entries.next().is_none() && first.file_type().ok()?.is_dir() {
        Some(first.path())
    } else {
        None
    }
}

#[cfg(test)]
#[path = "embedded.tests.rs"]
mod tests;
