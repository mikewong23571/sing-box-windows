use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::sync::OnceCell;
use tracing::{error, info, warn};

use crate::app::core::kernel_service::status::kernel_check_health;
use crate::app::storage::enhanced_storage_service::EnhancedStorageService;
use crate::app::system::update_service::{check_update, UpdateInfo};

const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(4 * 60 * 60); // 4h
const KERNEL_HEALTH_INTERVAL: Duration = Duration::from_secs(10 * 60); // 10min

pub async fn start_background_tasks<R: Runtime>(app: &AppHandle<R>) {
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = start_update_loop(app_handle.clone()).await {
            error!("后台更新检查任务结束，原因: {}", e);
        }
    });

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = start_kernel_health_loop(app_handle.clone()).await {
            error!("后台内核健康检查任务结束，原因: {}", e);
        }
    });
}

/// 等待存储就绪（可注入超时，便于单测）。
pub(crate) async fn wait_for_storage_with_timeout<R: Runtime>(
    app: &AppHandle<R>,
    poll: Duration,
    max_wait: Duration,
) -> Option<Arc<EnhancedStorageService>> {
    let storage_cell = app.state::<Arc<OnceCell<Arc<EnhancedStorageService>>>>();
    let started = std::time::Instant::now();
    loop {
        if let Some(storage) = storage_cell.get() {
            return Some(storage.clone());
        }
        if started.elapsed() >= max_wait {
            return None;
        }
        warn!("存储服务尚未就绪，稍后重试...");
        tokio::time::sleep(poll).await;
    }
}

async fn wait_for_storage<R: Runtime>(app: &AppHandle<R>) -> Option<Arc<EnhancedStorageService>> {
    // 生产：无限等待
    wait_for_storage_with_timeout(
        app,
        Duration::from_secs(1),
        Duration::from_secs(u64::MAX / 4),
    )
    .await
}

async fn start_update_loop<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let version = app.package_info().version.to_string();
    let storage = wait_for_storage(&app).await;

    loop {
        if let Some(storage) = &storage {
            match storage.get_update_config().await {
                Ok(config) => {
                    if !config.auto_check {
                        warn!("自动更新检查已关闭，跳过本轮后台检查");
                    } else {
                        match check_update(
                            version.clone(),
                            Some(config.accept_prerelease),
                            config.update_channel.clone(),
                        )
                        .await
                        {
                            Ok(info) => {
                                handle_update_result(&app, &config.skip_version, info).await
                            }
                            Err(e) => warn!("后台检查更新失败: {}", e),
                        }
                    }
                }
                Err(e) => warn!("读取更新配置失败: {}", e),
            }
        }

        tokio::time::sleep(UPDATE_CHECK_INTERVAL).await;
    }
}

/// 处理更新检查结果（纯分支 + emit，便于单测）。
pub(crate) async fn handle_update_result<R: Runtime>(
    app: &AppHandle<R>,
    skip_version: &Option<String>,
    info: UpdateInfo,
) {
    if info.has_update {
        if skip_version
            .as_ref()
            .map(|v| v == &info.latest_version)
            .unwrap_or(false)
        {
            info!("检测到可更新版本 {}，但用户已选择跳过", info.latest_version);
            return;
        }

        if let Err(e) = app.emit("update-available", &info) {
            error!("发送 update-available 事件失败: {}", e);
        } else {
            info!("后台检测到新版本 {}，已推送事件", info.latest_version);
        }
    } else {
        info!("后台检查：当前已是最新版本");
    }
}

async fn start_kernel_health_loop<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    loop {
        match kernel_check_health(None).await {
            Ok(payload) => {
                if let Err(e) = app.emit("kernel-health", &payload) {
                    error!("发送 kernel-health 事件失败: {}", e);
                }
            }
            Err(e) => warn!("后台内核健康检查失败: {}", e),
        }

        tokio::time::sleep(KERNEL_HEALTH_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::system::update_service::UpdateInfo;
    use crate::test_support::MockAppEnv;

    fn sample_update(has: bool, ver: &str) -> UpdateInfo {
        UpdateInfo {
            latest_version: ver.to_string(),
            download_url: "https://example.com/app.AppImage".into(),
            release_page_url: "https://example.com/releases".into(),
            has_update: has,
            release_notes: Some("notes".into()),
            release_date: Some("2026-01-01".into()),
            file_size: Some(1024),
            is_prerelease: false,
            supports_in_app_update: true,
        }
    }

    #[tokio::test]
    async fn handle_update_result_branches() {
        let env = MockAppEnv::new();
        let h = env.handle();

        // 无更新
        handle_update_result(&h, &None, sample_update(false, "1.0.0")).await;
        // 有更新
        handle_update_result(&h, &None, sample_update(true, "2.0.0")).await;
        // 跳过版本
        handle_update_result(&h, &Some("2.0.0".into()), sample_update(true, "2.0.0")).await;
    }

    #[tokio::test]
    async fn wait_for_storage_timeout_and_ready() {
        let env = MockAppEnv::new();
        // 未 install → 短超时返回 None
        let none = wait_for_storage_with_timeout(
            &env.handle(),
            Duration::from_millis(10),
            Duration::from_millis(40),
        )
        .await;
        assert!(none.is_none());

        let db = env.workspace.path().join("bg.db");
        env.install_storage_from_path(db.to_str().unwrap()).await;
        let some = wait_for_storage_with_timeout(
            &env.handle(),
            Duration::from_millis(10),
            Duration::from_secs(1),
        )
        .await;
        assert!(some.is_some());
    }

    #[tokio::test]
    async fn start_background_tasks_spawns_without_panic() {
        let env = MockAppEnv::new();
        let db = env.workspace.path().join("bg2.db");
        env.install_storage_from_path(db.to_str().unwrap()).await;
        start_background_tasks(&env.handle()).await;
        // 给 spawn 一点时间启动循环
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
