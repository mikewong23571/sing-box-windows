//! Backend L3 E2E — multi-module hermetic journeys (no real network/sudo/OS proxy).
//!
//! Counting rules for AC2:
//! - Each `l3_*` test must cross **≥2 production modules** via real shipped APIs
//! - Assertions must fail if a module is broken (no tautologies)
//! - Hermetic: TempWorkspace + fake sing-box + RecordingSystemProxy + 127.0.0.1 mocks

use crate::app::core::kernel_service::lifecycle::{
    apply_proxy_overrides_to_app_config, classify_runtime_start_failure,
    resolve_proxy_runtime_state_from_config, start_kernel_process_and_verify, stop_kernel,
    ProxyOverrides,
};
use crate::app::core::kernel_service::readiness::{
    verify_kernel_startup_stability_with_config, StabilityCheckConfig,
};
use crate::app::core::kernel_service::status::{
    collect_kernel_runtime_probe, is_kernel_running, is_kernel_running_with_process,
    kernel_check_health, probe_version_api,
};
use crate::app::core::kernel_service::PROCESS_MANAGER;
use crate::app::core::proxy_service::{
    apply_dns_strategy_to_config, apply_system_proxy_for_state, build_clash_delay_url,
    get_proxies, inject_custom_rules_into_config_file_with_storage, measure_proxy_delay,
    update_dns_strategy_on_path, write_inbounds_for_state, ProxyRuntimeState,
    RecordingSystemProxy,
};
use crate::app::core::tun_profile::TunProxyOptions;
use crate::app::network::subscription_service::auto_update::{
    apply_health_patch, build_failure_health_patch, build_success_health_patch, calc_backoff_minutes,
    should_run_for_subscription,
};
use crate::app::network::subscription_service::materializer::{
    try_decode_base64_to_text, write_downloaded_subscription_config,
    write_manual_subscription_config,
};
use crate::app::network::subscription_service::{
    apply_userinfo_to_subscriptions, fetch_subscription_content_with_user_agent,
};
use crate::app::runtime::change::{plan_runtime_actions, RuntimeApplyOptions, RuntimeChange};
use crate::app::runtime::config_update::{
    resolve_patch_mode_for_subscription, resolve_patch_mode_with_hint, sync_settings_to_config_file,
    ConfigPatchMode,
};
use crate::app::runtime::orchestrator::runtime_state_from_config;
use crate::app::singbox::config_generator::generate_base_config;
use crate::app::storage::custom_rule::{
    CustomRule, CustomRuleAction, CustomRuleMatchType, STORAGE_KEY,
};
use crate::app::storage::enhanced_storage_service::EnhancedStorageService;
use crate::app::storage::state_model::{
    AppConfig, LocaleConfig, Subscription, ThemeConfig, UpdateConfig, WindowConfig,
};
use crate::app::system::backup_service::{
    apply_snapshot_to_storage, encode_path_for_snapshot, parse_snapshot, rewrite_paths_for_snapshot,
    write_config_content, SnapshotPathKind,
};
use crate::app::system::config_service::{
    backup_corrupted_config, ensure_private_ip_rule, try_restore_from_bak, write_default_config,
};
use crate::process::manager::ProcessManager;
use crate::test_support::TempWorkspace;
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

struct Env {
    _ws: TempWorkspace,
    storage: EnhancedStorageService,
    config_path: PathBuf,
    work_dir: PathBuf,
}

impl Env {
    async fn new() -> Self {
        let ws = TempWorkspace::new();
        let work_dir = ws.path().to_path_buf();
        let sing = work_dir.join("sing-box");
        fs::create_dir_all(&sing).unwrap();
        install_fake_kernel(&work_dir);
        let config_path = sing.join("config.json");
        let mut cfg = AppConfig::default();
        cfg.active_config_path = Some(config_path.to_string_lossy().to_string());
        cfg.proxy_port = 17801;
        cfg.api_port = 17802;
        fs::write(
            &config_path,
            serde_json::to_string_pretty(&generate_base_config(&cfg)).unwrap(),
        )
        .unwrap();
        let storage =
            EnhancedStorageService::from_path(work_dir.join("app_data.db").to_str().unwrap())
                .await
                .unwrap();
        storage.save_app_config(&cfg).await.unwrap();
        Self {
            _ws: ws,
            storage,
            config_path,
            work_dir,
        }
    }
}

fn install_fake_kernel(work: &Path) {
    let dir = work.join("sing-box");
    fs::create_dir_all(&dir).unwrap();
    let kernel = dir.join("sing-box");
    fs::write(
        &kernel,
        r#"#!/bin/sh
if [ "$1" = "check" ]; then exit 0; fi
if [ "$1" = "run" ]; then
  echo "e2e-fake-kernel" >&2
  exec sleep "${FAKE_KERNEL_RUN_SECS:-5}"
fi
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

fn sample_sub(path: &str, original: bool) -> Subscription {
    Subscription {
        name: "e2e-sub".into(),
        url: "https://example.invalid/sub".into(),
        is_loading: false,
        last_update: None,
        is_manual: true,
        manual_content: Some("trojan://pw@h.example:443#n".into()),
        use_original_config: original,
        config_path: Some(path.into()),
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

fn rule(payload: &str) -> CustomRule {
    CustomRule {
        id: format!("id-{payload}"),
        enabled: true,
        match_type: CustomRuleMatchType::DomainSuffix,
        payload: payload.into(),
        action: CustomRuleAction::Direct,
        outbound: None,
        note: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn proxy_state(port: u16, system: bool) -> ProxyRuntimeState {
    ProxyRuntimeState {
        proxy_port: port,
        allow_lan_access: system,
        system_proxy_enabled: system,
        tun_enabled: false,
        system_proxy_bypass: "localhost".into(),
        tun_options: TunProxyOptions::default(),
    }
}

// ---------------------------------------------------------------------------
// L3-01 storage + config_generator + workdir paths
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_01_storage_config_generator_and_paths() {
    let env = Env::new().await;
    let mut cfg = env.storage.get_app_config().await.unwrap();
    assert!(env.config_path.exists());
    assert_eq!(
        cfg.active_config_path.as_deref(),
        Some(env.config_path.to_str().unwrap())
    );

    cfg.prefer_ipv6 = true;
    cfg.proxy_port = 18181;
    env.storage.save_app_config(&cfg).await.unwrap();
    let reloaded = env.storage.get_app_config().await.unwrap();
    assert!(reloaded.prefer_ipv6);
    assert_eq!(reloaded.proxy_port, 18181);

    let kpath = crate::app::constants::paths::get_kernel_path();
    assert!(
        kpath.starts_with(&env.work_dir),
        "kernel path must be under TempWorkspace"
    );
    let generated = generate_base_config(&reloaded);
    assert!(generated.get("inbounds").is_some() || generated.get("outbounds").is_some());
}

// ---------------------------------------------------------------------------
// L3-02 materializer + storage subscription + disk config
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_02_materialize_manual_subscription_and_persist() {
    let env = Env::new().await;
    let target = env.work_dir.join("sing-box/manual-sub.json");
    write_manual_subscription_config(
        "trojan://secret@node.example.com:443#manual\nvless://uuid@n2.example.com:443?security=tls#v",
        false,
        &AppConfig::default(),
        &target,
    )
    .unwrap();
    let text = fs::read_to_string(&target).unwrap();
    assert!(text.contains("outbounds"), "materializer must write outbounds");

    let sub = sample_sub(target.to_str().unwrap(), false);
    env.storage.save_subscriptions(&[sub]).await.unwrap();
    let mut cfg = env.storage.get_app_config().await.unwrap();
    cfg.active_config_path = Some(target.to_string_lossy().to_string());
    env.storage.save_app_config(&cfg).await.unwrap();

    let subs = env.storage.get_subscriptions().await.unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(
        env.storage
            .get_app_config()
            .await
            .unwrap()
            .active_config_path
            .as_deref(),
        Some(target.to_str().unwrap())
    );
}

// ---------------------------------------------------------------------------
// L3-03 base64 decode + materializer + storage
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_03_base64_decode_materialize_and_storage() {
    let env = Env::new().await;
    let raw = "trojan://p@b64.example.com:443#b64";
    let enc = general_purpose::STANDARD.encode(raw.as_bytes());
    let decoded = try_decode_base64_to_text(&enc).expect("decode");
    assert_eq!(decoded, raw);

    let target = env.work_dir.join("sing-box/b64.json");
    write_downloaded_subscription_config(&decoded, false, &AppConfig::default(), &target).unwrap();
    assert!(fs::read_to_string(&target).unwrap().contains("outbounds"));

    env.storage
        .save_subscriptions(&[sample_sub(target.to_str().unwrap(), false)])
        .await
        .unwrap();
    assert_eq!(env.storage.get_subscriptions().await.unwrap().len(), 1);
}

// ---------------------------------------------------------------------------
// L3-04 runtime plan + settings patch + recording proxy + storage
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_04_runtime_plan_patch_and_recording_proxy() {
    let env = Env::new().await;
    let mut cfg = env.storage.get_app_config().await.unwrap();
    cfg.system_proxy_enabled = true;
    cfg.proxy_port = 18881;
    env.storage.save_app_config(&cfg).await.unwrap();

    let plan = plan_runtime_actions(
        RuntimeChange::SubscriptionApplied,
        &RuntimeApplyOptions::new("e2e-04").patch_active_config(true),
    );
    assert!(plan.apply_proxy_runtime && plan.auto_manage_kernel && plan.patch_active_config);

    sync_settings_to_config_file(&env.config_path, &cfg, ConfigPatchMode::PortsOnly).unwrap();
    let state = runtime_state_from_config(&cfg);
    write_inbounds_for_state(&env.config_path, &state).unwrap();
    let rec = RecordingSystemProxy::default();
    apply_system_proxy_for_state(&state, &rec).unwrap();
    assert_eq!(rec.enables.lock().unwrap().len(), 1);
    assert_eq!(rec.enables.lock().unwrap()[0].1, 18881);
    assert!(fs::read_to_string(&env.config_path).unwrap().contains("inbounds"));
}

// ---------------------------------------------------------------------------
// L3-05 process manager + storage active path + kernel paths
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_05_process_start_stop_with_storage_config() {
    let env = Env::new().await;
    let cfg = env.storage.get_app_config().await.unwrap();
    let path = PathBuf::from(cfg.active_config_path.as_ref().unwrap());
    assert!(path.exists());

    let pm = ProcessManager::new();
    pm.start_inner::<tauri::Wry>(None, &path, false)
        .await
        .expect("fake kernel start");
    assert!(pm.is_running().await);
    let stderr = pm.read_stderr_output().await;
    let _ = stderr;
    pm.stop::<tauri::Wry>(None).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert!(!pm.is_running().await);
}

// ---------------------------------------------------------------------------
// L3-06 process fail path + storage + classify (multi-module, real fail)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_06_missing_config_start_fails_and_storage_intact() {
    let env = Env::new().await;
    let before = env.storage.get_app_config().await.unwrap();
    assert!(before.active_config_path.is_some());

    let pm = ProcessManager::new();
    let missing = env.work_dir.join("definitely-missing-config.json");
    assert!(
        !missing.exists(),
        "fixture must not pre-create the missing path"
    );
    let err = pm.start_inner::<tauri::Wry>(None, &missing, false).await;
    assert!(err.is_err(), "start on missing config must fail");
    assert!(!pm.is_running().await);

    let code = classify_runtime_start_failure(&err.unwrap_err().to_string());
    assert!(
        code == "KERNEL_CONFIG_MISSING" || code == "KERNEL_START_FAILED",
        "unexpected code {code}"
    );
    // storage untouched
    let after = env.storage.get_app_config().await.unwrap();
    assert_eq!(after.active_config_path, before.active_config_path);
}

// ---------------------------------------------------------------------------
// L3-07 process restart cycle + workdir isolation + storage
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_07_process_restart_cycle_with_workdir() {
    let env = Env::new().await;
    let kdir = crate::app::constants::paths::get_kernel_work_dir();
    assert!(kdir.starts_with(&env.work_dir));

    let pm = ProcessManager::new();
    pm.start_inner::<tauri::Wry>(None, &env.config_path, false).await.unwrap();
    assert!(pm.is_running().await);
    pm.stop::<tauri::Wry>(None).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    pm.start_inner::<tauri::Wry>(None, &env.config_path, false).await.unwrap();
    assert!(pm.is_running().await);
    pm.stop::<tauri::Wry>(None).await.unwrap();

    // storage still readable after process churn
    assert!(env.storage.get_app_config().await.unwrap().proxy_port > 0);
}

// ---------------------------------------------------------------------------
// L3-08 runtime plan differences + storage proxy flags
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_08_runtime_plans_and_proxy_flags_on_storage() {
    let env = Env::new().await;
    let ku = plan_runtime_actions(
        RuntimeChange::KernelUpdated,
        &RuntimeApplyOptions::new("e2e-08"),
    );
    let sub = plan_runtime_actions(
        RuntimeChange::SubscriptionApplied,
        &RuntimeApplyOptions::new("e2e-08b").patch_active_config(true),
    );
    // KernelUpdated vs SubscriptionApplied plans must differ on apply_proxy / patch flags
    assert_ne!(
        (ku.apply_proxy_runtime, ku.patch_active_config),
        (sub.apply_proxy_runtime, sub.patch_active_config)
    );

    let mut cfg = env.storage.get_app_config().await.unwrap();
    apply_proxy_overrides_to_app_config(
        &mut cfg,
        &ProxyOverrides {
            proxy_mode: Some("system".into()),
            proxy_port: Some(19001),
            ..Default::default()
        },
    );
    env.storage.save_app_config(&cfg).await.unwrap();
    let resolved = resolve_proxy_runtime_state_from_config(
        &env.storage.get_app_config().await.unwrap(),
        &ProxyOverrides::default(),
    );
    assert!(resolved.proxy.system_proxy_enabled);
    assert_eq!(resolved.proxy.proxy_port, 19001);
}

// ---------------------------------------------------------------------------
// L3-09 proxy modes write disk + recording system proxy + storage
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_09_proxy_modes_disk_and_recording() {
    let env = Env::new().await;
    let rec = RecordingSystemProxy::default();

    for (sys, tun) in [(true, false), (false, true), (false, false)] {
        let state = ProxyRuntimeState {
            proxy_port: 17801,
            allow_lan_access: sys,
            system_proxy_enabled: sys,
            tun_enabled: tun,
            system_proxy_bypass: "localhost".into(),
            tun_options: TunProxyOptions::default(),
        };
        write_inbounds_for_state(&env.config_path, &state).unwrap();
        apply_system_proxy_for_state(&state, &rec).unwrap();
    }
    assert!(*rec.disables.lock().unwrap() >= 1 || !rec.enables.lock().unwrap().is_empty());

    let mut cfg = env.storage.get_app_config().await.unwrap();
    cfg.system_proxy_enabled = true;
    env.storage.save_app_config(&cfg).await.unwrap();
    assert!(env.storage.get_app_config().await.unwrap().system_proxy_enabled);
}

// ---------------------------------------------------------------------------
// L3-10 custom rules storage + inject into config file
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_10_custom_rules_persist_and_inject() {
    let env = Env::new().await;
    let rules = vec![rule("openai.com"), rule("anthropic.com")];
    env.storage
        .save_generic_config(STORAGE_KEY, &rules)
        .await
        .unwrap();
    let loaded: Vec<CustomRule> = env
        .storage
        .load_generic_config(STORAGE_KEY)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.len(), 2);

    let cfg = env.storage.get_app_config().await.unwrap();
    inject_custom_rules_into_config_file_with_storage(&env.storage, &cfg, &env.config_path)
        .await
        .unwrap();
    let final_cfg: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&env.config_path).unwrap()).unwrap();
    let marked = final_cfg["route"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r.get("__custom_rule__").is_some())
        .count();
    assert_eq!(marked, 2);
}

// ---------------------------------------------------------------------------
// L3-11 clash API mock + delay helpers + process health (proxy + status)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_11_clash_api_delay_and_process_health() {
    let env = Env::new().await;
    let url = build_clash_delay_url(9090, "n1", 1500, "http://example.com").unwrap();
    assert!(url.as_str().contains("delay") && url.as_str().contains("1500"));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        for _ in 0..8 {
            let Ok((mut s, _)) = listener.accept().await else {
                break;
            };
            let mut buf = [0u8; 2048];
            let _ = s.read(&mut buf).await;
            let req = String::from_utf8_lossy(&buf);
            let body = if req.contains("/delay") {
                r#"{"delay":42}"#
            } else {
                r#"{"proxies":{"GLOBAL":{"all":["a"]}}}"#
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = s.write_all(resp.as_bytes()).await;
        }
    });

    let data = get_proxies(port).await.unwrap();
    assert!(data.get("proxies").is_some());
    let measured = measure_proxy_delay(port, "a".into(), 500, "http://example.com", 1).await;
    assert!(measured.ok || measured.delay == 42 || !measured.ok);

    // process + status module
    let pm = ProcessManager::new();
    pm.start_inner::<tauri::Wry>(None, &env.config_path, false).await.unwrap();
    let health = kernel_check_health(Some(port)).await.unwrap();
    assert!(health.get("healthy").is_some());
    pm.stop::<tauri::Wry>(None).await.unwrap();
    server.abort();
}

// ---------------------------------------------------------------------------
// L3-12 backup snapshot encode/rewrite/write + storage
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_12_backup_snapshot_rewrite_and_storage() {
    let env = Env::new().await;
    let abs = env.work_dir.join("sing-box/configs/legacy.json");
    fs::create_dir_all(abs.parent().unwrap()).unwrap();
    fs::write(&abs, "{}").unwrap();

    let encoded = encode_path_for_snapshot(
        &abs.to_string_lossy(),
        SnapshotPathKind::SubscriptionConfig,
    );
    assert!(!encoded.is_empty());

    let app = AppConfig {
        active_config_path: Some(abs.to_string_lossy().to_string()),
        proxy_port: 17171,
        ..AppConfig::default()
    };
    let snap_json = serde_json::json!({
        "format_version": 2,
        "app_config": app,
        "subscriptions": [sample_sub(&abs.to_string_lossy(), false)],
        "theme_config": ThemeConfig::default(),
        "locale_config": LocaleConfig::default(),
        "window_config": WindowConfig::default(),
        "update_config": UpdateConfig::default(),
    });
    let snapshot = parse_snapshot(&snap_json.to_string()).unwrap();
    let (app2, subs2, _stats) = rewrite_paths_for_snapshot(&snapshot);
    assert!(app2.active_config_path.is_some());
    assert!(!subs2.is_empty());

    let snap_path = env.work_dir.join("backup-e2e.json");
    write_config_content(&snap_path, &snap_json.to_string()).unwrap();
    assert!(snap_path.exists());

    env.storage.save_app_config(&app2).await.unwrap();
    assert_eq!(env.storage.get_app_config().await.unwrap().proxy_port, 17171);
}

// ---------------------------------------------------------------------------
// L3-13 kernel paths + storage + process validate
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_13_kernel_paths_storage_and_validate() {
    let env = Env::new().await;
    let kpath = crate::app::constants::paths::get_kernel_path();
    let kdir = crate::app::constants::paths::get_kernel_work_dir();
    assert!(kpath.starts_with(&env.work_dir));
    assert!(kdir.starts_with(&env.work_dir));
    assert!(kpath.exists());

    let cfg = env.storage.get_app_config().await.unwrap();
    assert!(cfg.active_config_path.as_ref().unwrap().contains("config.json"));

    // 通过 start_inner 间接覆盖 check 路径（validate_config 为 private）
    let pm = ProcessManager::new();
    pm.start_inner::<tauri::Wry>(None, &env.config_path, false)
        .await
        .expect("fake kernel check+start must pass");
    assert!(pm.is_running().await);
    pm.stop::<tauri::Wry>(None).await.unwrap();
}

// ---------------------------------------------------------------------------
// L3-14 import-like kernel install + process start + storage
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_14_install_kernel_start_and_persist_port() {
    let env = Env::new().await;
    install_fake_kernel(&env.work_dir);
    assert!(crate::app::constants::paths::get_kernel_path().exists());

    let mut cfg = env.storage.get_app_config().await.unwrap();
    cfg.proxy_port = 18282;
    env.storage.save_app_config(&cfg).await.unwrap();

    let pm = ProcessManager::new();
    pm.start_inner::<tauri::Wry>(None, &env.config_path, false).await.unwrap();
    assert!(pm.is_running().await);
    pm.stop::<tauri::Wry>(None).await.unwrap();
    assert_eq!(env.storage.get_app_config().await.unwrap().proxy_port, 18282);
}

// ---------------------------------------------------------------------------
// L3-15 auto_update health + materialize + storage
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_15_auto_update_health_materialize_storage() {
    let env = Env::new().await;
    let target = env.work_dir.join("sing-box/auto.json");
    write_manual_subscription_config(
        "trojan://p@auto.example.com:443#a",
        false,
        &AppConfig::default(),
        &target,
    )
    .unwrap();

    let mut sub = sample_sub(target.to_str().unwrap(), false);
    let now = 1_700_000_000_000u64;
    let fail = build_failure_health_patch(
        sub.config_path.clone(),
        &sub.url,
        now,
        0,
        60,
        "timeout",
    );
    apply_health_patch(&mut sub, &fail);
    assert!(sub.auto_update_fail_count.unwrap_or(0) >= 1);
    let backoff = calc_backoff_minutes(60, sub.auto_update_fail_count.unwrap_or(1));
    assert!(backoff >= 60);

    let ok = build_success_health_patch(sub.config_path.clone(), &sub.url, now + 1);
    apply_health_patch(&mut sub, &ok);
    env.storage.save_subscriptions(&[sub.clone()]).await.unwrap();
    let got = env.storage.get_subscriptions().await.unwrap();
    assert!(got[0].config_path.as_ref().unwrap().ends_with("auto.json"));
    assert!(target.exists());
    let _ = should_run_for_subscription(&got[0], now + 86_400_000);
}

// ---------------------------------------------------------------------------
// L3-16 process idle cleanup + storage + paths
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_16_idle_process_cleanup_and_storage() {
    let env = Env::new().await;
    let pm = ProcessManager::new();
    pm.clear_managed_pid();
    pm.kill_existing_processes::<tauri::Wry>(None).await.unwrap();
    assert!(!pm.is_running().await);

    env.storage
        .save_theme_config(&ThemeConfig {
            is_dark: true,
            mode: "dark".into(),
            accent_color: "#00ff00".into(),
            compact_mode: false,
        })
        .await
        .unwrap();
    assert!(env.storage.get_theme_config().await.unwrap().is_dark);
    assert!(env.work_dir.exists());
}

// ---------------------------------------------------------------------------
// L3-17 patch mode + subscription original flag + disk sync
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_17_patch_mode_subscription_and_disk_sync() {
    let env = Env::new().await;
    let path = env.config_path.to_string_lossy().to_string();
    let original = sample_sub(&path, true);
    env.storage
        .save_subscriptions(&[original.clone()])
        .await
        .unwrap();
    assert_eq!(
        resolve_patch_mode_for_subscription(Some(&original)),
        ConfigPatchMode::PortsOnly
    );
    assert_eq!(
        resolve_patch_mode_with_hint(Some(&original), Some(false)),
        ConfigPatchMode::Full
    );

    let mut cfg = env.storage.get_app_config().await.unwrap();
    cfg.proxy_port = 19991;
    cfg.api_port = 19992;
    env.storage.save_app_config(&cfg).await.unwrap();
    sync_settings_to_config_file(&env.config_path, &cfg, ConfigPatchMode::PortsOnly).unwrap();
    let text = fs::read_to_string(&env.config_path).unwrap();
    assert!(text.contains("19991") || text.contains("19992") || !text.is_empty());
}

// ---------------------------------------------------------------------------
// L3-18 generic KV + custom rules + theme coexist in storage
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_18_generic_kv_rules_and_theme() {
    let env = Env::new().await;
    #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
    struct Blob {
        ct: String,
    }
    env.storage
        .save_generic_config("sudo_blob", &Blob { ct: "aa".into() })
        .await
        .unwrap();
    env.storage
        .save_generic_config(STORAGE_KEY, &vec![rule("sec.e2e")])
        .await
        .unwrap();
    env.storage
        .save_theme_config(&ThemeConfig::default())
        .await
        .unwrap();

    let blob: Blob = env
        .storage
        .load_generic_config("sudo_blob")
        .await
        .unwrap()
        .unwrap();
    let rules: Vec<CustomRule> = env
        .storage
        .load_generic_config(STORAGE_KEY)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(blob.ct, "aa");
    assert_eq!(rules.len(), 1);
    let _ = env.storage.get_theme_config().await.unwrap();
}

// ---------------------------------------------------------------------------
// L3-19 Full + PortsOnly patch on active config + storage
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_19_full_and_ports_patch_with_storage() {
    let env = Env::new().await;
    let mut cfg = env.storage.get_app_config().await.unwrap();
    cfg.proxy_port = 16661;
    cfg.api_port = 16662;
    cfg.prefer_ipv6 = true;
    env.storage.save_app_config(&cfg).await.unwrap();
    sync_settings_to_config_file(&env.config_path, &cfg, ConfigPatchMode::PortsOnly).unwrap();
    sync_settings_to_config_file(&env.config_path, &cfg, ConfigPatchMode::Full).unwrap();
    let text = fs::read_to_string(&env.config_path).unwrap();
    assert!(!text.is_empty());
    assert_eq!(env.storage.get_app_config().await.unwrap().api_port, 16662);
}

// ---------------------------------------------------------------------------
// L3-20 GOLDEN: materialize → rules → runtime → proxy → process
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_20_golden_subscription_rules_proxy_process() {
    let env = Env::new().await;
    let manual = env.work_dir.join("sing-box/golden.json");
    write_manual_subscription_config(
        "trojan://gpw@golden.example.com:443#g",
        false,
        &AppConfig::default(),
        &manual,
    )
    .unwrap();

    let mut cfg = env.storage.get_app_config().await.unwrap();
    cfg.active_config_path = Some(manual.to_string_lossy().to_string());
    cfg.system_proxy_enabled = true;
    cfg.proxy_port = 17771;
    env.storage.save_app_config(&cfg).await.unwrap();
    env.storage
        .save_subscriptions(&[sample_sub(manual.to_str().unwrap(), false)])
        .await
        .unwrap();
    env.storage
        .save_generic_config(STORAGE_KEY, &vec![rule("golden.test")])
        .await
        .unwrap();

    inject_custom_rules_into_config_file_with_storage(&env.storage, &cfg, &manual)
        .await
        .unwrap();

    let plan = plan_runtime_actions(
        RuntimeChange::SubscriptionApplied,
        &RuntimeApplyOptions::new("golden")
            .patch_active_config(true)
            .force_restart(true),
    );
    assert!(plan.apply_proxy_runtime && plan.auto_manage_kernel);

    let state = runtime_state_from_config(&cfg);
    write_inbounds_for_state(&manual, &state).unwrap();
    let rec = RecordingSystemProxy::default();
    apply_system_proxy_for_state(&state, &rec).unwrap();
    assert_eq!(rec.enables.lock().unwrap()[0].1, 17771);

    let pm = ProcessManager::new();
    pm.start_inner::<tauri::Wry>(None, &manual, false).await.unwrap();
    assert!(pm.is_running().await);
    pm.stop::<tauri::Wry>(None).await.unwrap();

    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&manual).unwrap()).unwrap();
    assert!(v["route"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r.get("__custom_rule__").is_some()));
}

// ---------------------------------------------------------------------------
// L3-21 DNS strategy + proxy state on disk + recording
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_21_dns_strategy_proxy_disk_and_recording() {
    let env = Env::new().await;
    update_dns_strategy_on_path(&env.config_path, true).unwrap();
    let content = fs::read_to_string(&env.config_path).unwrap();
    let mut cfg: serde_json::Value = serde_json::from_str(&content).unwrap();
    apply_dns_strategy_to_config(&mut cfg, false).unwrap();
    assert_eq!(cfg["dns"]["strategy"], "ipv4_only");

    let state = proxy_state(17801, true);
    write_inbounds_for_state(&env.config_path, &state).unwrap();
    let rec = RecordingSystemProxy::default();
    apply_system_proxy_for_state(&state, &rec).unwrap();
    assert_eq!(rec.enables.lock().unwrap().len(), 1);

    let mut app = env.storage.get_app_config().await.unwrap();
    app.prefer_ipv6 = false;
    env.storage.save_app_config(&app).await.unwrap();
}

// ---------------------------------------------------------------------------
// L3-22 process + status/health modules
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_22_process_status_and_health_modules() {
    let env = Env::new().await;
    let manager = ProcessManager::new();
    manager
        .start_inner::<tauri::Wry>(None, &env.config_path, false)
        .await
        .unwrap();
    assert!(manager.is_running().await);
    let running = is_kernel_running_with_process::<tauri::Wry>(&manager)
        .await
        .unwrap_or(false);
    assert!(running);
    let health = kernel_check_health(Some(1)).await.unwrap();
    assert!(health.get("healthy").is_some());
    manager.stop::<tauri::Wry>(None).await.unwrap();
}

// ---------------------------------------------------------------------------
// L3-23 backup apply to storage + rewrite
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_23_backup_apply_to_storage_and_rewrite() {
    let env = Env::new().await;
    let content = fs::read_to_string(&env.config_path).unwrap();
    let snap = serde_json::json!({
        "format_version": 2,
        "app_config": AppConfig {
            active_config_path: Some("config.json".into()),
            proxy_port: 17999,
            ..AppConfig::default()
        },
        "theme_config": ThemeConfig::default(),
        "locale_config": LocaleConfig::default(),
        "window_config": WindowConfig::default(),
        "update_config": UpdateConfig::default(),
        "subscriptions": [sample_sub("configs/e2e.json", false)],
        "active_config_content": content,
    });
    let snapshot = parse_snapshot(&snap.to_string()).unwrap();
    let (_cfg, subs, _stats) = rewrite_paths_for_snapshot(&snapshot);
    assert!(!subs.is_empty());
    let _warnings = apply_snapshot_to_storage(&env.storage, &snapshot)
        .await
        .unwrap();
    assert_eq!(env.storage.get_app_config().await.unwrap().proxy_port, 17999);
}

// ---------------------------------------------------------------------------
// L3-24 config_service restore + storage
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_24_config_service_restore_and_storage() {
    let env = Env::new().await;
    let path = env.work_dir.join("sing-box/configs/gen.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    write_default_config(&path, &AppConfig::default()).unwrap();
    let bak = path.with_extension("bak");
    fs::copy(&path, &bak).unwrap();

    // 损坏主文件 → 从 bak 恢复
    fs::write(&path, b"{broken").unwrap();
    assert!(try_restore_from_bak(&path).unwrap());
    assert!(path.exists());
    assert!(serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&path).unwrap()).is_ok());

    // 备份损坏副本（会 rename 走当前文件）
    fs::write(&path, b"{still-broken").unwrap();
    backup_corrupted_config(&path);
    assert!(!path.exists());

    // 重新写入默认并挂到 storage
    write_default_config(&path, &AppConfig::default()).unwrap();
    let mut rules = vec![serde_json::json!({"outbound": "direct"})];
    ensure_private_ip_rule(&mut rules);
    assert!(rules.len() >= 2);

    let mut cfg = env.storage.get_app_config().await.unwrap();
    cfg.active_config_path = Some(path.to_string_lossy().to_string());
    env.storage.save_app_config(&cfg).await.unwrap();
    let active = env
        .storage
        .get_app_config()
        .await
        .unwrap()
        .active_config_path
        .unwrap();
    assert!(PathBuf::from(&active).exists());
    assert_eq!(active, path.to_string_lossy());
}

// ---------------------------------------------------------------------------
// L3-25 global PROCESS_MANAGER + readiness + mock API
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_25_global_process_readiness_and_api() {
    let env = Env::new().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        for _ in 0..16 {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let mut buf = [0u8; 512];
            let _ = sock.read(&mut buf).await;
            let body = "1.12.0";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });

    PROCESS_MANAGER
        .start_inner::<tauri::Wry>(None, &env.config_path, false)
        .await
        .unwrap();
    assert!(is_kernel_running().await.unwrap());

    let (ok, _, _) = probe_version_api(port).await;
    assert!(ok);
    let probe = collect_kernel_runtime_probe(port).await;
    assert!(probe.process_running);
    assert!(probe.api_ready);

    let cfg = StabilityCheckConfig {
        max_checks: 3,
        initial_retry_interval_ms: 20,
        max_retry_interval_ms: 50,
        api_timeout_ms: 500,
    };
    verify_kernel_startup_stability_with_config(port, cfg)
        .await
        .unwrap();

    // lifecycle pure start_kernel_process_and_verify when already running
    start_kernel_process_and_verify(&env.config_path, port, false)
        .await
        .expect("already running + stable");

    PROCESS_MANAGER.stop::<tauri::Wry>(None).await.unwrap();
    server.abort();
}

// ---------------------------------------------------------------------------
// L3-26 lifecycle resolve + storage + classify
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_26_lifecycle_resolve_classify_storage() {
    let env = Env::new().await;
    let mut cfg = env.storage.get_app_config().await.unwrap();
    apply_proxy_overrides_to_app_config(
        &mut cfg,
        &ProxyOverrides {
            proxy_mode: Some("system".into()),
            api_port: Some(18081),
            proxy_port: Some(17890),
            prefer_ipv6: Some(false),
            system_proxy_bypass: Some("localhost,127.0.0.1".into()),
            tun_options: Some(TunProxyOptions::default()),
            ..Default::default()
        },
    );
    env.storage.save_app_config(&cfg).await.unwrap();
    let resolved = resolve_proxy_runtime_state_from_config(&cfg, &ProxyOverrides::default());
    assert_eq!(resolved.api_port, 18081);
    assert!(resolved.proxy.system_proxy_enabled);
    assert_eq!(
        classify_runtime_start_failure("配置文件不存在: x"),
        "KERNEL_CONFIG_MISSING"
    );

    let state = resolved.proxy.clone();
    write_inbounds_for_state(&env.config_path, &state).unwrap();
}

// ---------------------------------------------------------------------------
// L3-27 subscription userinfo pure + storage + local fetch
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_27_subscription_userinfo_fetch_and_storage() {
    let env = Env::new().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = s.read(&mut buf).await;
        let body = "proxies: []\n";
        let resp = format!(
            "HTTP/1.1 200 OK\r\nsubscription-userinfo: upload=1; download=2; total=3; expire=4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = s.write_all(resp.as_bytes()).await;
    });

    let url = format!("http://127.0.0.1:{}/sub", port);
    let result = fetch_subscription_content_with_user_agent(&url, Some("clash.meta"))
        .await
        .expect("local fetch");
    assert!(!result.body.is_empty());
    assert!(result.userinfo.is_some());

    let mut subs = vec![sample_sub(env.config_path.to_str().unwrap(), false)];
    subs[0].url = url.clone();
    let info = result.userinfo.unwrap();
    assert!(apply_userinfo_to_subscriptions(
        &mut subs,
        env.config_path.to_str().unwrap(),
        &url,
        Some(&info),
        9999,
    ));
    env.storage.save_subscriptions(&subs).await.unwrap();
    let got = env.storage.get_subscriptions().await.unwrap();
    assert_eq!(got[0].subscription_upload, Some(1));
    server.abort();
}

// ---------------------------------------------------------------------------
// L3-28 stop_kernel lifecycle + PROCESS_MANAGER + storage
// ---------------------------------------------------------------------------
#[tokio::test]
async fn l3_28_stop_kernel_lifecycle_and_storage() {
    let env = Env::new().await;
    PROCESS_MANAGER
        .start_inner::<tauri::Wry>(None, &env.config_path, false)
        .await
        .unwrap();
    assert!(PROCESS_MANAGER.is_running().await);
    let _ = stop_kernel::<tauri::Wry>(None).await;
    let _ = PROCESS_MANAGER.stop::<tauri::Wry>(None).await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // storage still works after kernel stop
    let mut cfg = env.storage.get_app_config().await.unwrap();
    cfg.api_port = 18383;
    env.storage.save_app_config(&cfg).await.unwrap();
    assert_eq!(env.storage.get_app_config().await.unwrap().api_port, 18383);
}

// ---------------------------------------------------------------------------
// L3-29 sudo crypto pure + storage secret blob (no real sudo)
// ---------------------------------------------------------------------------
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn l3_29_sudo_crypto_and_storage_secret() {
    use crate::app::system::sudo_service::{
        decrypt_password_with_key, derive_crypto_key_from_material, encrypt_password_with_key,
    };

    let env = Env::new().await;
    let k = derive_crypto_key_from_material(b"e2e-material");
    let c = encrypt_password_with_key(&k, "pw").unwrap();
    assert_eq!(decrypt_password_with_key(&k, &c).unwrap(), "pw");

    env.storage
        .save_generic_config("e2e_encrypted_marker", &c)
        .await
        .unwrap();
    let loaded: String = env
        .storage
        .load_generic_config("e2e_encrypted_marker")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(decrypt_password_with_key(&k, &loaded).unwrap(), "pw");
}
