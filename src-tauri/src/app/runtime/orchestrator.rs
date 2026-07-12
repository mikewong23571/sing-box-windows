use crate::app::core::kernel_service::utils::{AppHandleOwnedSink, KernelEventSink};
use crate::app::core::kernel_service::{
    kernel_process_manager_singleton, orchestrated_apply_change_with_deps, KernelChangeImpact,
    KernelProcessControl,
};
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

    let runtime_active = deps.process.is_running().await;
    let mut proxy_applied = false;
    if plan.apply_proxy_runtime && runtime_active {
        let runtime_state = runtime_state_from_config(&effective_config);
        apply_proxy_runtime_state_with(app, &runtime_state, deps.system_proxy.as_ref())
            .await
            .map_err(|e| format!("应用运行态代理配置失败({}): {}", options.reason, e))?;
        proxy_applied = true;
        if let Err(e) = update_dns_strategy(app, effective_config.prefer_ipv6).await {
            warn!("更新 DNS 策略失败({}): {}", options.reason, e);
        }
    }

    let (kernel_action, message) = if plan.kernel_impact == KernelChangeImpact::RestartIfRunning {
        let result = orchestrated_apply_change_with_deps(
            app.clone(),
            plan.kernel_impact,
            options.reason.clone(),
            deps.process.clone(),
            deps.system_proxy.clone(),
        )
        .await?;
        if !result
            .get("success")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            return Err(result
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("内核运行态变更失败")
                .to_string());
        }
        let message = result
            .get("message")
            .and_then(|value| value.as_str())
            .unwrap_or("运行态变更已应用")
            .to_string();
        let state = match plan.kernel_impact {
            KernelChangeImpact::HotApply => "hot_apply",
            KernelChangeImpact::RestartIfRunning => "restart_if_running",
            KernelChangeImpact::PersistOnly => "persist_only",
        };
        (Some(state.to_string()), message)
    } else if plan.kernel_impact == KernelChangeImpact::HotApply && runtime_active {
        (
            Some("hot_apply".to_string()),
            "运行态变更已热应用".to_string(),
        )
    } else {
        (None, "配置已保存，内核状态保持不变".to_string())
    };

    Ok(RuntimeApplyResult {
        change: change.as_str().to_string(),
        reason: options.reason,
        config_patched,
        proxy_applied,
        kernel_action,
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
    async fn apply_runtime_change_proxy_and_patch_preserves_stopped_kernel() {
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
        assert!(result.kernel_action.is_none());
        assert_eq!(result.change, RuntimeChange::ProxySettingsChanged.as_str());
    }

    #[tokio::test]
    async fn apply_runtime_change_subscription_requests_restart_if_running() {
        use crate::app::core::kernel_service::status::set_platform_kernel_detection_enabled_for_tests;
        use crate::app::core::proxy_service::RecordingSystemProxy;
        use crate::test_support::FakeProcessController;

        set_platform_kernel_detection_enabled_for_tests(false);
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
            .restart_if_running(true)
            .use_original_config_hint(Some(true));
        // 使用明确处于停止态的依赖，避免与全局进程管理器的其他测试互相影响。
        let process: Arc<dyn KernelProcessControl<tauri::test::MockRuntime>> =
            Arc::new(FakeProcessController::default());
        let deps =
            RuntimeDeps::for_test(storage, process, Arc::new(RecordingSystemProxy::default()));
        // 无运行内核时保持停止，配置 patch 仍应成功。
        let result =
            apply_runtime_change_with_deps(&h, &deps, RuntimeChange::SubscriptionApplied, options)
                .await
                .expect("apply subscription change");
        assert!(result.config_patched);
        assert!(result.kernel_action.is_some());
        // 活动配置应被 PortsOnly 补丁（文件仍存在）
        assert!(cfg_path.exists());
        set_platform_kernel_detection_enabled_for_tests(true);
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
            .restart_if_running(false);
        let result = apply_runtime_change(&env.handle(), RuntimeChange::AppConfigUpdated, options)
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
        let fake_process = Arc::new(FakeProcessController::default());
        fake_process.set_running(true);
        let process: Arc<dyn KernelProcessControl<tauri::test::MockRuntime>> = fake_process;
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
