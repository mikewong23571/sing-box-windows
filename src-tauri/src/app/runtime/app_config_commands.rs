use crate::app::runtime::change::{RuntimeApplyOptions, RuntimeChange};
use crate::app::runtime::config_update::{
    resolve_patch_mode_with_hint, sync_settings_to_config_file, ConfigPatchMode,
};
use crate::app::runtime::orchestrator::apply_runtime_change;
use crate::app::storage::enhanced_storage_service::{
    db_save_app_config_internal, get_enhanced_storage,
};
use crate::app::storage::state_model::AppConfig;
use tauri::{AppHandle, Runtime};

/// 仅持久化 AppConfig（无 runtime apply）。Hermetic 单测用。
pub(crate) async fn db_save_app_config_persist_only<R: Runtime>(
    config: AppConfig,
    app: &AppHandle<R>,
) -> Result<(), String> {
    db_save_app_config_internal(config, app).await
}

#[tauri::command]
pub async fn db_save_app_config(
    config: AppConfig,
    app: AppHandle,
    apply_runtime: Option<bool>,
) -> Result<(), String> {
    db_save_app_config_persist_only(config, &app).await?;
    let apply_runtime = apply_runtime.unwrap_or(false);

    // 默认仅持久化，不自动触发运行态变更。
    // 这样可避免前端自动保存（输入框逐字变化）频繁触发内核重启/配置同步。
    if !apply_runtime {
        return Ok(());
    }

    // 保存设置后，尽量把变更同步到“当前生效配置文件”，避免用户需要重新下载订阅/重启应用才能生效。
    // 同步逻辑采用“局部 patch”策略：如果配置文件不是本程序生成的结构，会尽量只修改端口/TUN/DNS 策略等通用字段。
    let options = RuntimeApplyOptions::new("app-config-updated")
        .patch_active_config(true)
        .restart_if_running(true);
    apply_runtime_change(&app, RuntimeChange::AppConfigUpdated, options).await?;

    Ok(())
}

#[tauri::command]
pub async fn db_save_active_subscription_index<R: Runtime>(
    index: Option<i64>,
    app: AppHandle<R>,
) -> Result<(), String> {
    let storage = get_enhanced_storage(&app).await?;

    storage
        .save_active_subscription_index(index)
        .await
        .map_err(|e| e.to_string())?;

    // active_config_path 是内核启动时读取的真实生效配置路径。
    // active_subscription_index 只用于前端高亮/记忆选择位置，不能反向覆盖 active_config_path。
    // 这里保留原有兼容行为：切换高亮订阅时，把全局端口/TUN/DNS 设置同步到该订阅配置文件。
    let app_config = storage.get_app_config().await.map_err(|e| e.to_string())?;

    let (target_config_path, patch_mode) = if let Some(idx) = index {
        let subscriptions = storage
            .get_subscriptions()
            .await
            .map_err(|e| e.to_string())?;
        let subscription = subscriptions.get(idx as usize);
        (
            subscription
                .and_then(|sub| sub.config_path.clone())
                .map(std::path::PathBuf::from),
            resolve_patch_mode_with_hint(subscription, None),
        )
    } else {
        (None, ConfigPatchMode::Full)
    };

    if let Some(config_path) = target_config_path {
        if config_path.exists() {
            match sync_settings_to_config_file(&config_path, &app_config, patch_mode) {
                Ok(_) => {
                    tracing::info!("已将全局设置同步到配置文件: {:?}", config_path);
                }
                Err(e) => {
                    tracing::warn!("同步设置到配置文件失败: {}", e);
                }
            }
        } else {
            tracing::warn!(
                "订阅索引写入时发现配置文件不存在，跳过同步: {:?}",
                config_path
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::singbox::config_generator::generate_base_config;
    use crate::app::storage::state_model::{AppConfig, Subscription};
    use crate::test_support::MockAppEnv;
    use std::fs;

    fn sample_sub(path: &str) -> Subscription {
        Subscription {
            name: "s1".into(),
            url: "http://example.com/sub".into(),
            is_loading: false,
            last_update: None,
            is_manual: false,
            manual_content: None,
            use_original_config: false,
            config_path: Some(path.to_string()),
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
        }
    }

    #[tokio::test]
    async fn db_save_app_config_without_runtime_apply() {
        let env = MockAppEnv::new();
        let db = env.workspace.path().join("app_cfg.db");
        env.install_storage_from_path(db.to_str().unwrap()).await;
        let mut cfg = AppConfig::default();
        cfg.proxy_port = 12345;
        db_save_app_config_persist_only(cfg.clone(), &env.handle())
            .await
            .unwrap();
        db_save_app_config_persist_only(cfg, &env.handle())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn db_save_active_subscription_index_syncs_existing_config() {
        let env = MockAppEnv::new();
        let db = env.workspace.path().join("idx.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;

        let cfg_path = env.workspace.path().join("sub.json");
        let base = generate_base_config(&AppConfig::default());
        fs::write(&cfg_path, serde_json::to_string_pretty(&base).unwrap()).unwrap();

        storage
            .save_subscriptions(&[sample_sub(cfg_path.to_str().unwrap())])
            .await
            .unwrap();
        storage
            .save_app_config(&AppConfig::default())
            .await
            .unwrap();

        // index Some → 同步存在的文件
        db_save_active_subscription_index(Some(0), env.handle())
            .await
            .unwrap();
        // 不存在的 index
        db_save_active_subscription_index(Some(99), env.handle())
            .await
            .unwrap();
        // None index
        db_save_active_subscription_index(None, env.handle())
            .await
            .unwrap();

        // 文件不存在路径
        storage
            .save_subscriptions(&[sample_sub("/tmp/missing-sub-config-xyz.json")])
            .await
            .unwrap();
        db_save_active_subscription_index(Some(0), env.handle())
            .await
            .unwrap();
    }
}
