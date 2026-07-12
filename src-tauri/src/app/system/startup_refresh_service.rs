use std::time::Duration;

#[cfg(test)]
use tauri::Manager;
use tauri::{AppHandle, Emitter, Runtime};
use tracing::{info, warn};

use crate::app::network::subscription_service::{
    add_manual_subscription_core, download_subscription_core,
};
use crate::app::storage::enhanced_storage_service::{
    db_get_app_config, db_get_subscriptions, get_enhanced_storage,
};
use crate::app::storage::state_model::{AppConfig, Subscription};

const LAST_LAUNCHED_APP_VERSION_KEY: &str = "last_launched_app_version";
const RETRY_DELAYS_SECONDS: &[u64] = &[30, 120, 600];
const IMMEDIATE_TIMEOUT_SECONDS: u64 = 10;
const RETRY_TIMEOUT_SECONDS: u64 = 30;
const FAILURE_EVENT: &str = "upgrade-subscription-refresh-failed";

/// 测试可压低立即刷新超时（秒）；生产默认 10。
#[cfg(feature = "test-util")]
static IMMEDIATE_TIMEOUT_OVERRIDE_SECS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[cfg(feature = "test-util")]
#[allow(dead_code)]
pub(crate) fn set_immediate_timeout_secs_for_tests(secs: u64) {
    IMMEDIATE_TIMEOUT_OVERRIDE_SECS.store(secs, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(feature = "test-util")]
#[allow(dead_code)]
pub(crate) fn reset_immediate_timeout_secs_for_tests() {
    IMMEDIATE_TIMEOUT_OVERRIDE_SECS.store(0, std::sync::atomic::Ordering::Relaxed);
}

fn immediate_timeout_duration() -> Duration {
    #[cfg(feature = "test-util")]
    {
        let override_secs =
            IMMEDIATE_TIMEOUT_OVERRIDE_SECS.load(std::sync::atomic::Ordering::Relaxed);
        if override_secs > 0 {
            return Duration::from_secs(override_secs);
        }
    }
    Duration::from_secs(IMMEDIATE_TIMEOUT_SECONDS)
}

#[derive(Debug, Clone, serde::Serialize)]
struct UpgradeRefreshFailedPayload {
    message: String,
    version: String,
    active_config_path: String,
    attempts: usize,
    last_error: String,
}

/// 判断是否需要在版本变化时触发订阅刷新（纯逻辑）。
pub(crate) fn should_refresh_on_version_change(
    current_version: &str,
    last_launched_version: Option<&str>,
) -> bool {
    last_launched_version != Some(current_version)
}

/// 根据订阅列表与活动配置路径找到可刷新的订阅（纯逻辑）。
#[allow(dead_code)]
pub(crate) fn find_subscription_for_active_path<'a>(
    subscriptions: &'a [Subscription],
    active_path: &str,
) -> Option<&'a Subscription> {
    subscriptions
        .iter()
        .find(|sub| sub.config_path.as_deref() == Some(active_path))
}

pub async fn start_upgrade_subscription_refresh(app: &AppHandle) {
    if let Err(e) = run_upgrade_subscription_refresh(app).await {
        warn!("启动升级后订阅刷新流程失败: {}", e);
    }
}

/// 可注入版本号的升级后刷新入口（MockRuntime 可测完整下载/手动路径）。
pub(crate) async fn run_upgrade_subscription_refresh_with_version<R: Runtime + 'static>(
    app: &AppHandle<R>,
    current_version: &str,
) -> Result<(), String> {
    let last_launched_version = load_last_launched_version(app).await?;

    if !should_refresh_on_version_change(current_version, last_launched_version.as_deref()) {
        info!("应用版本未变化，跳过升级后订阅刷新: {}", current_version);
        return Ok(());
    }

    let Some(active_config_path) = resolve_active_subscription_config_path(app).await? else {
        // 没有可刷新的活动订阅时，记录版本避免无意义重复执行
        save_last_launched_version(app, current_version).await?;
        info!("当前无可刷新的活动订阅，已记录版本: {}", current_version);
        return Ok(());
    };

    info!(
        "检测到应用版本变化，将尝试刷新当前活动订阅: {:?} ({} -> {})",
        active_config_path,
        last_launched_version.unwrap_or_else(|| "unknown".to_string()),
        current_version
    );

    let immediate_result = tokio::time::timeout(
        immediate_timeout_duration(),
        refresh_subscription_by_config_path(app, &active_config_path, false),
    )
    .await;

    match immediate_result {
        Ok(Ok(())) => {
            info!("升级后首次订阅刷新成功: {}", active_config_path);
            save_last_launched_version(app, current_version).await?;
            return Ok(());
        }
        Ok(Err(e)) => {
            warn!("升级后首次订阅刷新失败，将进入后台重试: {}", e);
            spawn_retry_task(
                app.clone(),
                current_version.to_string(),
                active_config_path,
                e,
            );
        }
        Err(_) => {
            let timeout_error = format!(
                "升级后首次订阅刷新超时（{}s）",
                immediate_timeout_duration().as_secs()
            );
            warn!("{}", timeout_error);
            spawn_retry_task(
                app.clone(),
                current_version.to_string(),
                active_config_path,
                timeout_error,
            );
        }
    }

    Ok(())
}

async fn run_upgrade_subscription_refresh(app: &AppHandle) -> Result<(), String> {
    let current_version = app.package_info().version.to_string();
    run_upgrade_subscription_refresh_with_version(app, &current_version).await
}

fn spawn_retry_task<R: Runtime + 'static>(
    app: AppHandle<R>,
    current_version: String,
    active_config_path: String,
    initial_error: String,
) {
    tauri::async_runtime::spawn(async move {
        let mut last_error = initial_error;

        for (idx, delay_secs) in RETRY_DELAYS_SECONDS.iter().enumerate() {
            tokio::time::sleep(Duration::from_secs(*delay_secs)).await;
            info!(
                "升级后订阅后台重试第 {} 次（{}s 后触发）: {}",
                idx + 1,
                delay_secs,
                active_config_path
            );

            let retry_result = tokio::time::timeout(
                Duration::from_secs(RETRY_TIMEOUT_SECONDS),
                refresh_subscription_by_config_path(&app, &active_config_path, true),
            )
            .await;

            match retry_result {
                Ok(Ok(())) => {
                    info!(
                        "升级后订阅后台重试成功（第 {} 次）: {}",
                        idx + 1,
                        active_config_path
                    );
                    if let Err(e) = save_last_launched_version(&app, &current_version).await {
                        warn!("写入 last_launched_app_version 失败: {}", e);
                    }
                    return;
                }
                Ok(Err(e)) => {
                    last_error = e;
                    warn!(
                        "升级后订阅后台重试失败（第 {} 次）: {}",
                        idx + 1,
                        last_error
                    );
                }
                Err(_) => {
                    last_error = format!("后台重试超时（{}s）", RETRY_TIMEOUT_SECONDS);
                    warn!(
                        "升级后订阅后台重试超时（第 {} 次）: {}",
                        idx + 1,
                        active_config_path
                    );
                }
            }
        }

        let message =
            "应用升级后已多次尝试刷新当前订阅但仍失败，请手动在订阅页执行“立即更新配置”。";
        let payload = UpgradeRefreshFailedPayload {
            message: message.to_string(),
            version: current_version,
            active_config_path,
            attempts: RETRY_DELAYS_SECONDS.len() + 1,
            last_error,
        };

        if let Err(e) = app.emit(FAILURE_EVENT, &payload) {
            warn!("发送升级后订阅刷新失败事件失败: {}", e);
        }
    });
}

async fn resolve_active_subscription_config_path<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<Option<String>, String> {
    let app_config = db_get_app_config(app.clone()).await?;
    let Some(active_path) = app_config.active_config_path else {
        return Ok(None);
    };

    let subscriptions = db_get_subscriptions(app.clone()).await?;
    let exists = subscriptions
        .iter()
        .any(|sub| sub.config_path.as_deref() == Some(active_path.as_str()));
    if exists {
        Ok(Some(active_path))
    } else {
        Ok(None)
    }
}

/// 按活动配置路径刷新订阅（无 Window，任意 Runtime）。
pub(crate) async fn refresh_subscription_by_config_path<R: Runtime>(
    app: &AppHandle<R>,
    config_path: &str,
    apply_runtime: bool,
) -> Result<(), String> {
    let app_config = db_get_app_config(app.clone()).await?;
    let subscriptions = db_get_subscriptions(app.clone()).await?;
    let Some(subscription) = subscriptions
        .into_iter()
        .find(|sub| sub.config_path.as_deref() == Some(config_path))
    else {
        return Err(format!(
            "活动配置未在订阅列表中找到，跳过自动刷新: {}",
            config_path
        ));
    };

    refresh_subscription(app, &app_config, &subscription, apply_runtime).await
}

/// 刷新单个订阅（无 Window：走 download/manual core）。
pub(crate) async fn refresh_subscription<R: Runtime>(
    app: &AppHandle<R>,
    app_config: &AppConfig,
    sub: &Subscription,
    apply_runtime: bool,
) -> Result<(), String> {
    let file_name = Some(format!("{}.json", sub.name));
    let config_path = sub.config_path.clone();
    let proxy_port = Some(app_config.proxy_port);
    let api_port = Some(app_config.api_port);

    if sub.is_manual {
        let content = sub
            .manual_content
            .clone()
            .ok_or_else(|| format!("手动订阅内容为空: {}", sub.name))?;
        add_manual_subscription_core(
            app,
            content,
            sub.use_original_config,
            file_name,
            config_path,
            apply_runtime,
            proxy_port,
            api_port,
        )
        .await
        .map(|_| ())
    } else {
        let url = sub.url.trim().to_string();
        if url.is_empty() {
            return Err(format!("订阅 URL 为空: {}", sub.name));
        }
        download_subscription_core(
            app,
            url,
            sub.use_original_config,
            file_name,
            config_path,
            apply_runtime,
            proxy_port,
            api_port,
        )
        .await
        .map(|_| ())
    }
}

async fn load_last_launched_version<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<Option<String>, String> {
    let storage = get_enhanced_storage(app).await?;
    storage
        .get_config::<String>(LAST_LAUNCHED_APP_VERSION_KEY)
        .await
        .map_err(|e| format!("读取启动版本标记失败: {}", e))
}

async fn save_last_launched_version<R: Runtime>(
    app: &AppHandle<R>,
    version: &str,
) -> Result<(), String> {
    let storage = get_enhanced_storage(app).await?;
    storage
        .save_config(LAST_LAUNCHED_APP_VERSION_KEY, &version.to_string())
        .await
        .map_err(|e| format!("保存启动版本标记失败: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_refresh_on_version_change_logic() {
        assert!(should_refresh_on_version_change("1.0.0", None));
        assert!(should_refresh_on_version_change("1.0.1", Some("1.0.0")));
        assert!(!should_refresh_on_version_change("1.0.0", Some("1.0.0")));
    }

    #[test]
    fn retry_delays_and_timeouts_are_configured() {
        assert_eq!(RETRY_DELAYS_SECONDS, &[30, 120, 600]);
        assert_eq!(IMMEDIATE_TIMEOUT_SECONDS, 10);
        assert_eq!(RETRY_TIMEOUT_SECONDS, 30);
        assert_eq!(FAILURE_EVENT, "upgrade-subscription-refresh-failed");
        assert_eq!(LAST_LAUNCHED_APP_VERSION_KEY, "last_launched_app_version");
    }

    #[test]
    fn find_subscription_for_active_path_match() {
        use crate::app::storage::state_model::Subscription;
        let subs = vec![
            Subscription {
                name: "a".into(),
                url: "http://a".into(),
                is_loading: false,
                last_update: None,
                is_manual: false,
                manual_content: None,
                use_original_config: false,
                config_path: Some("/tmp/a.json".into()),
                backup_path: None,
                auto_update_interval_minutes: None,
                subscription_upload: None,
                subscription_download: None,
                subscription_total: None,
                subscription_expire: None,
                auto_update_fail_count: None,
                last_auto_update_attempt: None,
                last_auto_update_error: None,
                last_auto_update_error_type: None,
                last_auto_update_backoff_until: None,
            },
            Subscription {
                name: "b".into(),
                url: "http://b".into(),
                is_loading: false,
                last_update: None,
                is_manual: true,
                manual_content: Some("x".into()),
                use_original_config: true,
                config_path: Some("/tmp/b.json".into()),
                backup_path: None,
                auto_update_interval_minutes: Some(60),
                subscription_upload: None,
                subscription_download: None,
                subscription_total: None,
                subscription_expire: None,
                auto_update_fail_count: None,
                last_auto_update_attempt: None,
                last_auto_update_error: None,
                last_auto_update_error_type: None,
                last_auto_update_backoff_until: None,
            },
        ];
        let found = find_subscription_for_active_path(&subs, "/tmp/b.json").unwrap();
        assert_eq!(found.name, "b");
        assert!(find_subscription_for_active_path(&subs, "/tmp/missing.json").is_none());
        assert!(find_subscription_for_active_path(&[], "/tmp/a.json").is_none());
    }

    #[test]
    fn should_refresh_version_edges() {
        assert!(should_refresh_on_version_change("1.0.0", None));
        assert!(should_refresh_on_version_change("1.0.1", Some("1.0.0")));
        assert!(!should_refresh_on_version_change("2.0.0", Some("2.0.0")));
        assert!(should_refresh_on_version_change("", Some("x")));
        assert!(!should_refresh_on_version_change("", Some("")));
    }

    #[test]
    fn find_subscription_for_active_path_match_legacy() {
        let subs = vec![
            Subscription {
                name: "a".into(),
                url: "https://a".into(),
                is_loading: false,
                last_update: None,
                is_manual: false,
                manual_content: None,
                use_original_config: false,
                config_path: Some("configs/a.json".into()),
                backup_path: None,
                auto_update_interval_minutes: None,
                subscription_upload: None,
                subscription_download: None,
                subscription_total: None,
                subscription_expire: None,
                auto_update_fail_count: None,
                last_auto_update_attempt: None,
                last_auto_update_error: None,
                last_auto_update_error_type: None,
                last_auto_update_backoff_until: None,
            },
            Subscription {
                name: "b".into(),
                url: "https://b".into(),
                is_manual: true,
                is_loading: false,
                last_update: None,
                manual_content: Some("x".into()),
                use_original_config: true,
                config_path: Some("configs/b.json".into()),
                backup_path: None,
                auto_update_interval_minutes: Some(0),
                subscription_upload: None,
                subscription_download: None,
                subscription_total: None,
                subscription_expire: None,
                auto_update_fail_count: None,
                last_auto_update_attempt: None,
                last_auto_update_error: None,
                last_auto_update_error_type: None,
                last_auto_update_backoff_until: None,
            },
        ];
        assert_eq!(
            find_subscription_for_active_path(&subs, "configs/b.json").map(|s| s.name.as_str()),
            Some("b")
        );
        assert!(find_subscription_for_active_path(&subs, "missing").is_none());
    }

    #[tokio::test]
    async fn upgrade_refresh_skips_when_version_unchanged() {
        use crate::test_support::MockAppEnv;

        let env = MockAppEnv::new();
        let db = env.workspace.path().join("sr.db");
        env.install_storage_from_path(db.to_str().unwrap()).await;

        // 首次：无订阅 → 记录版本
        run_upgrade_subscription_refresh_with_version(&env.handle(), "1.0.0")
            .await
            .unwrap();
        // 版本未变 → 跳过
        run_upgrade_subscription_refresh_with_version(&env.handle(), "1.0.0")
            .await
            .unwrap();

        let loaded: Option<String> = env
            .app
            .try_state::<std::sync::Arc<
                tokio::sync::OnceCell<
                    std::sync::Arc<
                        crate::app::storage::enhanced_storage_service::EnhancedStorageService,
                    >,
                >,
            >>()
            .unwrap()
            .get()
            .unwrap()
            .get_config(LAST_LAUNCHED_APP_VERSION_KEY)
            .await
            .unwrap();
        assert_eq!(loaded.as_deref(), Some("1.0.0"));
    }

    #[tokio::test]
    async fn upgrade_refresh_manual_subscription_succeeds_without_window() {
        use crate::app::storage::state_model::{AppConfig, Subscription};
        use crate::test_support::MockAppEnv;

        let env = MockAppEnv::new();
        let db = env.workspace.path().join("sr2.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        let path = env.workspace.path().join("sing-box/c.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{}").unwrap();
        let mut cfg = AppConfig::default();
        cfg.active_config_path = Some(path.to_string_lossy().to_string());
        storage.save_app_config(&cfg).await.unwrap();
        storage
            .save_subscriptions(&[Subscription {
                name: "s".into(),
                url: "https://x".into(),
                is_loading: false,
                last_update: None,
                is_manual: true,
                manual_content: Some("trojan://p@h:1#n".into()),
                use_original_config: false,
                config_path: Some(path.to_string_lossy().to_string()),
                backup_path: None,
                auto_update_interval_minutes: None,
                subscription_upload: None,
                subscription_download: None,
                subscription_total: None,
                subscription_expire: None,
                auto_update_fail_count: None,
                last_auto_update_attempt: None,
                last_auto_update_error: None,
                last_auto_update_error_type: None,
                last_auto_update_backoff_until: None,
            }])
            .await
            .unwrap();

        // 版本变化 + 手动订阅 → 无窗口也可成功刷新并记录版本
        run_upgrade_subscription_refresh_with_version(&env.handle(), "2.0.0")
            .await
            .expect("manual refresh without window");
    }

    #[tokio::test]
    async fn run_upgrade_refresh_version_skip_and_no_subscription() {
        use crate::app::storage::state_model::AppConfig;
        use crate::test_support::MockAppEnv;

        let env = MockAppEnv::new();
        let db = env.workspace.path().join("refresh.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        storage
            .save_app_config(&AppConfig::default())
            .await
            .unwrap();
        let h = env.handle();

        // 首次：版本变化但无活动订阅 → 记录版本并 Ok
        run_upgrade_subscription_refresh_with_version(&h, "9.9.9")
            .await
            .expect("no-sub path");
        // 再次同版本 → skip
        run_upgrade_subscription_refresh_with_version(&h, "9.9.9")
            .await
            .expect("same version skip");
        // 新版本仍无活动订阅
        run_upgrade_subscription_refresh_with_version(&h, "9.9.10")
            .await
            .expect("new version no sub");
    }

    #[tokio::test]
    async fn run_upgrade_refresh_with_remote_sub_local_http() {
        use crate::app::storage::state_model::{AppConfig, Subscription};
        use crate::test_support::MockAppEnv;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let env = MockAppEnv::new();
        let db = env.workspace.path().join("refresh2.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        let cfg_path = env.workspace.path().join("configs/act.json");
        std::fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
        std::fs::write(&cfg_path, b"{}").unwrap();

        // 本地 mock 订阅内容
        let body = b"ss://YWVzLTI1Ni1nY206cGFzcw@127.0.0.1:8388#node1\n";
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((mut s, _)) = listener.accept().await {
                let mut buf = [0u8; 512];
                let _ = s.read(&mut buf).await;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
                let _ = s.write_all(body).await;
            }
        });

        let mut cfg = AppConfig::default();
        cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
        storage.save_app_config(&cfg).await.unwrap();
        let sub = Subscription {
            name: "act".into(),
            url: format!("http://127.0.0.1:{}/sub", port),
            is_loading: false,
            last_update: None,
            is_manual: false,
            manual_content: None,
            use_original_config: false,
            config_path: Some(cfg_path.to_string_lossy().to_string()),
            backup_path: None,
            auto_update_interval_minutes: None,
            subscription_upload: None,
            subscription_download: None,
            subscription_total: None,
            subscription_expire: None,
            auto_update_fail_count: None,
            last_auto_update_attempt: None,
            last_auto_update_error: None,
            last_auto_update_error_type: None,
            last_auto_update_backoff_until: None,
        };
        storage.save_subscriptions(&[sub]).await.unwrap();
        let h = env.handle();
        let _ = storage.remove_config(LAST_LAUNCHED_APP_VERSION_KEY).await;
        run_upgrade_subscription_refresh_with_version(&h, "10.0.0")
            .await
            .expect("remote sub refresh via local http");
    }

    #[tokio::test]
    async fn refresh_subscription_by_path_missing_errors() {
        use crate::app::storage::state_model::AppConfig;
        use crate::test_support::MockAppEnv;

        let env = MockAppEnv::new();
        let db = env.workspace.path().join("missing-sub.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        storage
            .save_app_config(&AppConfig::default())
            .await
            .unwrap();
        storage.save_subscriptions(&[]).await.unwrap();
        let err = refresh_subscription_by_config_path(&env.handle(), "/no/such.json", false)
            .await
            .unwrap_err();
        assert!(err.contains("未在订阅列表") || err.contains("跳过"));
    }

    #[tokio::test]
    async fn refresh_subscription_empty_url_and_empty_manual() {
        use crate::app::storage::state_model::{AppConfig, Subscription};
        use crate::test_support::MockAppEnv;

        let env = MockAppEnv::new();
        let db = env.workspace.path().join("empty.db");
        env.install_storage_from_path(db.to_str().unwrap()).await;
        let h = env.handle();
        let cfg = AppConfig::default();
        let empty_url = Subscription {
            name: "u".into(),
            url: "  ".into(),
            is_loading: false,
            last_update: None,
            is_manual: false,
            manual_content: None,
            use_original_config: false,
            config_path: Some("u.json".into()),
            backup_path: None,
            auto_update_interval_minutes: None,
            subscription_upload: None,
            subscription_download: None,
            subscription_total: None,
            subscription_expire: None,
            auto_update_fail_count: None,
            last_auto_update_attempt: None,
            last_auto_update_error: None,
            last_auto_update_error_type: None,
            last_auto_update_backoff_until: None,
        };
        let err = refresh_subscription(&h, &cfg, &empty_url, false)
            .await
            .unwrap_err();
        assert!(err.contains("URL"));

        let empty_manual = Subscription {
            name: "m".into(),
            url: "".into(),
            is_loading: false,
            last_update: None,
            is_manual: true,
            manual_content: None,
            use_original_config: false,
            config_path: Some("m.json".into()),
            backup_path: None,
            auto_update_interval_minutes: None,
            subscription_upload: None,
            subscription_download: None,
            subscription_total: None,
            subscription_expire: None,
            auto_update_fail_count: None,
            last_auto_update_attempt: None,
            last_auto_update_error: None,
            last_auto_update_error_type: None,
            last_auto_update_backoff_until: None,
        };
        let err = refresh_subscription(&h, &cfg, &empty_manual, false)
            .await
            .unwrap_err();
        assert!(err.contains("手动订阅") || err.contains("空"));
    }
}
