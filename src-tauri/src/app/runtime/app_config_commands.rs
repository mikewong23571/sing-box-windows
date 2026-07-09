use crate::app::runtime::change::{RuntimeApplyOptions, RuntimeChange};
use crate::app::runtime::config_update::{
    resolve_patch_mode_with_hint, sync_settings_to_config_file, ConfigPatchMode,
};
use crate::app::runtime::orchestrator::apply_runtime_change;
use crate::app::storage::enhanced_storage_service::{
    db_save_app_config_internal, get_enhanced_storage,
};
use crate::app::storage::state_model::AppConfig;
use tauri::AppHandle;

#[tauri::command]
pub async fn db_save_app_config(
    config: AppConfig,
    app: AppHandle,
    apply_runtime: Option<bool>,
) -> Result<(), String> {
    db_save_app_config_internal(config, &app).await?;
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
        .force_restart(true);
    apply_runtime_change(&app, RuntimeChange::AppConfigUpdated, options).await?;

    Ok(())
}

#[tauri::command]
pub async fn db_save_active_subscription_index(
    index: Option<i64>,
    app: AppHandle,
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
