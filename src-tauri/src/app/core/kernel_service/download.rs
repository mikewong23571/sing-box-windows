use crate::app::core::kernel_service::lifecycle::stop_kernel_with_process;
use crate::app::core::kernel_service::status::{is_kernel_running, is_kernel_running_with_process};
use crate::app::core::kernel_service::versioning::{get_latest_kernel_version, get_system_arch};
use crate::app::core::kernel_service::KernelProcessControl;
use crate::app::core::kernel_service::PROCESS_MANAGER;
use crate::app::kernel_service::stop_kernel;
use crate::app::runtime::change::{RuntimeApplyOptions, RuntimeChange};
use crate::app::runtime::orchestrator::apply_runtime_change;
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use tauri::Manager;
use tauri::{AppHandle, Emitter, WebviewWindow};
use tracing::{info, warn};

/// 平台标识（与 sing-box 发布包命名一致）。
pub(crate) fn kernel_platform_name() -> Result<&'static str, String> {
    if cfg!(target_os = "windows") {
        Ok("windows")
    } else if cfg!(target_os = "linux") {
        Ok("linux")
    } else if cfg!(target_os = "macos") {
        Ok("darwin")
    } else {
        Err("当前平台不支持".to_string())
    }
}

/// 构造内核发布包文件名。
pub(crate) fn kernel_release_filename(version: &str, platform: &str, arch: &str) -> String {
    if platform == "windows" {
        format!("sing-box-{}-windows-{}.zip", version, arch)
    } else if platform == "darwin" {
        format!("sing-box-{}-darwin-{}.tar.gz", version, arch)
    } else {
        format!("sing-box-{}-linux-{}.tar.gz", version, arch)
    }
}

/// 内核下载镜像 URL 列表（纯逻辑）。
pub(crate) fn kernel_download_urls(version: &str, filename: &str) -> Vec<String> {
    vec![
        format!(
            "https://v6.gh-proxy.com/https://github.com/SagerNet/sing-box/releases/download/v{}/{}",
            version, filename
        ),
        format!(
            "https://gh-proxy.com/https://github.com/SagerNet/sing-box/releases/download/v{}/{}",
            version, filename
        ),
        format!(
            "https://ghfast.top/https://github.com/SagerNet/sing-box/releases/download/v{}/{}",
            version, filename
        ),
        format!(
            "https://hub.fastgit.xyz/SagerNet/sing-box/releases/download/v{}/{}",
            version, filename
        ),
        format!(
            "https://hub.fgit.cf/SagerNet/sing-box/releases/download/v{}/{}",
            version, filename
        ),
        format!(
            "https://cdn.jsdelivr.net/gh/SagerNet/sing-box@releases/download/v{}/{}",
            version, filename
        ),
        format!(
            "https://github.com/SagerNet/sing-box/releases/download/v{}/{}",
            version, filename
        ),
    ]
}

/// 下载源显示名（日志/进度用）。
pub(crate) fn kernel_download_source_name(index: usize) -> &'static str {
    match index {
        0 => "v6.gh-proxy 镜像",
        1 => "gh-proxy 镜像",
        2 => "ghfast.top 加速",
        3 => "hub.fastgit.xyz",
        4 => "hub.fgit.cf",
        5 => "jsdelivr CDN",
        6 => "GitHub 原始",
        _ => "未知源",
    }
}

/// 清理临时目录内容（存在则尽量清空）。
pub(crate) fn clear_dir_contents(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Err(e) = if entry.path().is_dir() {
                std::fs::remove_dir_all(entry.path())
            } else {
                std::fs::remove_file(entry.path())
            } {
                warn!("清理临时目录失败: {}", e);
            }
        }
    }
}

/// 内核下载目录布局：kernel_dir / update_temp / download_path（纯 FS 准备，无网络）。
pub(crate) fn prepare_kernel_download_layout(
    work_dir: &Path,
    filename: &str,
) -> Result<(std::path::PathBuf, std::path::PathBuf, std::path::PathBuf), String> {
    let kernel_dir = work_dir.join("sing-box");
    let temp_update_dir = kernel_dir.join("update_temp");
    std::fs::create_dir_all(&temp_update_dir)
        .map_err(|e| format!("创建临时更新目录失败: {}", e))?;
    clear_dir_contents(&temp_update_dir);
    let download_path = temp_update_dir.join(filename);
    Ok((kernel_dir, temp_update_dir, download_path))
}

/// 从已下载的压缩包解压并部署内核（无窗口/无 AppHandle）。
pub(crate) async fn install_kernel_from_archive(
    archive_path: &Path,
    extract_dir: &Path,
    kernel_dir: &Path,
) -> Result<std::path::PathBuf, String> {
    if !archive_path.exists() {
        return Err("下载的文件不存在".to_string());
    }
    extract_archive(archive_path, extract_dir)
        .await
        .map_err(|e| format!("解压文件失败: {}", e))?;
    let _ = std::fs::remove_file(archive_path);
    let executable_name = kernel_executable_name();
    deploy_kernel_from_extract_dir(extract_dir, kernel_dir, executable_name).await
}

/// 下载进度事件负载（纯逻辑）。
pub(crate) fn build_kernel_download_progress_payload(
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

#[tauri::command]
pub async fn download_kernel(app_handle: AppHandle, version: Option<String>) -> Result<(), String> {
    info!("开始下载内核 (指定版本: {:?})...", version);

    let window = app_handle
        .get_webview_window("main")
        .ok_or("无法获取主窗口")?;

    let _ = window.emit(
        "kernel-download-progress",
        json!({
            "status": "downloading",
            "progress": 0,
            "message": "开始下载内核..."
        }),
    );

    let platform = kernel_platform_name()?;
    let arch = get_system_arch();

    info!("检测到平台: {}, 架构: {}", platform, arch);

    let latest = get_latest_kernel_version()
        .await
        .map_err(|e| e.to_string());
    if let Ok(ref v) = latest {
        info!("获取到最新版本号: {}", v);
    } else if let Err(ref e) = latest {
        warn!("获取最新版本失败: {}, 使用默认版本 1.12.10", e);
    }
    let version = resolve_kernel_version_to_download(version, latest, "1.12.10");

    let filename = kernel_release_filename(&version, platform, arch);
    let download_urls = kernel_download_urls(&version, &filename);

    info!("内核版本: {}", version);
    info!("平台: {}, 架构: {}", platform, arch);
    info!("文件名: {}", filename);
    info!("主要下载 URL (v6.gh-proxy 加速): {}", download_urls[0]);
    info!("备用下载源 1 (gh-proxy): {}", download_urls[1]);
    info!("备用下载源 2 (ghfast.top): {}", download_urls[2]);
    info!("备用下载源 3 (hub.fastgit.xyz): {}", download_urls[3]);
    info!("备用下载源 4 (hub.fgit.cf): {}", download_urls[4]);
    info!("备用下载源 5 (jsdelivr CDN): {}", download_urls[5]);
    info!("备用下载源 6 (GitHub 原始): {}", download_urls[6]);
    info!("总共 {} 个下载源", download_urls.len());

    let work_dir = crate::utils::app_util::get_work_dir_sync();
    let (kernel_dir, temp_update_dir, download_path) =
        prepare_kernel_download_layout(Path::new(&work_dir), &filename)?;

    let _ = window.emit(
        "kernel-download-progress",
        build_kernel_download_progress_payload("downloading", 10, "正在下载内核文件..."),
    );

    // 先尝试无窗口多源下载（覆盖主路径逻辑）；失败时保留原窗口进度语义再逐源尝试
    match try_download_from_urls(&download_urls, &download_path).await {
        Ok(index) => {
            info!(
                "下载成功，使用下载源 #{}: {}",
                index + 1,
                download_urls.get(index).map(|s| s.as_str()).unwrap_or("")
            );
            let _ = window.emit(
                "kernel-download-progress",
                build_kernel_download_progress_payload(
                    "downloading",
                    download_source_progress(index),
                    format!("下载成功: {}", kernel_download_source_name(index)),
                ),
            );
        }
        Err(final_error) => {
            // 回退：带窗口进度的逐源下载（保持历史 emit 行为）
            let mut recovered = false;
            for (index, download_url) in download_urls.iter().enumerate() {
                info!("尝试第 {} 个下载源: {}", index + 1, download_url);
                let _ = window.emit(
                    "kernel-download-progress",
                    build_kernel_download_progress_payload(
                        "downloading",
                        download_source_progress(index),
                        format!("尝试第 {} 个下载源...", index + 1),
                    ),
                );
                match download_file(download_url, &download_path, &window).await {
                    Ok(_) => {
                        info!("下载成功，使用下载源: {}", download_url);
                        recovered = true;
                        break;
                    }
                    Err(e) => {
                        let source_name = kernel_download_source_name(index);
                        warn!("下载源 {} 失败: {}", source_name, e);
                        let _ = window.emit(
                            "kernel-download-progress",
                            build_kernel_download_progress_payload(
                                "downloading",
                                download_source_progress(index),
                                format!("?? {} 失败 - 尝试下一个下载源...", source_name),
                            ),
                        );
                        let _ = std::fs::remove_file(&download_path);
                    }
                }
            }
            if !recovered {
                let _ = window.emit(
                    "kernel-download-progress",
                    build_kernel_download_progress_payload("error", 0, final_error.clone()),
                );
                let _ = std::fs::remove_dir_all(&temp_update_dir);
                return Err(final_error);
            }
        }
    }

    if !download_path.exists() {
        let _ = std::fs::remove_dir_all(&temp_update_dir);
        return Err("下载的文件不存在".to_string());
    }

    let was_running_before_update = is_kernel_running().await.unwrap_or(false);
    if was_running_before_update {
        info!("内核更新前检测到正在运行，先尝试停止以便替换");

        // 尝试多次停止内核
        for i in 0..5 {
            let _ = stop_kernel(Some(&app_handle)).await; // stop_kernel 内部已有 guard disable 和 2s 等待

            if !is_kernel_running().await.unwrap_or(true) {
                info!("内核已成功停止");
                break;
            }
            warn!("停止内核尝试 {} 失败，等待重试...", i + 1);
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // 最后再次确认
        if is_kernel_running().await.unwrap_or(false) {
            warn!("几次尝试后内核仍在运行，尝试强制终止进程...");
            if let Err(e) = PROCESS_MANAGER
                .kill_existing_processes(Some(&app_handle))
                .await
            {
                warn!("强制终止内核进程失败: {}", e);
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    let _ = window.emit(
        "kernel-download-progress",
        build_kernel_download_progress_payload("extracting", 80, "正在解压内核文件..."),
    );

    // 解压并部署到内核目录（与 install_kernel_from_archive 同一路径）
    let target_executable_path =
        match install_kernel_from_archive(&download_path, &temp_update_dir, &kernel_dir).await {
            Ok(p) => p,
            Err(e) => {
                let _ = window.emit(
                    "kernel-download-progress",
                    build_kernel_download_progress_payload("error", 0, e.clone()),
                );
                let _ = std::fs::remove_dir_all(&temp_update_dir);
                return Err(e);
            }
        };

    info!("内核文件已准备就绪: {:?}", target_executable_path);
    info!("内核下载并解压完成: {:?}", target_executable_path);

    let _ = window.emit(
        "kernel-download-progress",
        build_kernel_download_progress_payload("completed", 100, "内核下载完成！"),
    );

    if was_running_before_update {
        info!("内核更新完成，自动重新启动内核");
        let options = RuntimeApplyOptions::new("kernel-update").force_restart(true);
        if let Err(error) =
            apply_runtime_change(&app_handle, RuntimeChange::KernelUpdated, options).await
        {
            warn!("内核更新后自动重启失败: {}", error);
        }
    }

    // 更新安装版本信息到数据库
    use crate::app::storage::enhanced_storage_service::{
        db_get_app_config, db_save_app_config_internal,
    };
    if let Ok(mut config) = db_get_app_config(app_handle.clone()).await {
        config.installed_kernel_version = Some(version);
        if let Err(e) = db_save_app_config_internal(config, &app_handle).await {
            warn!("保存内核版本信息失败: {}", e);
        } else {
            info!("已更新数据库中的已安装内核版本信息");
        }
    }

    Ok(())
}

/// 下载进度 0-70（与历史 UI 语义一致）。
pub(crate) fn download_progress_percent(downloaded: u64, total_size: u64) -> u64 {
    if total_size == 0 {
        return 0;
    }
    total_size
        .checked_div(100)
        .and_then(|scaled_total| downloaded.checked_div(scaled_total.max(1)))
        .unwrap_or(0)
        .min(70)
}

/// 尝试第 N 个源时的 UI 进度值。
pub(crate) fn download_source_progress(index: usize) -> u64 {
    15 + (index as u64 * 5)
}

/// 解析最终版本号：优先用户指定，其次远端 latest，失败回退默认。
pub(crate) fn resolve_kernel_version_to_download(
    requested: Option<String>,
    latest: Result<String, String>,
    default_version: &str,
) -> String {
    match requested {
        Some(v) if !v.trim().is_empty() => v,
        _ => match latest {
            Ok(v) if !v.trim().is_empty() => v,
            _ => default_version.to_string(),
        },
    }
}

/// 所有镜像失败时的错误文案。
pub(crate) fn all_download_sources_failed_message(last_source_name: &str) -> String {
    format!(
        "所有下载源都已失败。最后尝试的 {} 也失败了。请检查网络连接或稍后重试。",
        last_source_name
    )
}

/// 按镜像列表依次下载到 path；成功返回 Ok(index)，全失败返回 Err。
pub(crate) async fn try_download_from_urls(
    urls: &[String],
    path: &Path,
) -> Result<usize, String> {
    if urls.is_empty() {
        return Err("下载源列表为空".to_string());
    }
    let mut last_err = String::new();
    for (index, url) in urls.iter().enumerate() {
        match download_file_to_path(url, path).await {
            Ok(()) => return Ok(index),
            Err(e) => {
                last_err = format!("{}: {}", kernel_download_source_name(index), e);
                let _ = std::fs::remove_file(path);
                if index + 1 < urls.len() {
                    continue;
                }
            }
        }
    }
    Err(all_download_sources_failed_message(
        kernel_download_source_name(urls.len().saturating_sub(1)),
    ) + &format!(" ({})", last_err))
}

/// 无窗口下载+解压部署流水线（hermetic：可注入 URL，不依赖 WebviewWindow）。
/// 返回部署后的可执行文件路径。
#[allow(dead_code)]
pub(crate) async fn download_and_install_kernel_from_urls(
    urls: &[String],
    work_dir: &Path,
    filename: &str,
) -> Result<std::path::PathBuf, String> {
    let (kernel_dir, temp_update_dir, download_path) =
        prepare_kernel_download_layout(work_dir, filename)?;

    try_download_from_urls(urls, &download_path).await?;

    if !download_path.exists() {
        let _ = std::fs::remove_dir_all(&temp_update_dir);
        return Err("下载的文件不存在".to_string());
    }

    match install_kernel_from_archive(&download_path, &temp_update_dir, &kernel_dir).await {
        Ok(path) => Ok(path),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&temp_update_dir);
            Err(e)
        }
    }
}

/// 是否应在替换内核前先停进程（纯逻辑）。
#[allow(dead_code)]
pub(crate) fn should_stop_kernel_before_replace(is_running: bool) -> bool {
    is_running
}

/// 停核重试是否已成功（纯逻辑）。
#[allow(dead_code)]
pub(crate) fn kernel_stop_retry_succeeded(still_running: bool) -> bool {
    !still_running
}

/// 替换前停核最大尝试次数（纯常量，便于测试与生产一致）。
#[allow(dead_code)]
pub(crate) const KERNEL_REPLACE_STOP_ATTEMPTS: u32 = 5;

/// 是否还应继续停核重试（纯逻辑）。
#[allow(dead_code)]
pub(crate) fn should_retry_kernel_stop(attempt_index: u32, still_running: bool) -> bool {
    still_running && attempt_index + 1 < KERNEL_REPLACE_STOP_ATTEMPTS
}

/// 停核重试全部结束后是否应强制 kill（纯逻辑）。
#[allow(dead_code)]
pub(crate) fn should_force_kill_after_stop_retries(still_running: bool) -> bool {
    still_running
}

/// 将安装版本写回 app_config（纯逻辑，无 IO）。
#[allow(dead_code)]
pub(crate) fn apply_installed_kernel_version(
    mut config: crate::app::storage::state_model::AppConfig,
    version: String,
) -> crate::app::storage::state_model::AppConfig {
    config.installed_kernel_version = Some(version);
    config
}

/// 无窗口：可选停核后从本地归档安装内核（hermetic，process 可注入）。
/// `was_running` 仅用于决定返回后是否需要调用方重启；本函数不重启。
#[allow(dead_code)]
pub(crate) async fn install_local_kernel_archive_with_optional_stop_with_process<R: tauri::Runtime>(
    process: &dyn KernelProcessControl<R>,
    app_handle: &tauri::AppHandle<R>,
    archive_path: &Path,
    work_dir: &Path,
) -> Result<(std::path::PathBuf, bool), String> {
    let filename = archive_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("kernel-archive.bin");
    let (kernel_dir, temp_update_dir, staged_download) =
        prepare_kernel_download_layout(work_dir, filename)?;
    // 复用已下载归档：复制到 layout 期望路径
    tokio::fs::copy(archive_path, &staged_download)
        .await
        .map_err(|e| format!("复制归档失败: {}", e))?;

    let was_running = is_kernel_running_with_process(process).await.unwrap_or(false);
    if should_stop_kernel_before_replace(was_running) {
        for i in 0..KERNEL_REPLACE_STOP_ATTEMPTS {
            let _ = stop_kernel_with_process(process, Some(app_handle)).await;
            let still = is_kernel_running_with_process(process).await.unwrap_or(true);
            if kernel_stop_retry_succeeded(still) {
                break;
            }
            if !should_retry_kernel_stop(i, still) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if should_force_kill_after_stop_retries(
            is_kernel_running_with_process(process).await.unwrap_or(false),
        ) {
            let _ = process.kill_existing_processes(Some(app_handle)).await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    let target = install_kernel_from_archive(&staged_download, &temp_update_dir, &kernel_dir).await?;
    Ok((target, was_running))
}

/// 无窗口：可选停核后从本地归档安装内核（生产入口）。
/// `was_running` 仅用于决定返回后是否需要调用方重启；本函数不重启。
#[allow(dead_code)]
pub(crate) async fn install_local_kernel_archive_with_optional_stop<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    archive_path: &Path,
    work_dir: &Path,
) -> Result<(std::path::PathBuf, bool), String> {
    install_local_kernel_archive_with_optional_stop_with_process(
        PROCESS_MANAGER.as_ref(),
        app_handle,
        archive_path,
        work_dir,
    )
    .await
}

/// 清理内核目录下残留的版本目录（sing-box-*）。
pub(crate) fn cleanup_legacy_version_dirs(kernel_dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(kernel_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir()
                && path.file_name().unwrap_or_default() != "logs"
                && path.file_name().unwrap_or_default() != "update_temp"
            {
                let name = path.file_name().unwrap().to_string_lossy();
                if name.starts_with("sing-box-") {
                    let _ = std::fs::remove_dir_all(&path);
                }
            }
        }
    }
}

/// 纯下载（无窗口）：写入 path。
pub(crate) async fn download_file_to_path(
    url: &str,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use futures_util::StreamExt;
    use tokio::fs::File;
    use tokio::io::AsyncWriteExt;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .user_agent("sing-box-windows/1.8.2")
        .build()?;

    info!("开始下载: {}", url);
    let response = client.get(url).send().await?;

    if !response.status().is_success() {
        return Err(format!("HTTP 错误: {}", response.status()).into());
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut downloaded = 0u64;
    let mut file = File::create(path).await?;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        let _ = download_progress_percent(downloaded, total_size);
    }

    file.flush().await?;
    Ok(())
}

async fn download_file(
    url: &str,
    path: &Path,
    window: &WebviewWindow,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use futures_util::StreamExt;
    use tokio::fs::File;
    use tokio::io::AsyncWriteExt;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .user_agent("sing-box-windows/1.8.2")
        .build()?;

    info!("开始下载: {}", url);
    let response = client.get(url).send().await?;

    if !response.status().is_success() {
        return Err(format!("HTTP 错误: {}", response.status()).into());
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut downloaded = 0u64;
    let mut file = File::create(path).await?;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        let progress = download_progress_percent(downloaded, total_size);
        let _ = window.emit(
            "kernel-download-progress",
            json!({
                "status": "downloading",
                "progress": progress,
                "message": format!("下载中... {}/{} bytes", downloaded, total_size)
            }),
        );
    }

    file.flush().await?;
    Ok(())
}

/// 当前平台内核可执行文件名。
pub(crate) fn kernel_executable_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "sing-box.exe"
    } else {
        "sing-box"
    }
}

/// 从解压目录定位可执行文件并部署到 kernel_dir（纯 FS，无 AppHandle）。
pub(crate) async fn deploy_kernel_from_extract_dir(
    extract_dir: &Path,
    kernel_dir: &Path,
    executable_name: &str,
) -> Result<std::path::PathBuf, String> {
    info!("开始在临时目录中查找新内核: {}", executable_name);

    let found_executable_path = find_executable_file(extract_dir, executable_name).await?;
    let target_executable_path = kernel_dir.join(executable_name);

    info!(
        "准备迁移新内核文件从 {:?} 到 {:?}",
        found_executable_path, target_executable_path
    );

    if !kernel_dir.exists() {
        std::fs::create_dir_all(kernel_dir).map_err(|e| format!("创建内核目录失败: {}", e))?;
    }

    // 目标如果存在，处理备份/重命名
    if target_executable_path.exists() {
        let old_executable_path = if cfg!(target_os = "windows") {
            target_executable_path.with_extension("exe.old")
        } else {
            target_executable_path.with_extension("old")
        };

        if old_executable_path.exists() {
            let _ = std::fs::remove_file(&old_executable_path);
        }

        if let Err(e) = std::fs::rename(&target_executable_path, &old_executable_path) {
            warn!("重命名旧文件失败: {}, 尝试直接删除...", e);
            if let Err(e) = std::fs::remove_file(&target_executable_path) {
                return Err(format!(
                    "无法删除或重命名旧内核文件 (可能正在使用?): {}. 请尝试手动停止内核或重启应用。",
                    e
                ));
            }
        } else {
            info!("旧内核文件已重命名为: {:?}", old_executable_path);
        }
    }

    // 移动新文件到目标位置
    if let Err(_e) = std::fs::rename(&found_executable_path, &target_executable_path) {
        if let Err(copy_err) = std::fs::copy(&found_executable_path, &target_executable_path) {
            return Err(format!("复制新内核文件失败: {}", copy_err));
        }
    }

    info!("成功部署新内核文件");

    // 清理临时解压目录
    if let Err(e) = std::fs::remove_dir_all(extract_dir) {
        warn!("清理临时更新目录失败: {}, 请手动清理 {:?}", e, extract_dir);
    }

    // 清理残留的旧版本目录
    cleanup_legacy_version_dirs(kernel_dir);

    if cfg!(target_os = "linux") || cfg!(target_os = "macos") {
        if let Err(e) = set_executable_permission(&target_executable_path) {
            warn!("设置执行权限失败: {}, 将继续...", e);
        }
    }

    Ok(target_executable_path)
}

pub(crate) async fn extract_archive(
    archive_path: &Path,
    extract_to: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("开始解压文件: {:?}", archive_path);

    if !archive_path.exists() {
        return Err(format!("压缩文件不存在: {:?}", archive_path).into());
    }

    let metadata = std::fs::metadata(archive_path)?;
    let file_size = metadata.len();
    info!("压缩文件大小: {} bytes", file_size);

    if file_size == 0 {
        return Err("压缩文件为空".into());
    }

    let file_extension = archive_path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if file_extension == "zip" {
        extract_zip_archive(archive_path, extract_to).await?;
    } else if file_extension == "gz" || archive_path.to_string_lossy().ends_with(".tar.gz") {
        extract_tar_gz_archive(archive_path, extract_to).await?;
    } else {
        return Err(format!("不支持的压缩格式: {}", file_extension).into());
    }

    if let Ok(entries) = std::fs::read_dir(extract_to) {
        info!("解压后的文件:");
        for entry in entries.flatten() {
            info!("  - {:?}", entry.path());
        }
    }

    Ok(())
}

pub(crate) async fn extract_zip_archive(
    archive_path: &Path,
    extract_to: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use zip::ZipArchive;

    info!("解压 ZIP 文件: {:?}", archive_path);

    let file = std::fs::File::open(archive_path)?;
    let mut zip = ZipArchive::new(file)?;

    if !extract_to.exists() {
        std::fs::create_dir_all(extract_to)?;
    }

    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let file_path = extract_to.join(file.name());

        if file.name().ends_with('/') {
            if let Some(parent) = file_path.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            continue;
        }

        if let Some(parent) = file_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let mut output_file = std::fs::File::create(&file_path)?;
        std::io::copy(&mut file, &mut output_file)?;
    }

    info!("ZIP 文件解压完成");
    Ok(())
}

pub(crate) async fn extract_tar_gz_archive(
    archive_path: &Path,
    extract_to: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use flate2::read::GzDecoder;
    use std::fs::File;
    use tar::Archive;

    info!("解压 TAR.GZ 文件: {:?}", archive_path);

    let file = File::open(archive_path)?;
    let gz = GzDecoder::new(file);
    let mut archive = Archive::new(gz);

    if !extract_to.exists() {
        std::fs::create_dir_all(extract_to)?;
    }

    match archive.unpack(extract_to) {
        Ok(_) => info!("TAR.GZ 文件解压完成"),
        Err(e) => return Err(format!("TAR.GZ 解压失败: {}", e).into()),
    }

    Ok(())
}

pub(crate) async fn find_executable_file(
    search_dir: &Path,
    executable_name: &str,
) -> Result<std::path::PathBuf, String> {
    info!(
        "在目录 {:?} 中查找可执行文件: {}",
        search_dir, executable_name
    );

    let direct_path = search_dir.join(executable_name);
    if direct_path.exists() && direct_path.is_file() {
        info!("直接找到可执行文件: {:?}", direct_path);
        return Ok(direct_path);
    }

    let mut found_files = Vec::new();
    for entry in walkdir::WalkDir::new(search_dir).into_iter().flatten() {
        let path = entry.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name == executable_name)
            .unwrap_or(false)
            && path.is_file()
        {
            found_files.push(path.to_path_buf());
        }
    }

    if found_files.is_empty() {
        if let Ok(entries) = std::fs::read_dir(search_dir) {
            warn!("未找到可执行文件，目录内容:");
            for entry in entries.flatten() {
                warn!("  - {:?}", entry.path());
            }
        }
        return Err(format!(
            "未找到可执行文件: {} 在目录 {:?} 中",
            executable_name, search_dir
        ));
    }

    Ok(found_files[0].clone())
}

#[cfg(unix)]
pub(crate) fn set_executable_permission(file_path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;

    let mut perms = std::fs::metadata(file_path)?.permissions();
    perms.set_mode(perms.mode() | 0o755);
    std::fs::set_permissions(file_path, perms)?;

    info!("已设置执行权限: {:?}", file_path);
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn set_executable_permission(_file_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(test)]
#[path = "download.tests.rs"]
mod tests;
