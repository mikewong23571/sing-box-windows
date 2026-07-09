use super::*;
use crate::app::core::tun_profile::TunProxyOptions;
use crate::app::singbox::config_generator::{generate_base_config, inject_custom_rules, strip_custom_rules};
use crate::app::storage::custom_rule::{CustomRule, CustomRuleAction, CustomRuleMatchType};
use crate::app::storage::state_model::AppConfig;
use chrono::Utc;
use std::fs;
use std::sync::Arc;

fn sample_state(system: bool, tun: bool, lan: bool) -> ProxyRuntimeState {
    ProxyRuntimeState {
        proxy_port: 17890,
        allow_lan_access: lan,
        system_proxy_enabled: system,
        tun_enabled: tun,
        system_proxy_bypass: DEFAULT_BYPASS_LIST.to_string(),
        tun_options: TunProxyOptions::default(),
    }
}

#[test]
fn derived_mode_and_listen_address() {
    assert_eq!(sample_state(false, true, false).derived_mode(), "tun");
    assert_eq!(sample_state(true, false, false).derived_mode(), "system");
    assert_eq!(sample_state(false, false, false).derived_mode(), "manual");

    assert_eq!(
        resolve_proxy_listen_address(&sample_state(false, false, true)),
        network_config::DEFAULT_LISTEN_ADDRESS
    );
    assert_eq!(
        resolve_proxy_listen_address(&sample_state(false, false, false)),
        network_config::DEFAULT_CLASH_API_ADDRESS
    );
}

#[test]
fn build_inbounds_manual_and_tun() {
    let manual = build_inbounds_for_state(&sample_state(false, false, false));
    assert_eq!(manual.len(), 1);
    assert_eq!(manual[0].listen_port, Some(17890));
    assert_eq!(manual[0].set_system_proxy, Some(false));

    let tun = build_inbounds_for_state(&sample_state(true, true, false));
    assert!(!tun.is_empty());
    assert_eq!(tun[0].set_system_proxy, Some(true));
}

#[test]
fn write_inbounds_updates_config_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    let base = generate_base_config(&AppConfig::default());
    fs::write(&path, serde_json::to_string_pretty(&base).unwrap()).unwrap();

    write_inbounds_for_state(&path, &sample_state(true, false, true)).unwrap();
    let updated: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let inbounds = updated["inbounds"].as_array().unwrap();
    assert_eq!(inbounds[0]["listen_port"], 17890);
    assert_eq!(inbounds[0]["set_system_proxy"], true);
}

#[test]
fn recording_system_proxy_enable_and_disable() {
    let rec = RecordingSystemProxy::default();
    let mut state = sample_state(true, false, false);
    state.system_proxy_bypass = "  ".into();
    apply_system_proxy_for_state(&state, &rec).unwrap();
    assert_eq!(rec.enables.lock().unwrap().len(), 1);
    assert_eq!(rec.enables.lock().unwrap()[0].1, 17890);

    state.system_proxy_enabled = false;
    apply_system_proxy_for_state(&state, &rec).unwrap();
    assert_eq!(*rec.disables.lock().unwrap(), 1);
}

#[test]
fn controller_url_and_delay_helpers() {
    assert_eq!(
        build_controller_url(9090, "/proxies"),
        "http://127.0.0.1:9090/proxies"
    );
    assert_eq!(
        build_controller_url(9090, "proxies"),
        "http://127.0.0.1:9090/proxies"
    );

    assert!(normalize_test_url("https://example.com/a").starts_with("https://"));
    assert_eq!(normalize_test_url("ftp://x"), DEFAULT_DELAY_TEST_URL);
    assert_eq!(normalize_test_url("not-a-url"), DEFAULT_DELAY_TEST_URL);

    let url = build_clash_delay_url(12081, "节点 A", 3000, "http://cp.cloudflare.com").unwrap();
    assert!(url.as_str().contains("12081"));
    assert!(url.as_str().contains("delay"));
    assert!(url.as_str().contains("timeout=3000"));

    assert!(median_u64(vec![]).is_none());
    assert_eq!(median_u64(vec![10]).unwrap(), 10);
    assert_eq!(median_u64(vec![1, 3, 2]).unwrap(), 2);

    let id = uuid_v4();
    assert!(id.contains('-'));
    assert_eq!(get_api_token(), "");
}

#[tokio::test]
async fn clash_api_helpers_against_local_mock() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = tokio::spawn(async move {
        for _ in 0..8 {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf).await;
            let req = String::from_utf8_lossy(&buf);
            let body = if req.contains("GET /proxies ") {
                r#"{"proxies":{"GLOBAL":{"all":["a"]}}}"#
            } else if req.contains("PUT /proxies/") {
                "{}"
            } else if req.contains("PATCH /") {
                "{}"
            } else if req.contains("DELETE /connections") {
                "{}"
            } else if req.contains("/delay") {
                r#"{"delay":42}"#
            } else {
                r#"{}"#
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });

    let proxies = get_proxies(port).await.unwrap();
    assert!(proxies.get("proxies").is_some());

    change_proxy("GLOBAL".into(), "a".into(), port).await.unwrap();
    close_all_connections(port).await.unwrap();
    close_connection("id1".into(), port).await.unwrap();

    let delay = fetch_single_delay(port, "a", 1000, "http://example.com").await.unwrap();
    assert_eq!(delay, 42);

    let _ = server.abort();
}

#[test]
fn inject_custom_rules_into_written_config_file_without_app_handle() {
    // 模拟 inject_custom_rules_into_config_file 的 strip+inject 核心
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("active.json");
    let mut config = generate_base_config(&AppConfig::default());
    let rules = vec![CustomRule {
        id: "r1".into(),
        enabled: true,
        match_type: CustomRuleMatchType::DomainSuffix,
        payload: "openai.com".into(),
        action: CustomRuleAction::Proxy,
        outbound: None,
        note: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }];
    inject_custom_rules(&mut config, &rules, "手动选择");
    fs::write(&path, serde_json::to_string_pretty(&config).unwrap()).unwrap();

    let mut loaded: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    strip_custom_rules(&mut loaded);
    inject_custom_rules(&mut loaded, &rules, "手动选择");
    fs::write(&path, serde_json::to_string_pretty(&loaded).unwrap()).unwrap();

    let final_cfg: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let marked = final_cfg["route"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r.get("__custom_rule__").is_some())
        .count();
    assert_eq!(marked, 1);
    let _ = Arc::new(()); // keep Arc import used if optimized
}

#[tokio::test]
async fn inject_custom_rules_with_storage_roundtrip() {
    use crate::app::storage::custom_rule::STORAGE_KEY;
    use crate::app::storage::enhanced_storage_service::EnhancedStorageService;
    use crate::test_support::TempWorkspace;

    let ws = TempWorkspace::new();
    let db = ws.join("rules.db");
    let storage = EnhancedStorageService::from_path(db.to_str().unwrap())
        .await
        .unwrap();

    let rules = vec![CustomRule {
        id: "r-storage".into(),
        enabled: true,
        match_type: CustomRuleMatchType::DomainSuffix,
        payload: "example.org".into(),
        action: CustomRuleAction::Direct,
        outbound: None,
        note: Some("t".into()),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }];
    storage
        .save_generic_config(STORAGE_KEY, &rules)
        .await
        .unwrap();

    let cfg_path = ws.join("cfg.json");
    let base = generate_base_config(&AppConfig::default());
    fs::write(&cfg_path, serde_json::to_string_pretty(&base).unwrap()).unwrap();

    inject_custom_rules_into_config_file_with_storage(&storage, &AppConfig::default(), &cfg_path)
        .await
        .unwrap();

    let final_cfg: Value = serde_json::from_str(&fs::read_to_string(&cfg_path).unwrap()).unwrap();
    let marked = final_cfg["route"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r.get("__custom_rule__").is_some())
        .count();
    assert_eq!(marked, 1);

    // 缺失文件应静默成功
    inject_custom_rules_into_config_file_with_storage(
        &storage,
        &AppConfig::default(),
        &ws.join("missing.json"),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn clash_api_providers_rules_and_group_nodes() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = tokio::spawn(async move {
        for _ in 0..16 {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let mut buf = [0u8; 4096];
            let _ = sock.read(&mut buf).await;
            let req = String::from_utf8_lossy(&buf);
            let body = if req.contains("GET /proxies ") {
                r#"{"proxies":{"GLOBAL":{"all":["n1","n2"]},"n1":{},"n2":{}}}"#
            } else if req.contains("GET /providers/proxies") {
                r#"{"providers":{}}"#
            } else if req.contains("GET /providers/rules") {
                r#"{"providers":{}}"#
            } else if req.contains("GET /rules") {
                r#"{"rules":[]}"#
            } else if req.contains("/delay") {
                r#"{"delay":15}"#
            } else {
                r#"{}"#
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });

    let _ = get_proxy_providers(port).await;
    let _ = get_rule_providers(port).await;
    let rules = get_rules(port).await.unwrap();
    assert!(rules.get("rules").is_some());

    let nodes = resolve_group_nodes(port, "GLOBAL").await.unwrap();
    assert_eq!(nodes, vec!["n1".to_string(), "n2".to_string()]);

    let miss = resolve_group_nodes(port, "NOPE").await;
    assert!(miss.is_err());

    let measured = measure_proxy_delay(port, "n1".into(), 500, "http://example.com", 1).await;
    assert!(measured.ok);
    assert_eq!(measured.delay, 15);

    let _ = update_proxy_provider("p1".into(), port).await;
    let _ = update_rule_provider("r1".into(), port).await;
    let _ = toggle_rule_disabled(0, true, port).await;

    let _ = server.abort();
}

#[test]
fn build_inbounds_system_and_lan_flags() {
    let sys = sample_state(true, false, true);
    let inbounds = build_inbounds_for_state(&sys);
    assert_eq!(inbounds.len(), 1);
    assert_eq!(inbounds[0].set_system_proxy, Some(true));
    assert_eq!(
        inbounds[0].listen.as_deref(),
        Some(crate::app::network_config::DEFAULT_LISTEN_ADDRESS)
    );

    let both = sample_state(true, true, false);
    assert_eq!(both.derived_mode(), "tun");
}

#[test]
fn apply_system_proxy_disable_error_is_swallowed() {
    use std::sync::Mutex;

    struct FailDisable;
    impl SystemProxyPort for FailDisable {
        fn enable(&self, _host: &str, _port: u16, _bypass: Option<&str>) -> Result<(), String> {
            Ok(())
        }
        fn disable(&self) -> Result<(), String> {
            Err("disable failed".into())
        }
    }

    let state = sample_state(false, false, false);
    // disable 失败只 warn，不应 Err
    apply_system_proxy_for_state(&state, &FailDisable).unwrap();
    let _ = Mutex::new(0);
}

#[test]
fn write_inbounds_missing_config_errors() {
    let err = write_inbounds_for_state(
        std::path::Path::new("/tmp/definitely-missing-config-xyz.json"),
        &sample_state(false, false, false),
    );
    assert!(err.is_err());
}

#[test]
fn normalize_and_median_edge_cases() {
    assert_eq!(median_u64(vec![1, 2, 3, 4]).unwrap(), 3);
    assert_eq!(
        normalize_test_url("http://a.example/path"),
        "http://a.example/path"
    );
}

#[test]
fn apply_dns_strategy_to_config_creates_and_updates() {
    let mut cfg = json!({});
    apply_dns_strategy_to_config(&mut cfg, true).unwrap();
    assert_eq!(cfg["dns"]["strategy"], "prefer_ipv6");

    let mut cfg2 = json!({
        "dns": {
            "strategy": "ipv4_only",
            "servers": [
                { "address": "8.8.8.8", "strategy": "ipv4_only" },
                { "type": "rcode", "tag": "block" }
            ]
        }
    });
    apply_dns_strategy_to_config(&mut cfg2, false).unwrap();
    assert_eq!(cfg2["dns"]["strategy"], "ipv4_only");
    assert_eq!(cfg2["dns"]["servers"][0]["strategy"], "ipv4_only");
    assert!(cfg2["dns"]["servers"][1].get("strategy").is_none());
}

#[test]
fn update_dns_strategy_on_path_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.json");
    fs::write(&path, r#"{"log":{"level":"info"}}"#).unwrap();
    update_dns_strategy_on_path(&path, true).unwrap();
    let loaded: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(loaded["dns"]["strategy"], "prefer_ipv6");
    update_dns_strategy_on_path(&path, false).unwrap();
    let loaded2: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(loaded2["dns"]["strategy"], "ipv4_only");
}

#[tokio::test]
async fn clash_api_http_errors() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        for _ in 0..6 {
            let Ok((mut sock, _)) = listener.accept().await else { break; };
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            let resp = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });
    assert!(get_proxies(port).await.is_err());
    assert!(change_proxy("G".into(), "n".into(), port).await.is_err());
    assert!(fetch_single_delay(port, "n", 100, "http://x").await.is_err());
    let failed = measure_proxy_delay(port, "n".into(), 50, "http://x", 1).await;
    assert!(!failed.ok);
    let _ = server.abort();
}

#[tokio::test]
async fn resolve_group_nodes_empty_list_errors() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            let body = r#"{"proxies":{"G":{"all":[]}}}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(), body
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });
    assert!(resolve_group_nodes(port, "G").await.is_err());
    let _ = server.abort();
}

#[test]
fn build_proxy_mode_runtime_states() {
    let sys = build_system_proxy_runtime_state(1080, true, "localhost".into());
    assert!(sys.system_proxy_enabled);
    assert!(!sys.tun_enabled);
    assert_eq!(sys.proxy_port, 1080);
    assert_eq!(sys.system_proxy_bypass, "localhost");
    assert_eq!(sys.derived_mode(), "system");

    let man = build_manual_proxy_runtime_state(1081, false);
    assert!(!man.system_proxy_enabled);
    assert!(!man.tun_enabled);
    assert_eq!(man.derived_mode(), "manual");

    let tun = build_tun_proxy_runtime_state(1082, false, TunProxyOptions::default());
    assert!(tun.tun_enabled);
    assert!(!tun.system_proxy_enabled);
    assert_eq!(tun.derived_mode(), "tun");
}

#[tokio::test]
async fn custom_rules_crud_via_mock_app() {
    use crate::app::singbox::config_generator::generate_base_config;
    use crate::app::storage::state_model::AppConfig;
    use crate::test_support::MockAppEnv;
    use std::fs;

    let env = MockAppEnv::new();
    let cfg_path = env.workspace.path().join("sing-box/config.json");
    fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
    let mut cfg = AppConfig::default();
    cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
    fs::write(
        &cfg_path,
        serde_json::to_string_pretty(&generate_base_config(&cfg)).unwrap(),
    )
    .unwrap();
    let db = env.workspace.path().join("rules_crud.db");
    env.install_storage_from_path(db.to_str().unwrap()).await;
    let storage = get_enhanced_storage(&env.handle()).await.unwrap();
    storage.save_app_config(&cfg).await.unwrap();

    let h = env.handle();
    assert!(list_custom_rules(h.clone()).await.unwrap().is_empty());

    // empty payload rejected
    assert!(add_custom_rule(
        h.clone(),
        CustomRuleMatchType::DomainSuffix,
        "  ".into(),
        CustomRuleAction::Proxy,
        None,
    )
    .await
    .is_err());

    let rule = add_custom_rule(
        h.clone(),
        CustomRuleMatchType::DomainSuffix,
        "openai.com".into(),
        CustomRuleAction::Proxy,
        Some("n".into()),
    )
    .await
    .unwrap();
    assert!(!rule.id.is_empty());

    let listed = list_custom_rules(h.clone()).await.unwrap();
    assert_eq!(listed.len(), 1);

    update_custom_rule(
        h.clone(),
        rule.id.clone(),
        CustomRuleMatchType::Domain,
        "api.openai.com".into(),
        CustomRuleAction::Direct,
        Some("u".into()),
    )
    .await
    .unwrap();

    // empty update payload
    assert!(update_custom_rule(
        h.clone(),
        rule.id.clone(),
        CustomRuleMatchType::Domain,
        "".into(),
        CustomRuleAction::Direct,
        None,
    )
    .await
    .is_err());

    // missing id
    assert!(update_custom_rule(
        h.clone(),
        "no-such".into(),
        CustomRuleMatchType::Domain,
        "x.com".into(),
        CustomRuleAction::Proxy,
        None,
    )
    .await
    .is_err());

    toggle_custom_rule(h.clone(), rule.id.clone()).await.unwrap();
    let after_toggle = list_custom_rules(h.clone()).await.unwrap();
    assert!(!after_toggle[0].enabled);

    assert!(toggle_custom_rule(h.clone(), "missing".into()).await.is_err());

    delete_custom_rule(h.clone(), rule.id.clone()).await.unwrap();
    assert!(list_custom_rules(h.clone()).await.unwrap().is_empty());
    assert!(delete_custom_rule(h.clone(), rule.id).await.is_err());

    // use_original_config 活动订阅：注入应跳过
    let subs = vec![crate::app::storage::state_model::Subscription {
        name: "orig".into(),
        url: "http://example.com".into(),
        is_loading: false,
        last_update: None,
        is_manual: false,
        manual_content: None,
        use_original_config: true,
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
    }];
    storage.save_subscriptions(&subs).await.unwrap();
    let rule2 = add_custom_rule(
        h.clone(),
        CustomRuleMatchType::DomainSuffix,
        "skip.me".into(),
        CustomRuleAction::Proxy,
        None,
    )
    .await
    .unwrap();
    assert!(!rule2.id.is_empty());
}

#[tokio::test]
async fn toggle_ip_version_and_delay_tests_with_mock() {
    use crate::app::singbox::config_generator::generate_base_config;
    use crate::app::storage::state_model::AppConfig;
    use crate::test_support::MockAppEnv;
    use std::fs;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let env = MockAppEnv::new();
    let cfg_path = env.workspace.path().join("sing-box/config.json");
    fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
    let mut cfg = AppConfig::default();
    cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
    cfg.singbox_urltest_url = "https://www.gstatic.com/generate_204".into();
    fs::write(
        &cfg_path,
        serde_json::to_string_pretty(&generate_base_config(&cfg)).unwrap(),
    )
    .unwrap();
    let db = env.workspace.path().join("ip.db");
    env.install_storage_from_path(db.to_str().unwrap()).await;
    get_enhanced_storage(&env.handle())
        .await
        .unwrap()
        .save_app_config(&cfg)
        .await
        .unwrap();

    let h = env.handle();
    toggle_ip_version(h.clone(), true).await.unwrap();
    let v: Value = serde_json::from_str(&fs::read_to_string(&cfg_path).unwrap()).unwrap();
    assert_eq!(v["dns"]["strategy"], "prefer_ipv6");
    toggle_ip_version(h.clone(), false).await.unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        for _ in 0..8 {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf).await;
            let body = r#"{"delay":33}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });

    let one = test_node_delay(h.clone(), "n1".into(), Some("http://example.com".into()), port)
        .await
        .unwrap();
    assert!(one.ok);
    assert_eq!(one.delay, 33);

    let many = test_nodes_delay(
        h.clone(),
        vec!["a".into(), "a".into(), "b".into()],
        Some(DelayTestOptions {
            timeout_ms: Some(200),
            url: Some("http://example.com".into()),
            concurrency: Some(2),
            samples: Some(1),
        }),
        port,
    )
    .await
    .unwrap();
    assert_eq!(many.len(), 2); // 去重

    let _ = test_nodes_delay(h, vec![], None, port).await.unwrap();
    let _ = server.abort();
}

// 需要 get_enhanced_storage 在 tests 模块可见
use crate::app::storage::enhanced_storage_service::get_enhanced_storage;
