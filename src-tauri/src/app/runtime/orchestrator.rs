use crate::app::core::kernel_auto_manage::run_auto_manage_with_saved_config;
use crate::app::core::kernel_service::utils::{AppHandleOwnedSink, KernelEventSink};
use crate::app::core::kernel_service::{kernel_process_manager_singleton, KernelProcessControl};
use crate::app::core::proxy_service::{
    apply_proxy_runtime_state_with, update_dns_strategy, ProxyRuntimeState, SystemProxyPort,
};
use crate::app::core::tun_profile::TunProxyOptions;
use crate::app::runtime::change::{
    plan_runtime_actions, RuntimeApplyOptions, RuntimeApplyResult, RuntimeChange,
};
use crate::app::runtime::config_update::sync_active_config_settings;
use crate::app::storage::enhanced_storage_service::{
    db_get_app_config, get_enhanced_storage, EnhancedStorageService,
};
use crate::app::storage::state_model::AppConfig;
use std::sync::Arc;
use tauri::{AppHandle, Runtime};
use tracing::warn;

pub fn runtime_state_from_config(app_config: &AppConfig) -> ProxyRuntimeState {
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

/// 运行态变更所需的外部依赖（可注入，便于测试）。
pub struct RuntimeDeps<R: Runtime> {
    pub storage: Arc<EnhancedStorageService>,
    pub process: Arc<dyn KernelProcessControl<R>>,
    pub events: Arc<dyn KernelEventSink>,
    pub system_proxy: Arc<dyn SystemProxyPort>,
}

impl<R: Runtime> RuntimeDeps<R> {
    pub async fn from_app(app: &AppHandle<R>) -> Result<Self, String> {
        let storage = get_enhanced_storage(app).await.map_err(|e| e.to_string())?;
        Ok(Self {
            storage,
            process: kernel_process_manager_singleton(),
            events: Arc::new(AppHandleOwnedSink(app.clone())),
            system_proxy: Arc::new(crate::app::core::proxy_service::OsSystemProxy),
        })
    }
}

/// 应用运行态变更（可注入依赖版本）。
pub async fn apply_runtime_change_with_deps<R: Runtime>(
    app: &AppHandle<R>,
    deps: &RuntimeDeps<R>,
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
        if let Err(e) =
            apply_proxy_runtime_state_with(app, &runtime_state, deps.system_proxy.as_ref()).await
        {
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

/// 应用运行态变更（泛型 Runtime，MockAppEnv 可测；内部组装默认依赖）。
pub async fn apply_runtime_change<R: Runtime>(
    app: &AppHandle<R>,
    change: RuntimeChange,
    options: RuntimeApplyOptions,
) -> Result<RuntimeApplyResult, String> {
    let deps = RuntimeDeps::from_app(app).await?;
    apply_runtime_change_with_deps(app, &deps, change, options).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::storage::state_model::AppConfig;
    use crate::test_support::MockAppEnv;
    use std::fs;

    #[test]
    fn runtime_state_from_config_maps_fields() {
        let mut cfg = AppConfig::default();
        cfg.proxy_port = 1111;
        cfg.allow_lan_access = true;
        cfg.system_proxy_enabled = true;
        cfg.tun_enabled = false;
        cfg.system_proxy_bypass = "localhost".into();
        cfg.tun_mtu = 1400;
        let state = runtime_state_from_config(&cfg);
        assert_eq!(state.proxy_port, 1111);
        assert!(state.allow_lan_access);
        assert!(state.system_proxy_enabled);
        assert!(!state.tun_enabled);
        assert_eq!(state.system_proxy_bypass, "localhost");
        assert_eq!(state.tun_options.mtu, 1400);
        assert_eq!(state.derived_mode(), "system");
    }

    #[tokio::test]
    async fn apply_runtime_change_proxy_and_patch_without_auto_manage() {
        let env = MockAppEnv::new();
        let cfg_path = env.workspace.path().join("sing-box/configs/active.json");
        fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
        fs::write(
            &cfg_path,
            r#"{"log":{"level":"info"},"inbounds":[],"outbounds":[{"type":"direct","tag":"direct"}]}"#,
        )
        .unwrap();

        let db = env.workspace.path().join("runtime.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        let mut cfg = AppConfig::default();
        cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
        cfg.system_proxy_enabled = false;
        cfg.tun_enabled = false;
        cfg.proxy_port = 17880;
        storage.save_app_config(&cfg).await.unwrap();

        let h = env.handle();
        // ProxySettingsChanged：应用代理、不自动管理内核
        let options = RuntimeApplyOptions::new("test-proxy-only").patch_active_config(false);
        let result = apply_runtime_change(&h, RuntimeChange::ProxySettingsChanged, options)
            .await
            .expect("apply proxy change");
        assert!(!result.config_patched);
        assert!(result.auto_manage_state.is_none());
        assert_eq!(result.change, RuntimeChange::ProxySettingsChanged.as_str());
    }

    #[tokio::test]
    async fn apply_runtime_change_subscription_patches_and_auto_manage_missing_kernel() {
        let env = MockAppEnv::new();
        let cfg_path = env.workspace.path().join("sing-box/configs/sub.json");
        fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
        fs::write(
            &cfg_path,
            r#"{"log":{"level":"info"},"inbounds":[],"outbounds":[{"type":"direct","tag":"direct"}]}"#,
        )
        .unwrap();

        let db = env.workspace.path().join("runtime2.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        let mut cfg = AppConfig::default();
        cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
        cfg.system_proxy_enabled = false;
        cfg.tun_enabled = false;
        storage.save_app_config(&cfg).await.unwrap();

        let h = env.handle();
        let options = RuntimeApplyOptions::new("test-sub-apply")
            .patch_active_config(true)
            .force_restart(true)
            .use_original_config_hint(Some(true));
        // 无内核时 auto_manage 返回 missing_kernel，仍应 Ok
        let result = apply_runtime_change(&h, RuntimeChange::SubscriptionApplied, options)
            .await
            .expect("apply subscription change");
        assert!(result.config_patched);
        assert!(result.auto_manage_state.is_some());
        // 活动配置应被 PortsOnly 补丁（文件仍存在）
        assert!(cfg_path.exists());
    }

    #[tokio::test]
    async fn apply_runtime_change_app_config_updated_path() {
        let env = MockAppEnv::new();
        let cfg_path = env.workspace.path().join("sing-box/config.json");
        fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
        fs::write(
            &cfg_path,
            r#"{"log":{"level":"info"},"inbounds":[],"outbounds":[{"type":"direct","tag":"direct"}]}"#,
        )
        .unwrap();
        let db = env.workspace.path().join("runtime3.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        let mut cfg = AppConfig::default();
        cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
        storage.save_app_config(&cfg).await.unwrap();

        let options = RuntimeApplyOptions::new("test-app-config")
            .patch_active_config(true)
            .force_restart(false);
        let result = apply_runtime_change(
            &env.handle(),
            RuntimeChange::AppConfigUpdated,
            options,
        )
        .await
        .expect("app config update");
        assert!(result.config_patched);
        assert_eq!(result.change, "app_config_updated");
    }

    #[tokio::test]
    async fn apply_runtime_change_with_deps_records_proxy_call() {
        use crate::app::core::proxy_service::RecordingSystemProxy;
        use crate::test_support::FakeProcessController;

        let env = MockAppEnv::new();
        let cfg_path = env.workspace.path().join("sing-box/configs/deps.json");
        fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
        fs::write(
            &cfg_path,
            r#"{"log":{"level":"info"},"inbounds":[],"outbounds":[{"type":"direct","tag":"direct"}]}"#,
        )
        .unwrap();

        let db = env.workspace.path().join("runtime_deps.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        let mut cfg = AppConfig::default();
        cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
        cfg.system_proxy_enabled = true;
        cfg.proxy_port = 17777;
        storage.save_app_config(&cfg).await.unwrap();

        let proxy = Arc::new(RecordingSystemProxy::default());
        let process: Arc<dyn KernelProcessControl<tauri::test::MockRuntime>> =
            Arc::new(FakeProcessController::default());
        let deps = RuntimeDeps::for_test(storage.clone(), process, proxy.clone());

        let options = RuntimeApplyOptions::new("test-deps").patch_active_config(false);
        let result = apply_runtime_change_with_deps(
            &env.handle(),
            &deps,
            RuntimeChange::ProxySettingsChanged,
            options,
        )
        .await
        .expect("apply with deps");
        assert!(!result.config_patched);
        // 系统代理应被 recording proxy 记录，而不是直接调用 OS
        assert_eq!(proxy.enables.lock().unwrap().len(), 1);
    }
}
