use crate::app::core::kernel_auto_manage::run_auto_manage_with_saved_config;
use crate::app::core::proxy_service::{
    apply_proxy_runtime_state, update_dns_strategy, ProxyRuntimeState,
};
use crate::app::core::tun_profile::TunProxyOptions;
use crate::app::runtime::change::{
    plan_runtime_actions, RuntimeApplyOptions, RuntimeApplyResult, RuntimeChange,
};
use crate::app::runtime::config_update::sync_active_config_settings;
use crate::app::storage::enhanced_storage_service::db_get_app_config;
use crate::app::storage::state_model::AppConfig;
use tauri::AppHandle;
use tracing::warn;

fn runtime_state_from_config(app_config: &AppConfig) -> ProxyRuntimeState {
    ProxyRuntimeState {
        proxy_port: app_config.proxy_port,
        allow_lan_access: app_config.allow_lan_access,
        system_proxy_enabled: app_config.system_proxy_enabled,
        tun_enabled: app_config.tun_enabled,
        system_proxy_bypass: app_config.system_proxy_bypass.clone(),
        tun_options: TunProxyOptions {
            ipv4_address: app_config.tun_ipv4.clone(),
            ipv6_address: app_config.tun_ipv6.clone(),
            mtu: app_config.tun_mtu,
            auto_route: app_config.tun_auto_route,
            strict_route: app_config.tun_strict_route,
            stack: app_config.tun_stack.clone(),
            enable_ipv6: app_config.tun_enable_ipv6,
            route_exclude_address: app_config.tun_route_exclude_address.clone(),
            interface_name: None,
        },
    }
}

pub async fn apply_runtime_change(
    app: &AppHandle,
    change: RuntimeChange,
    options: RuntimeApplyOptions,
) -> Result<RuntimeApplyResult, String> {
    let plan = plan_runtime_actions(change, &options);
    let effective_config = db_get_app_config(app.clone()).await?;

    let mut config_patched = false;
    if plan.patch_active_config {
        sync_active_config_settings(app, &effective_config, options.use_original_config_hint).await;
        config_patched = true;
    }

    let mut proxy_applied = false;
    if plan.apply_proxy_runtime {
        let runtime_state = runtime_state_from_config(&effective_config);
        if let Err(e) = apply_proxy_runtime_state(app, &runtime_state).await {
            warn!("应用运行态代理配置失败({}): {}", options.reason, e);
        } else {
            proxy_applied = true;
        }
        if let Err(e) = update_dns_strategy(app, effective_config.prefer_ipv6).await {
            warn!("更新 DNS 策略失败({}): {}", options.reason, e);
        }
    }

    let mut auto_manage_state = None;
    let mut message = "运行态变更已应用".to_string();
    if plan.auto_manage_kernel {
        match run_auto_manage_with_saved_config(app, options.force_restart, &options.reason).await {
            Ok(Some(result)) => {
                message = result.message.clone();
                auto_manage_state = Some(result.state);
            }
            Ok(None) => {
                message = "运行态自动管理已跳过".to_string();
            }
            Err(err) => {
                return Err(err);
            }
        }
    }

    Ok(RuntimeApplyResult {
        change: change.as_str().to_string(),
        reason: options.reason,
        config_patched,
        proxy_applied,
        auto_manage_state,
        message,
    })
}
