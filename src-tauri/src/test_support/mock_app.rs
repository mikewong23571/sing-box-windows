//! Tauri MockRuntime app harness for hermetic backend tests.

use crate::app::storage::enhanced_storage_service::EnhancedStorageService;
use crate::test_support::TempWorkspace;
use crate::utils::app_util::WORK_DIR_ENV;
use std::sync::Arc;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tauri::{App, AppHandle, Manager};
use tokio::sync::OnceCell;

/// Mock app + isolated workdir + managed storage OnceCell.
pub struct MockAppEnv {
    pub workspace: TempWorkspace,
    pub app: App<MockRuntime>,
}

impl Default for MockAppEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl MockAppEnv {
    pub fn new() -> Self {
        let workspace = TempWorkspace::new();
        // Ensure work dir env is set before any service resolves paths
        assert_eq!(
            std::env::var(WORK_DIR_ENV).ok().as_deref(),
            Some(workspace.path().to_str().unwrap())
        );

        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock tauri app should build");

        // Storage OnceCell for get_enhanced_storage
        app.manage(Arc::new(OnceCell::<Arc<EnhancedStorageService>>::new()));

        Self { workspace, app }
    }

    pub fn handle(&self) -> AppHandle<MockRuntime> {
        self.app.handle().clone()
    }

    pub async fn install_storage_from_path(&self, db_path: &str) -> Arc<EnhancedStorageService> {
        let svc = Arc::new(
            EnhancedStorageService::from_path(db_path)
                .await
                .expect("open storage"),
        );
        let cell = self
            .app
            .try_state::<Arc<OnceCell<Arc<EnhancedStorageService>>>>()
            .expect("storage cell managed");
        // OnceCell set once
        let _ = cell.set(svc.clone());
        svc
    }
}

#[cfg(test)]
mod emit_tests {
    use super::MockAppEnv;
    use crate::app::core::kernel_service::utils::{
        emit_kernel_error, emit_kernel_started, emit_kernel_starting, emit_kernel_stopped,
        emit_kernel_status, KernelStatusPayload,
    };

    #[test]
    fn mock_app_can_emit_kernel_events() {
        let env = MockAppEnv::new();
        let h = env.handle();
        emit_kernel_starting(&h, "manual", 1, 2);
        emit_kernel_started(&h, "manual", 1, 2, false);
        emit_kernel_status(&h, &KernelStatusPayload::from_state());
        emit_kernel_error(&h, "test-error");
        emit_kernel_stopped(&h);
    }
}

#[cfg(test)]
mod mock_app_e2e {
    use super::MockAppEnv;
    use crate::app::core::kernel_service::lifecycle::{
        prepare_kernel_runtime_before_start, resolve_proxy_runtime_state,
        resolve_proxy_runtime_state_from_config, start_kernel_process_and_verify, ProxyOverrides,
    };
    use crate::app::core::kernel_service::utils::{
        emit_kernel_started, emit_kernel_starting, resolve_config_path, resolve_config_path_or_default,
    };
    use crate::app::core::proxy_service::{
        apply_proxy_runtime_state_with, inject_custom_rules_into_config_file,
        inject_custom_rules_into_config_file_with_storage, update_dns_strategy,
        write_inbounds_for_state, ProxyRuntimeState, RecordingSystemProxy,
    };
    use crate::app::core::tun_profile::TunProxyOptions;
    use crate::app::singbox::config_generator::generate_base_config;
    use crate::app::storage::custom_rule::{
        CustomRule, CustomRuleAction, CustomRuleMatchType, STORAGE_KEY,
    };
    use crate::app::storage::enhanced_storage_service::{
        db_get_app_config, db_save_app_config_internal,
    };
    use crate::app::storage::state_model::AppConfig;
    use crate::app::system::config_service::{ensure_singbox_config, update_singbox_ports};
    use chrono::Utc;
    use std::fs;
    use std::path::Path;
    use tauri::Manager;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn install_fake_kernel(work: &std::path::Path) {
        let dir = work.join("sing-box");
        fs::create_dir_all(&dir).unwrap();
        let kernel = dir.join("sing-box");
        fs::write(
            &kernel,
            r#"#!/bin/sh
if [ "$1" = "check" ]; then exit 0; fi
if [ "$1" = "run" ]; then exec sleep "${FAKE_KERNEL_RUN_SECS:-5}"; fi
exit 0
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = fs::metadata(&kernel).unwrap().permissions();
            p.set_mode(0o755);
            fs::set_permissions(&kernel, p).unwrap();
        }
    }

    async fn setup_env_with_config(
        proxy_port: u16,
        api_port: u16,
    ) -> (MockAppEnv, AppConfig, std::path::PathBuf) {
        let env = MockAppEnv::new();
        install_fake_kernel(env.workspace.path());
        let cfg_path = env.workspace.path().join("sing-box/config.json");
        fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
        let mut cfg = AppConfig::default();
        cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
        cfg.proxy_port = proxy_port;
        cfg.api_port = api_port;
        fs::write(
            &cfg_path,
            serde_json::to_string_pretty(&generate_base_config(&cfg)).unwrap(),
        )
        .unwrap();
        let db = env.workspace.path().join("app_data.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        storage.save_app_config(&cfg).await.unwrap();
        (env, cfg, cfg_path)
    }

    #[tokio::test]
    async fn mock_app_storage_proxy_emit_and_process_start() {
        let (env, cfg, cfg_path) = setup_env_with_config(17001, 17002).await;
        let h = env.handle();
        ensure_singbox_config(&h).await.unwrap();
        let path = resolve_config_path(&h).await.unwrap();
        assert!(path.exists());

        let resolved = resolve_proxy_runtime_state_from_config(
            &cfg,
            &ProxyOverrides {
                proxy_mode: Some("manual".into()),
                ..Default::default()
            },
        );
        let rec = RecordingSystemProxy::default();
        apply_proxy_runtime_state_with(&h, &resolved.proxy, &rec)
            .await
            .unwrap();
        assert!(*rec.disables.lock().unwrap() >= 1);

        emit_kernel_starting(&h, "manual", 17002, 17001);
        let _ = start_kernel_process_and_verify(&cfg_path, 17002, false).await;
        emit_kernel_started(&h, "manual", 17002, 17001, false);
        let _ = crate::app::core::kernel_service::PROCESS_MANAGER
            .stop::<tauri::Wry>(None)
            .await;
    }

    #[tokio::test]
    async fn ensure_singbox_creates_missing_and_repairs_corrupt() {
        let env = MockAppEnv::new();
        install_fake_kernel(env.workspace.path());
        let cfg_path = env.workspace.path().join("sing-box/configs/active.json");
        fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
        let mut cfg = AppConfig::default();
        cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
        let db = env.workspace.path().join("app_data.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        storage.save_app_config(&cfg).await.unwrap();

        let h = env.handle();
        // 缺失文件 → 写默认模板
        ensure_singbox_config(&h).await.unwrap();
        assert!(cfg_path.exists());

        // 损坏文件 + 有 bak → 从 bak 恢复
        let bak = cfg_path.with_extension("bak");
        fs::copy(&cfg_path, &bak).unwrap();
        fs::write(&cfg_path, b"{broken").unwrap();
        ensure_singbox_config(&h).await.unwrap();
        let restored: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&cfg_path).unwrap()).unwrap();
        assert!(restored.is_object());

        // 损坏且无 bak → 写默认
        fs::remove_file(&bak).ok();
        fs::write(&cfg_path, b"not-json").unwrap();
        ensure_singbox_config(&h).await.unwrap();
        assert!(serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&cfg_path).unwrap())
            .is_ok());
    }

    #[tokio::test]
    async fn ensure_singbox_rewrites_external_absolute_path() {
        let env = MockAppEnv::new();
        install_fake_kernel(env.workspace.path());
        let db = env.workspace.path().join("app_data.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        let mut cfg = AppConfig::default();
        // 外部绝对路径应被重定位
        cfg.active_config_path = Some("/tmp/legacy-user/outside/config.json".into());
        storage.save_app_config(&cfg).await.unwrap();

        let h = env.handle();
        ensure_singbox_config(&h).await.unwrap();
        let loaded = db_get_app_config(h.clone()).await.unwrap();
        let path = loaded.active_config_path.unwrap();
        // 应被重定位到本机 workdir 托管目录
        assert!(
            !path.starts_with("/tmp/legacy-user"),
            "path was still external: {}",
            path
        );
        assert!(Path::new(&path).exists() || path.contains("config"));
    }

    #[tokio::test]
    async fn update_singbox_ports_and_validation() {
        let (env, _cfg, cfg_path) = setup_env_with_config(18001, 18002).await;
        let h = env.handle();

        assert!(update_singbox_ports(h.clone(), 100, 18002).await.is_err());
        assert!(update_singbox_ports(h.clone(), 18001, 18001).await.is_err());
        update_singbox_ports(h.clone(), 19001, 19002).await.unwrap();

        let content: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&cfg_path).unwrap()).unwrap();
        let ctrl = content["experimental"]["clash_api"]["external_controller"]
            .as_str()
            .unwrap_or("");
        assert!(ctrl.contains("19002"), "controller={}", ctrl);
    }

    #[tokio::test]
    async fn apply_proxy_all_modes_with_recording() {
        let (env, mut cfg, cfg_path) = setup_env_with_config(17101, 17102).await;
        let h = env.handle();

        // resolve via db + overrides (generic AppHandle)
        let resolved = resolve_proxy_runtime_state(
            &h,
            ProxyOverrides {
                proxy_mode: Some("system".into()),
                proxy_port: Some(17101),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(resolved.proxy.system_proxy_enabled);

        let rec = RecordingSystemProxy::default();
        apply_proxy_runtime_state_with(&h, &resolved.proxy, &rec)
            .await
            .unwrap();
        assert_eq!(rec.enables.lock().unwrap().len(), 1);

        // tun
        let mut tun_state = resolved.proxy.clone();
        tun_state.system_proxy_enabled = false;
        tun_state.tun_enabled = true;
        tun_state.tun_options = TunProxyOptions::default();
        apply_proxy_runtime_state_with(&h, &tun_state, &rec)
            .await
            .unwrap();
        // tun 关闭系统代理
        assert!(*rec.disables.lock().unwrap() >= 1);
        write_inbounds_for_state(&cfg_path, &tun_state).unwrap();

        // manual
        let mut manual = tun_state.clone();
        manual.tun_enabled = false;
        apply_proxy_runtime_state_with(&h, &manual, &rec)
            .await
            .unwrap();

        // dns via AppHandle
        update_dns_strategy(&h, true).await.unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&cfg_path).unwrap()).unwrap();
        assert_eq!(v["dns"]["strategy"], "prefer_ipv6");

        cfg.prefer_ipv6 = false;
        db_save_app_config_internal(cfg, &h).await.unwrap();
        update_dns_strategy(&h, false).await.unwrap();
    }

    #[tokio::test]
    async fn inject_custom_rules_via_mock_storage() {
        let (env, cfg, cfg_path) = setup_env_with_config(17201, 17202).await;
        let h = env.handle();
        let storage = env
            .app
            .try_state::<std::sync::Arc<tokio::sync::OnceCell<std::sync::Arc<crate::app::storage::enhanced_storage_service::EnhancedStorageService>>>>()
            .unwrap();
        let storage = storage.get().unwrap().clone();

        let rules = vec![CustomRule {
            id: "mock-r1".into(),
            enabled: true,
            match_type: CustomRuleMatchType::DomainSuffix,
            payload: "openai.com".into(),
            action: CustomRuleAction::Proxy,
            outbound: None,
            note: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }];
        storage
            .save_generic_config(STORAGE_KEY, &rules)
            .await
            .unwrap();

        inject_custom_rules_into_config_file_with_storage(storage.as_ref(), &cfg, &cfg_path)
            .await
            .unwrap();
        inject_custom_rules_into_config_file(&h, &cfg_path)
            .await
            .unwrap();

        let final_cfg: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&cfg_path).unwrap()).unwrap();
        let marked = final_cfg["route"]["rules"]
            .as_array()
            .map(|a| a.iter().filter(|r| r.get("__custom_rule__").is_some()).count())
            .unwrap_or(0);
        assert!(marked >= 1);
    }

    #[tokio::test]
    async fn start_kernel_process_and_verify_with_mock_api_success() {
        let (env, _cfg, cfg_path) = setup_env_with_config(17301, 17302).await;
        let _h = env.handle();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else {
                    break;
                };
                let mut buf = [0u8; 512];
                let _ = s.read(&mut buf).await;
                let body = r#"{"version":"1.2.3"}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes()).await;
            }
        });

        // 完整冷启动路径
        start_kernel_process_and_verify(&cfg_path, port, false)
            .await
            .expect("should pass with mock /version");
        assert!(crate::app::core::kernel_service::PROCESS_MANAGER
            .is_running()
            .await);

        // 已运行再调一次
        start_kernel_process_and_verify(&cfg_path, port, false)
            .await
            .expect("already running + stable");

        let _ = crate::app::core::kernel_service::PROCESS_MANAGER
            .stop::<tauri::Wry>(None)
            .await;
    }

    #[tokio::test]
    async fn resolve_config_path_variants() {
        let (env, _cfg, cfg_path) = setup_env_with_config(17401, 17402).await;
        let h = env.handle();
        let p = resolve_config_path(&h).await.unwrap();
        assert_eq!(p, cfg_path);
        let p2 = resolve_config_path_or_default(&h).await;
        assert_eq!(p2, cfg_path);
    }

    #[tokio::test]
    async fn proxy_runtime_state_manual_system_tun_inbounds() {
        let (env, _cfg, cfg_path) = setup_env_with_config(17501, 17502).await;
        let h = env.handle();
        ensure_singbox_config(&h).await.unwrap();

        for (sys, tun) in [(true, false), (false, true), (false, false)] {
            let state = ProxyRuntimeState {
                proxy_port: 17501,
                allow_lan_access: sys,
                system_proxy_enabled: sys,
                tun_enabled: tun,
                system_proxy_bypass: "localhost".into(),
                tun_options: TunProxyOptions::default(),
            };
            write_inbounds_for_state(&cfg_path, &state).unwrap();
            let rec = RecordingSystemProxy::default();
            apply_proxy_runtime_state_with(&h, &state, &rec)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn prepare_kernel_runtime_then_process_verify_success() {
        let (env, cfg, cfg_path) = setup_env_with_config(17601, 17602).await;
        let h = env.handle();
        let resolved = resolve_proxy_runtime_state_from_config(
            &cfg,
            &ProxyOverrides {
                proxy_mode: Some("manual".into()),
                api_port: Some(17602),
                proxy_port: Some(17601),
                ..Default::default()
            },
        );

        // 完整启动前准备（config/ports/proxy/dns/log）
        prepare_kernel_runtime_before_start(&h, &resolved)
            .await
            .expect("prepare should succeed");

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else {
                    break;
                };
                let mut buf = [0u8; 512];
                let _ = s.read(&mut buf).await;
                let body = r#"{"version":"9.9.9"}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes()).await;
            }
        });

        emit_kernel_starting(&h, &resolved.derived_mode(), port, 17601);
        start_kernel_process_and_verify(&cfg_path, port, false)
            .await
            .expect("process+stability");
        emit_kernel_started(&h, &resolved.derived_mode(), port, 17601, false);

        // 二次 prepare 在进程已运行时也应成功
        prepare_kernel_runtime_before_start(&h, &resolved)
            .await
            .unwrap();

        let _ = crate::app::core::kernel_service::PROCESS_MANAGER
            .stop::<tauri::Wry>(None)
            .await;
    }
}

#[cfg(test)]
mod mock_app_storage_commands {
    use super::MockAppEnv;
    use crate::app::storage::enhanced_storage_service::{
        db_get_app_config, db_get_locale_config, db_get_subscriptions, db_get_theme_config,
        db_get_update_config, db_get_window_config, db_save_app_config_internal,
        db_save_locale_config, db_save_subscriptions, db_save_theme_config, db_save_update_config,
        db_save_window_config,
    };
    use crate::app::storage::state_model::{
        AppConfig, LocaleConfig, Subscription, ThemeConfig, UpdateConfig, WindowConfig,
    };

    #[tokio::test]
    async fn mock_db_all_config_types_roundtrip() {
        let env = MockAppEnv::new();
        let db = env.workspace.path().join("all.db");
        env.install_storage_from_path(db.to_str().unwrap()).await;
        let h = env.handle();

        let mut app = AppConfig::default();
        app.proxy_port = 18888;
        db_save_app_config_internal(app.clone(), &h).await.unwrap();
        assert_eq!(db_get_app_config(h.clone()).await.unwrap().proxy_port, 18888);

        let theme = ThemeConfig {
            is_dark: false,
            ..ThemeConfig::default()
        };
        db_save_theme_config(theme, h.clone()).await.unwrap();
        assert!(!db_get_theme_config(h.clone()).await.unwrap().is_dark);

        let locale = LocaleConfig {
            locale: "en-US".into(),
        };
        db_save_locale_config(locale, h.clone()).await.unwrap();
        assert_eq!(db_get_locale_config(h.clone()).await.unwrap().locale, "en-US");

        db_save_window_config(WindowConfig::default(), h.clone())
            .await
            .unwrap();
        let _ = db_get_window_config(h.clone()).await.unwrap();

        db_save_update_config(UpdateConfig::default(), h.clone())
            .await
            .unwrap();
        let _ = db_get_update_config(h.clone()).await.unwrap();

        let subs = vec![Subscription {
            name: "m".into(),
            url: "https://x".into(),
            is_loading: false,
            last_update: None,
            is_manual: true,
            manual_content: Some("x".into()),
            use_original_config: false,
            config_path: Some("c.json".into()),
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
        }];
        db_save_subscriptions(subs, h.clone()).await.unwrap();
        assert_eq!(db_get_subscriptions(h.clone()).await.unwrap().len(), 1);
    }
}
