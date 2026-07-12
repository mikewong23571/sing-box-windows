use std::time::Duration;

use tauri::{AppHandle, Emitter, Runtime};
use tracing::{info, warn};

use crate::app::network::subscription_service::{
    download_subscription_core, get_current_config_impl,
};
use crate::app::storage::enhanced_storage_service::{
    db_get_app_config, db_get_subscriptions, db_save_subscriptions,
};
use crate::app::storage::state_model::Subscription;

// 默认 12 小时
const DEFAULT_INTERVAL_MINUTES: u64 = 12 * 60;
const MAX_BACKOFF_MINUTES: u64 = 24 * 60;

#[derive(Debug, Clone)]
pub(crate) struct SubscriptionHealthPatch {
    pub config_path: Option<String>,
    pub url: String,
    pub fail_count: u32,
    pub last_attempt_ms: u64,
    pub last_error: Option<String>,
    pub last_error_type: Option<String>,
    pub backoff_until_ms: Option<u64>,
}

pub(crate) fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn classify_error(error: &str) -> &'static str {
    let lower = error.to_ascii_lowercase();
    if lower.contains("timeout") || lower.contains("timed out") {
        return "timeout";
    }
    if lower.contains("dns") || lower.contains("resolve") {
        return "network_dns";
    }
    if lower.contains("401") || lower.contains("403") || lower.contains("unauthorized") {
        return "auth";
    }
    if lower.contains("json") || lower.contains("yaml") || lower.contains("配置") {
        return "config_parse";
    }
    if lower.contains("connect")
        || lower.contains("network")
        || lower.contains("connection")
        || lower.contains("tls")
    {
        return "network";
    }
    "unknown"
}

pub(crate) fn calc_backoff_minutes(base_interval_minutes: u64, fail_count: u32) -> u64 {
    let base = base_interval_minutes.max(5);
    let exp = fail_count.saturating_sub(1).min(6);
    let factor = 2_u64.pow(exp);
    (base.saturating_mul(factor)).min(MAX_BACKOFF_MINUTES)
}

pub(crate) fn should_run_for_subscription(sub: &Subscription, now_ms: u64) -> bool {
    let interval = sub
        .auto_update_interval_minutes
        .unwrap_or(DEFAULT_INTERVAL_MINUTES);
    if interval == 0 {
        return false;
    }

    if let Some(backoff_until_ms) = sub.last_auto_update_backoff_until {
        if now_ms < backoff_until_ms {
            return false;
        }
    }

    let last_ref = sub
        .last_auto_update_attempt
        .or(sub.last_update)
        .unwrap_or(0);
    if last_ref == 0 {
        return true;
    }

    now_ms.saturating_sub(last_ref) >= interval.max(5) * 60 * 1000
}

pub(crate) fn subscription_matches_patch(
    sub: &Subscription,
    patch: &SubscriptionHealthPatch,
) -> bool {
    let url_match = !patch.url.is_empty() && sub.url.trim() == patch.url;
    let path_match = match (&patch.config_path, &sub.config_path) {
        (Some(lhs), Some(rhs)) => lhs == rhs,
        _ => false,
    };
    path_match || url_match
}

pub(crate) fn apply_health_patch(sub: &mut Subscription, patch: &SubscriptionHealthPatch) {
    sub.auto_update_fail_count = Some(patch.fail_count);
    sub.last_auto_update_attempt = Some(patch.last_attempt_ms);
    sub.last_auto_update_error = patch.last_error.clone();
    sub.last_auto_update_error_type = patch.last_error_type.clone();
    sub.last_auto_update_backoff_until = patch.backoff_until_ms;
}

pub(crate) async fn save_health_patches<R: Runtime>(
    app: &AppHandle<R>,
    patches: &[SubscriptionHealthPatch],
) -> Result<(), String> {
    if patches.is_empty() {
        return Ok(());
    }

    let mut latest = db_get_subscriptions(app.clone())
        .await
        .map_err(|e| format!("读取订阅配置失败: {}", e))?;

    for sub in latest.iter_mut() {
        if let Some(patch) = patches
            .iter()
            .find(|patch| subscription_matches_patch(sub, patch))
        {
            apply_health_patch(sub, patch);
        }
    }

    db_save_subscriptions(latest, app.clone())
        .await
        .map_err(|e| format!("保存订阅健康状态失败: {}", e))
}

pub async fn start_subscription_auto_update(app: &AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            if let Err(e) = run_once(&handle).await {
                warn!("自动订阅刷新失败: {}", e);
            }

            let interval = get_min_interval_minutes(&handle)
                .await
                .unwrap_or(DEFAULT_INTERVAL_MINUTES);
            tokio::time::sleep(Duration::from_secs(interval * 60)).await;
        }
    });
}

/// 从订阅列表计算最小自动更新间隔（分钟，至少 5）。
/// `interval == 0` 表示关闭自动更新，不参与最小值计算。
pub(crate) fn calc_min_interval_minutes(subs: &[Subscription]) -> u64 {
    let mut min_interval = DEFAULT_INTERVAL_MINUTES;
    for sub in subs.iter() {
        if let Some(interval) = sub.auto_update_interval_minutes {
            if interval > 0 && interval < min_interval {
                min_interval = interval;
            }
        }
    }
    min_interval.max(5)
}

async fn get_min_interval_minutes<R: Runtime>(app: &AppHandle<R>) -> Result<u64, String> {
    let subs = db_get_subscriptions(app.clone())
        .await
        .map_err(|e| format!("读取订阅配置失败: {}", e))?;
    Ok(calc_min_interval_minutes(&subs))
}

/// 构造成功/失败健康补丁（纯逻辑）。
pub(crate) fn build_success_health_patch(
    config_path: Option<String>,
    url: &str,
    now_ms: u64,
) -> SubscriptionHealthPatch {
    SubscriptionHealthPatch {
        config_path,
        url: url.trim().to_string(),
        fail_count: 0,
        last_attempt_ms: now_ms,
        last_error: None,
        last_error_type: None,
        backoff_until_ms: None,
    }
}

pub(crate) fn build_failure_health_patch(
    config_path: Option<String>,
    url: &str,
    now_ms: u64,
    prev_fail: u32,
    interval_minutes: u64,
    error: &str,
) -> SubscriptionHealthPatch {
    let next_fail = prev_fail.saturating_add(1);
    let backoff_minutes = calc_backoff_minutes(interval_minutes, next_fail);
    SubscriptionHealthPatch {
        config_path,
        url: url.trim().to_string(),
        fail_count: next_fail,
        last_attempt_ms: now_ms,
        last_error: Some(error.to_string()),
        last_error_type: Some(classify_error(error).to_string()),
        backoff_until_ms: Some(now_ms + backoff_minutes * 60 * 1000),
    }
}

/// 筛选需要自动更新的订阅（纯逻辑）。
pub(crate) fn collect_subscriptions_due(subs: &[Subscription], now_ms: u64) -> Vec<&Subscription> {
    subs.iter()
        .filter(|sub| {
            let interval = sub
                .auto_update_interval_minutes
                .unwrap_or(DEFAULT_INTERVAL_MINUTES);
            interval != 0 && should_run_for_subscription(sub, now_ms)
        })
        .collect()
}

/// 是否应对该订阅应用运行时（纯逻辑：仅 active 订阅）。
pub(crate) fn should_apply_runtime_for_subscription(
    active_config_path: Option<&str>,
    sub_config_path: Option<&str>,
) -> bool {
    match (active_config_path, sub_config_path) {
        (Some(active), Some(sub_path)) => active == sub_path,
        _ => false,
    }
}

/// 自动更新单轮核心（无 Window，任意 Runtime；MockAppEnv 可测）。
pub(crate) async fn run_once<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let subs = db_get_subscriptions(app.clone())
        .await
        .map_err(|e| format!("读取订阅配置失败: {}", e))?;

    let app_config = db_get_app_config(app.clone())
        .await
        .map_err(|e| format!("读取应用配置失败: {}", e))?;

    let now_ms = now_millis();
    let mut health_patches: Vec<SubscriptionHealthPatch> = Vec::new();
    let due: Vec<Subscription> = collect_subscriptions_due(&subs, now_ms)
        .into_iter()
        .cloned()
        .collect();
    for sub in due {
        let interval = sub
            .auto_update_interval_minutes
            .unwrap_or(DEFAULT_INTERVAL_MINUTES);

        info!("自动刷新订阅: {}", sub.name);

        // 仅当当前订阅就是用户正在使用的订阅时，才允许应用到运行时。
        let should_apply_runtime = should_apply_runtime_for_subscription(
            app_config.active_config_path.as_deref(),
            sub.config_path.as_deref(),
        );

        match download_subscription_core(
            app,
            sub.url.clone(),
            sub.use_original_config,
            Some(format!("{}.json", sub.name)),
            sub.config_path.clone(),
            should_apply_runtime,
            Some(app_config.proxy_port),
            Some(app_config.api_port),
        )
        .await
        {
            Ok(_) => {
                info!("自动刷新订阅 {} 完成", sub.name);
                health_patches.push(build_success_health_patch(
                    sub.config_path.clone(),
                    &sub.url,
                    now_ms,
                ));
            }
            Err(e) => {
                warn!("自动刷新订阅 {} 失败: {}", sub.name, e);
                health_patches.push(build_failure_health_patch(
                    sub.config_path.clone(),
                    &sub.url,
                    now_ms,
                    sub.auto_update_fail_count.unwrap_or(0),
                    interval,
                    &e,
                ));
            }
        };
    }

    // 仅回写自动更新健康字段，避免覆盖下载流程刚更新的流量额度等字段。
    if let Err(e) = save_health_patches(app, &health_patches).await {
        warn!("回写订阅健康状态失败: {}", e);
    }

    // 触发前端刷新当前配置
    if let Ok(cfg) = get_current_config_impl(app.clone()).await {
        let _ = app.emit("subscription-updated", cfg);
    }

    Ok(())
}

#[cfg(test)]
#[path = "auto_update.tests.rs"]
mod tests;
