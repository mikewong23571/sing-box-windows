use crate::app::runtime::change::{RuntimeApplyOptions, RuntimeChange};
use crate::app::runtime::orchestrator::apply_runtime_change;
use crate::app::storage::enhanced_storage_service::get_enhanced_storage;
use crate::app::storage::state_model::{AppConfig, Subscription};
use tauri::{AppHandle, Runtime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigPatchMode {
    Full,
    PortsOnly,
}

pub fn resolve_patch_mode_for_subscription(
    subscription: Option<&Subscription>,
) -> ConfigPatchMode {
    match subscription {
        Some(sub) if sub.use_original_config => ConfigPatchMode::PortsOnly,
        _ => ConfigPatchMode::Full,
    }
}

pub fn resolve_patch_mode_with_hint(
    subscription: Option<&Subscription>,
    use_original_config_hint: Option<bool>,
) -> ConfigPatchMode {
    match use_original_config_hint {
        // 显式提示来自当前前端交互，优先级高于数据库里的历史记录，可避免异步落库时的误判。
        Some(true) => ConfigPatchMode::PortsOnly,
        Some(false) => ConfigPatchMode::Full,
        None => resolve_patch_mode_for_subscription(subscription),
    }
}

pub fn sync_settings_to_config_file(
    config_path: &std::path::Path,
    app_config: &AppConfig,
    patch_mode: ConfigPatchMode,
) -> Result<(), String> {
    use crate::app::singbox::settings_patch::{
        apply_app_settings_to_config, apply_port_settings_only,
    };

    let content =
        std::fs::read_to_string(config_path).map_err(|e| format!("读取配置文件失败: {}", e))?;

    let mut config: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("解析配置文件失败: {}", e))?;

    match patch_mode {
        ConfigPatchMode::Full => apply_app_settings_to_config(&mut config, app_config),
        ConfigPatchMode::PortsOnly => apply_port_settings_only(&mut config, app_config),
    }

    let updated =
        serde_json::to_string_pretty(&config).map_err(|e| format!("序列化配置失败: {}", e))?;
    std::fs::write(config_path, updated).map_err(|e| format!("写入配置文件失败: {}", e))?;

    Ok(())
}

async fn resolve_patch_mode_for_active_config<R: Runtime>(
    app: &AppHandle<R>,
    active_config_path: &str,
    use_original_config_hint: Option<bool>,
) -> ConfigPatchMode {
    if use_original_config_hint.is_some() {
        return resolve_patch_mode_with_hint(None, use_original_config_hint);
    }

    let storage = match get_enhanced_storage(app).await {
        Ok(storage) => storage,
        Err(error) => {
            tracing::warn!("读取数据库服务失败，回退使用 Full patch: {}", error);
            return ConfigPatchMode::Full;
        }
    };

    match storage.get_subscriptions().await {
        Ok(subscriptions) => resolve_patch_mode_for_subscription(
            subscriptions
                .iter()
                .find(|sub| sub.config_path.as_deref() == Some(active_config_path)),
        ),
        Err(error) => {
            tracing::warn!("读取订阅列表失败，回退使用 Full patch: {}", error);
            ConfigPatchMode::Full
        }
    }
}

pub(crate) async fn sync_active_config_settings<R: Runtime>(
    app: &AppHandle<R>,
    effective_config: &AppConfig,
    use_original_config_hint: Option<bool>,
) {
    if let Some(path) = effective_config.active_config_path.as_deref() {
        let config_path = std::path::PathBuf::from(path);
        if config_path.exists() {
            let patch_mode =
                resolve_patch_mode_for_active_config(app, path, use_original_config_hint).await;
            if let Err(error) =
                sync_settings_to_config_file(&config_path, effective_config, patch_mode)
            {
                tracing::warn!("同步活动配置文件失败: {}", error);
            }
        }
    }
}

pub async fn apply_runtime_config_update<R: Runtime>(
    app: &AppHandle<R>,
    _effective_config: &AppConfig,
    use_original_config_hint: Option<bool>,
    force_restart: bool,
    reason: &'static str,
) {
    let options = RuntimeApplyOptions::new(reason)
        .patch_active_config(true)
        .force_restart(force_restart)
        .use_original_config_hint(use_original_config_hint);

    if let Err(error) = apply_runtime_change(app, RuntimeChange::AppConfigUpdated, options).await {
        tracing::warn!("应用运行态配置更新失败({}): {}", reason, error);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_runtime_config_update, resolve_patch_mode_for_subscription,
        resolve_patch_mode_with_hint, sync_active_config_settings, sync_settings_to_config_file,
        ConfigPatchMode,
    };
    use crate::app::storage::state_model::{AppConfig, Subscription};
    use crate::test_support::MockAppEnv;
    use std::fs;

    fn build_subscription(use_original_config: bool) -> Subscription {
        Subscription {
            name: "test".to_string(),
            url: "https://example.com/sub".to_string(),
            is_loading: false,
            last_update: None,
            is_manual: false,
            manual_content: None,
            use_original_config,
            config_path: Some("/tmp/sub.json".to_string()),
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
        }
    }

    #[test]
    fn should_use_ports_only_for_original_subscription_without_hint() {
        let subscription = build_subscription(true);
        assert_eq!(
            resolve_patch_mode_for_subscription(Some(&subscription)),
            ConfigPatchMode::PortsOnly
        );
    }

    #[test]
    fn should_trust_explicit_original_hint_when_subscription_missing() {
        assert_eq!(
            resolve_patch_mode_with_hint(None, Some(true)),
            ConfigPatchMode::PortsOnly
        );
    }

    #[test]
    fn should_trust_explicit_non_original_hint_over_subscription_state() {
        let subscription = build_subscription(true);
        assert_eq!(
            resolve_patch_mode_with_hint(Some(&subscription), Some(false)),
            ConfigPatchMode::Full
        );
    }

    #[test]
    fn sync_settings_to_config_file_full_and_ports_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cfg.json");
        fs::write(
            &path,
            r#"{"log":{"level":"info"},"inbounds":[{"type":"mixed","tag":"mixed-in","listen":"127.0.0.1","listen_port":7890}],"outbounds":[{"type":"direct","tag":"direct"}]}"#,
        )
        .unwrap();
        let mut cfg = AppConfig::default();
        cfg.proxy_port = 17990;
        cfg.api_port = 17991;
        sync_settings_to_config_file(&path, &cfg, ConfigPatchMode::PortsOnly).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("17990") || content.contains("mixed"));
        sync_settings_to_config_file(&path, &cfg, ConfigPatchMode::Full).unwrap();
        assert!(path.exists());
        // 缺失文件
        assert!(sync_settings_to_config_file(
            &dir.path().join("missing.json"),
            &cfg,
            ConfigPatchMode::Full
        )
        .is_err());
    }

    #[tokio::test]
    async fn sync_active_config_settings_with_hint_and_subscription() {
        let env = MockAppEnv::new();
        let cfg_path = env.workspace.path().join("sing-box/active.json");
        fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
        fs::write(
            &cfg_path,
            r#"{"log":{"level":"info"},"inbounds":[],"outbounds":[{"type":"direct","tag":"direct"}]}"#,
        )
        .unwrap();
        let db = env.workspace.path().join("cfg_update.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        let mut app_cfg = AppConfig::default();
        app_cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
        app_cfg.proxy_port = 18001;
        storage.save_app_config(&app_cfg).await.unwrap();

        let mut sub = build_subscription(true);
        sub.config_path = Some(cfg_path.to_string_lossy().to_string());
        storage.save_subscriptions(&[sub]).await.unwrap();

        let h = env.handle();
        // 显式 hint
        sync_active_config_settings(&h, &app_cfg, Some(true)).await;
        // 无 hint：从订阅解析 PortsOnly
        sync_active_config_settings(&h, &app_cfg, None).await;
        // 无活动路径：no-op
        let empty = AppConfig::default();
        sync_active_config_settings(&h, &empty, None).await;
    }

    #[tokio::test]
    async fn apply_runtime_config_update_with_mock_app() {
        let env = MockAppEnv::new();
        let cfg_path = env.workspace.path().join("sing-box/c.json");
        fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
        fs::write(
            &cfg_path,
            r#"{"log":{"level":"info"},"inbounds":[],"outbounds":[{"type":"direct","tag":"direct"}]}"#,
        )
        .unwrap();
        let db = env.workspace.path().join("cfg_update2.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        let mut app_cfg = AppConfig::default();
        app_cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
        storage.save_app_config(&app_cfg).await.unwrap();

        apply_runtime_config_update(&env.handle(), &app_cfg, Some(false), false, "test-update")
            .await;
    }
}
