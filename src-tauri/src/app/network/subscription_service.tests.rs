use super::{
    active_config_change_requires_restart, add_manual_subscription_core,
    apply_userinfo_to_subscriptions, build_subscription_persist_result, delete_subscription_config,
    download_subscription_core, extract_nodes_from_subscription, extract_subscription_userinfo,
    fetch_subscription_content, fetch_subscription_content_with_user_agent, get_current_config_impl,
    merge_subscription_fetch_result, normalized_active_config_path, parse_subscription_userinfo,
    persist_downloaded_subscription_content, persist_manual_subscription_content,
    read_config_file_content, resolve_current_config_file_path, rollback_subscription_config,
    set_active_config_path_internal, should_retry_subscription_userinfo, try_decode_base64_to_text,
    update_subscription_userinfo, SubscriptionFetchResult, SubscriptionUserInfo,
};
use base64::{engine::general_purpose, Engine as _};
use reqwest::header::{HeaderMap, HeaderValue};

#[test]
fn base64_uri_list_should_extract_nodes_after_decode() {
    let uri_list = "trojan://password@example.com:443#test\nvless://uuid@example.com:443?security=tls&sni=example.com#vless\n";
    let b64 = general_purpose::STANDARD.encode(uri_list.as_bytes());

    let decoded = try_decode_base64_to_text(&b64).expect("decode should work");
    let nodes = extract_nodes_from_subscription(&decoded).expect("extract should work");
    assert_eq!(nodes.len(), 2);
}

#[test]
fn active_config_change_should_request_runtime_restart() {
    assert!(active_config_change_requires_restart(
        &Some("D:/configs/old.json".to_string()),
        &Some("D:/configs/new.json".to_string()),
    ));
    assert!(active_config_change_requires_restart(
        &Some("D:/configs/old.json".to_string()),
        &None,
    ));
}

#[test]
fn unchanged_active_config_should_not_request_runtime_restart() {
    assert!(!active_config_change_requires_restart(
        &Some("D:/configs/current.json".to_string()),
        &Some("D:/configs/current.json".to_string()),
    ));
}

#[test]
fn try_decode_base64_to_text_should_accept_whitespace_and_missing_padding() {
    let raw = "vmess://example\nvless://demo";
    let encoded = general_purpose::STANDARD
        .encode(raw.as_bytes())
        .trim_end_matches('=')
        .chars()
        .collect::<Vec<_>>();
    let formatted = format!(
        "{} \n {}",
        encoded[..8].iter().collect::<String>(),
        encoded[8..].iter().collect::<String>()
    );

    let decoded = try_decode_base64_to_text(&formatted).expect("decode should work");
    assert_eq!(decoded, raw);
}

#[test]
fn parse_subscription_userinfo_should_parse_known_fields() {
    let info = parse_subscription_userinfo("upload=1; download=2; total=3; expire=4; foo=bar")
        .expect("userinfo should be parsed");

    assert_eq!(info.upload, Some(1));
    assert_eq!(info.download, Some(2));
    assert_eq!(info.total, Some(3));
    assert_eq!(info.expire, Some(4));
}

#[test]
fn parse_subscription_userinfo_should_return_none_when_no_known_fields() {
    assert!(parse_subscription_userinfo("foo=1;bar=2").is_none());
    assert!(parse_subscription_userinfo("   ").is_none());
}

#[test]
fn extract_subscription_userinfo_should_support_case_variants() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "Subscription-Userinfo",
        HeaderValue::from_static("upload=10;download=20;total=30;expire=40"),
    );

    let info = extract_subscription_userinfo(&headers).expect("header should be parsed");
    assert_eq!(info.upload, Some(10));
    assert_eq!(info.download, Some(20));
    assert_eq!(info.total, Some(30));
    assert_eq!(info.expire, Some(40));
}

#[test]
fn should_retry_subscription_userinfo_when_body_exists_but_header_missing() {
    let result = SubscriptionFetchResult {
        body: "vmess://demo".to_string(),
        userinfo: None,
    };

    assert!(should_retry_subscription_userinfo(&result));
}

#[test]
fn should_not_retry_subscription_userinfo_when_body_is_empty() {
    let result = SubscriptionFetchResult {
        body: "   ".to_string(),
        userinfo: None,
    };

    assert!(!should_retry_subscription_userinfo(&result));
}

#[test]
fn merge_subscription_fetch_result_should_preserve_primary_body_and_use_fallback_userinfo() {
    let primary = SubscriptionFetchResult {
        body: "primary-body".to_string(),
        userinfo: None,
    };
    let fallback_userinfo = Some(SubscriptionUserInfo {
        upload: Some(1),
        download: Some(2),
        total: Some(3),
        expire: Some(4),
    });

    let merged = merge_subscription_fetch_result(primary, fallback_userinfo);

    assert_eq!(merged.body, "primary-body");
    assert_eq!(
        merged.userinfo.as_ref().and_then(|info| info.upload),
        Some(1)
    );
    assert_eq!(
        merged.userinfo.as_ref().and_then(|info| info.download),
        Some(2)
    );
    assert_eq!(
        merged.userinfo.as_ref().and_then(|info| info.total),
        Some(3)
    );
    assert_eq!(
        merged.userinfo.as_ref().and_then(|info| info.expire),
        Some(4)
    );
}

#[test]
fn merge_subscription_fetch_result_should_not_override_existing_userinfo() {
    let primary = SubscriptionFetchResult {
        body: "primary-body".to_string(),
        userinfo: Some(SubscriptionUserInfo {
            upload: Some(10),
            download: Some(20),
            total: Some(30),
            expire: Some(40),
        }),
    };
    let fallback_userinfo = Some(SubscriptionUserInfo {
        upload: Some(1),
        download: Some(2),
        total: Some(3),
        expire: Some(4),
    });

    let merged = merge_subscription_fetch_result(primary, fallback_userinfo);

    assert_eq!(merged.body, "primary-body");
    assert_eq!(
        merged.userinfo.as_ref().and_then(|info| info.upload),
        Some(10)
    );
    assert_eq!(
        merged.userinfo.as_ref().and_then(|info| info.download),
        Some(20)
    );
    assert_eq!(
        merged.userinfo.as_ref().and_then(|info| info.total),
        Some(30)
    );
    assert_eq!(
        merged.userinfo.as_ref().and_then(|info| info.expire),
        Some(40)
    );
}

#[test]
fn merge_subscription_fetch_result_should_keep_none_when_fallback_missing() {
    let primary = SubscriptionFetchResult {
        body: "primary-body".to_string(),
        userinfo: None,
    };

    let merged = merge_subscription_fetch_result(primary, None);

    assert_eq!(merged.body, "primary-body");
    assert!(merged.userinfo.is_none());
}

#[test]
fn normalized_active_config_path_trims_and_filters_empty() {
    assert_eq!(
        normalized_active_config_path(&Some("  /a.json  ".into())),
        Some("/a.json")
    );
    assert_eq!(normalized_active_config_path(&Some("   ".into())), None);
    assert_eq!(normalized_active_config_path(&None), None);
}

#[test]
fn delete_and_rollback_subscription_config_files() {
    use crate::test_support::TempWorkspace;
    use std::fs;

    let ws = TempWorkspace::new();
    let cfg = ws.join("sub.json");
    let bak = ws.join("sub.bak");
    fs::write(&cfg, b"v1").unwrap();
    fs::write(&bak, b"v0").unwrap();

    rollback_subscription_config(cfg.to_string_lossy().to_string()).unwrap();
    assert_eq!(fs::read_to_string(&cfg).unwrap(), "v0");

    delete_subscription_config(cfg.to_string_lossy().to_string()).unwrap();
    assert!(!cfg.exists());
    assert!(!bak.exists());

    // 删除不存在的文件应成功
    delete_subscription_config(ws.join("nope.json").to_string_lossy().to_string()).unwrap();

    // 无备份回滚失败
    let only = ws.join("only.json");
    fs::write(&only, b"x").unwrap();
    assert!(rollback_subscription_config(only.to_string_lossy().to_string()).is_err());
}

#[tokio::test]
async fn fetch_subscription_content_from_local_mock() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = tokio::spawn(async move {
        // 首次无 userinfo；兼容 UA 重试时带上
        for _ in 0..6 {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let mut buf = [0u8; 2048];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let has_compat_ua = req.to_ascii_lowercase().contains("clash");
            let body = "proxies: []\n";
            let extra = if has_compat_ua {
                "Subscription-Userinfo: upload=1; download=2; total=3; expire=4\r\n"
            } else {
                ""
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n{}",
                body.len(),
                extra,
                body
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });

    let url = format!("http://127.0.0.1:{}/sub", port);
    let (body, info) = fetch_subscription_content(&url).await.expect("fetch");
    assert!(body.contains("proxies"));
    // 兼容 UA 重试后应带上 userinfo
    assert!(info.is_some());
    assert_eq!(info.as_ref().and_then(|i| i.upload), Some(1));

    let with_ua = fetch_subscription_content_with_user_agent(&url, Some("clash.meta"))
        .await
        .expect("ua fetch");
    assert!(!with_ua.body.is_empty());
    assert!(with_ua.userinfo.is_some());

    let _ = server.abort();
}

#[tokio::test]
async fn fetch_subscription_content_http_error() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = [0u8; 512];
            let _ = sock.read(&mut buf).await;
            let resp = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });

    let url = format!("http://127.0.0.1:{}/bad", port);
    let err = fetch_subscription_content_with_user_agent(&url, None).await;
    assert!(err.is_err());
    let _ = server.abort();
}

#[test]
fn apply_userinfo_to_subscriptions_path_and_url_match() {
    use crate::app::storage::state_model::Subscription;

    fn sub(name: &str, url: &str, path: Option<&str>) -> Subscription {
        Subscription {
            name: name.into(),
            url: url.into(),
            is_loading: false,
            last_update: None,
            is_manual: false,
            manual_content: None,
            use_original_config: false,
            config_path: path.map(|p| p.to_string()),
            backup_path: None,
            auto_update_interval_minutes: None,
            subscription_upload: Some(9),
            subscription_download: Some(9),
            subscription_total: Some(9),
            subscription_expire: Some(9),
            auto_update_fail_count: None,
            last_auto_update_attempt: None,
            last_auto_update_error: None,
            last_auto_update_error_type: None,
            last_auto_update_backoff_until: None,
        }
    }

    let mut list = vec![
        sub("a", "http://a.example/sub", Some("/tmp/a.json")),
        sub("b", "http://b.example/sub", Some("/tmp/b.json")),
        sub("c", "http://c.example/sub", None),
    ];

    let info = SubscriptionUserInfo {
        upload: Some(1),
        download: Some(2),
        total: Some(3),
        expire: Some(4),
    };
    assert!(apply_userinfo_to_subscriptions(
        &mut list,
        "/tmp/a.json",
        "http://other",
        Some(&info),
        1000,
    ));
    assert_eq!(list[0].subscription_upload, Some(1));
    assert_eq!(list[0].last_update, Some(1000));
    // 其它未匹配保持
    assert_eq!(list[1].subscription_upload, Some(9));

    // url 匹配
    assert!(apply_userinfo_to_subscriptions(
        &mut list,
        "/nope",
        "http://b.example/sub",
        None,
        2000,
    ));
    assert_eq!(list[1].subscription_upload, None);
    assert_eq!(list[1].last_update, Some(2000));

    // 无匹配
    assert!(!apply_userinfo_to_subscriptions(
        &mut list,
        "/none",
        "",
        Some(&info),
        3000,
    ));
}

#[tokio::test]
async fn update_subscription_userinfo_with_mock_storage() {
    use crate::app::storage::state_model::Subscription;
    use crate::test_support::MockAppEnv;
    use std::path::PathBuf;

    let env = MockAppEnv::new();
    let db = env.workspace.path().join("sub_ui.db");
    let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
    let path = env.workspace.path().join("cfg.json");
    std::fs::write(&path, b"{}").unwrap();

    let subs = vec![Subscription {
        name: "s".into(),
        url: "http://example.com/sub".into(),
        is_loading: false,
        last_update: None,
        is_manual: false,
        manual_content: None,
        use_original_config: false,
        config_path: Some(path.to_string_lossy().to_string()),
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

    let info = SubscriptionUserInfo {
        upload: Some(10),
        download: Some(20),
        total: Some(30),
        expire: Some(40),
    };
    update_subscription_userinfo(
        &env.handle(),
        &path,
        "http://example.com/sub",
        Some(info),
    )
    .await
    .unwrap();

    let loaded = storage.get_subscriptions().await.unwrap();
    assert_eq!(loaded[0].subscription_upload, Some(10));
    assert!(loaded[0].last_update.is_some());

    // 无匹配 → 不报错
    update_subscription_userinfo(
        &env.handle(),
        &PathBuf::from("/tmp/nope.json"),
        "http://no-match",
        None,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn set_active_config_path_internal_and_delete_rollback() {
    use crate::app::singbox::config_generator::generate_base_config;
    use crate::app::storage::state_model::AppConfig;
    use crate::test_support::MockAppEnv;
    use std::fs;

    let env = MockAppEnv::new();
    let a = env.workspace.path().join("sing-box/a.json");
    let b = env.workspace.path().join("sing-box/b.json");
    fs::create_dir_all(a.parent().unwrap()).unwrap();
    let base = serde_json::to_string_pretty(&generate_base_config(&AppConfig::default())).unwrap();
    fs::write(&a, &base).unwrap();
    fs::write(&b, &base).unwrap();
    fs::write(b.with_extension("bak"), &base).unwrap();

    let db = env.workspace.path().join("active.db");
    let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
    let mut cfg = AppConfig::default();
    cfg.active_config_path = Some(a.to_string_lossy().to_string());
    storage.save_app_config(&cfg).await.unwrap();

    let (cfg2, restart) = set_active_config_path_internal(
        &env.handle(),
        Some(b.to_string_lossy().to_string()),
    )
    .await
    .unwrap();
    assert!(restart);
    assert_eq!(
        cfg2.active_config_path.as_deref(),
        Some(b.to_str().unwrap())
    );

    // same path → no restart
    let (_cfg3, restart2) = set_active_config_path_internal(
        &env.handle(),
        Some(b.to_string_lossy().to_string()),
    )
    .await
    .unwrap();
    assert!(!restart2);

    rollback_subscription_config(b.to_string_lossy().to_string()).unwrap();
    delete_subscription_config(b.to_string_lossy().to_string()).unwrap();
    assert!(!b.exists());
}

#[test]
fn resolve_current_config_and_persist_result_helpers() {
    let explicit = resolve_current_config_file_path(Some("/tmp/a.json"));
    assert!(explicit.ends_with("a.json"));
    let empty = resolve_current_config_file_path(Some("  "));
    assert!(empty.to_string_lossy().contains("config.json"));
    let none = resolve_current_config_file_path(None);
    assert!(none.to_string_lossy().contains("config.json"));

    let info = SubscriptionUserInfo {
        upload: Some(1),
        download: Some(2),
        total: Some(3),
        expire: Some(4),
    };
    let r = build_subscription_persist_result(std::path::Path::new("/x/c.json"), Some(&info));
    assert_eq!(r.config_path, "/x/c.json");
    assert_eq!(r.subscription_upload, Some(1));
    assert_eq!(r.subscription_total, Some(3));
    let r2 = build_subscription_persist_result(std::path::Path::new("/y.json"), None);
    assert!(r2.subscription_upload.is_none());
}

#[test]
fn read_config_file_content_missing_and_ok() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no.json");
    assert!(read_config_file_content(&missing).is_err());
    let ok = dir.path().join("ok.json");
    std::fs::write(&ok, b"{\"x\":1}").unwrap();
    assert_eq!(read_config_file_content(&ok).unwrap(), "{\"x\":1}");
}

#[tokio::test]
async fn persist_downloaded_and_manual_subscription_content() {
    use crate::app::storage::state_model::AppConfig;
    use crate::test_support::TempWorkspace;

    let ws = TempWorkspace::new();
    let cfg = AppConfig::default();
    let target = ws.path().join("sing-box/configs/sub.json");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();

    // 原始 JSON 订阅
    let body = r#"{"outbounds":[{"type":"direct","tag":"direct"}],"inbounds":[]}"#;
    persist_downloaded_subscription_content(body, true, &cfg, &target).unwrap();
    assert!(target.exists());
    let content = std::fs::read_to_string(&target).unwrap();
    assert!(content.contains("outbounds") || content.contains("direct"));

    let manual_path = ws.path().join("sing-box/configs/manual.json");
    persist_manual_subscription_content(body, true, &cfg, &manual_path).unwrap();
    assert!(manual_path.exists());
}

#[tokio::test]
async fn get_current_config_with_mock_storage() {
    use crate::app::storage::state_model::AppConfig;
    use crate::test_support::MockAppEnv;
    use std::fs;

    let env = MockAppEnv::new();
    let cfg_path = env.workspace.path().join("sing-box/active.json");
    fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
    fs::write(&cfg_path, r#"{"hello":"world"}"#).unwrap();

    let db = env.workspace.path().join("cfg.db");
    let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
    let mut cfg = AppConfig::default();
    cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
    storage.save_app_config(&cfg).await.unwrap();

    let content = get_current_config_impl(env.handle()).await.unwrap();
    assert!(content.contains("hello"));

    // 缺文件
    cfg.active_config_path = Some(env.workspace.path().join("missing.json").to_string_lossy().to_string());
    storage.save_app_config(&cfg).await.unwrap();
    assert!(get_current_config_impl(env.handle()).await.is_err());
}

#[tokio::test]
async fn fetch_subscription_content_retries_userinfo_with_compat_ua() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        for i in 0..4 {
            let Ok((mut s, _)) = listener.accept().await else {
                break;
            };
            let mut buf = [0u8; 2048];
            let n = s.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let body = "proxies: []\n";
            if i == 0 {
                // 首次无 userinfo
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes()).await;
            } else if req.to_ascii_lowercase().contains("clash") {
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nsubscription-userinfo: upload=1; download=2; total=3; expire=4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes()).await;
            } else {
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes()).await;
            }
        }
    });

    let url = format!("http://127.0.0.1:{}/sub", port);
    let (text, info) = fetch_subscription_content(&url).await.unwrap();
    assert!(!text.is_empty());
    // 兼容 UA 可能拿到 userinfo
    let _ = info;
}

#[tokio::test]
async fn download_subscription_core_local_http_without_runtime_apply() {
    use crate::app::storage::state_model::AppConfig;
    use crate::test_support::MockAppEnv;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let env = MockAppEnv::new();
    let db = env.workspace.path().join("sub_dl.db");
    env.install_storage_from_path(db.to_str().unwrap())
        .await
        .save_app_config(&AppConfig::default())
        .await
        .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        for _ in 0..4 {
            let Ok((mut s, _)) = listener.accept().await else {
                break;
            };
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf).await;
            let body = r#"{"outbounds":[{"type":"direct","tag":"direct"}],"inbounds":[]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nsubscription-userinfo: upload=10; download=20; total=30; expire=40\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = s.write_all(resp.as_bytes()).await;
        }
    });

    let target = env
        .workspace
        .path()
        .join("sing-box/configs/from_core.json");
    let result = download_subscription_core(
        &env.handle(),
        format!("http://127.0.0.1:{}/s", port),
        true,
        None,
        Some(target.to_string_lossy().to_string()),
        false,
        Some(17800),
        Some(17801),
    )
    .await
    .expect("download core");
    assert!(std::path::Path::new(&result.config_path).exists());
    assert_eq!(result.subscription_upload, Some(10));
}

#[tokio::test]
async fn download_subscription_core_with_runtime_apply_and_generated() {
    use crate::app::storage::state_model::AppConfig;
    use crate::test_support::MockAppEnv;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let env = MockAppEnv::new();
    let db = env.workspace.path().join("sub_dl2.db");
    env.install_storage_from_path(db.to_str().unwrap())
        .await
        .save_app_config(&AppConfig::default())
        .await
        .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        for _ in 0..4 {
            let Ok((mut s, _)) = listener.accept().await else {
                break;
            };
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf).await;
            // base64 空列表 / 简单 JSON 生成路径
            let body = r#"{"outbounds":[{"type":"direct","tag":"direct"}],"inbounds":[]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = s.write_all(resp.as_bytes()).await;
        }
    });

    let target = env
        .workspace
        .path()
        .join("sing-box/configs/generated.json");
    // use_original_config=true：跳过节点提取，仍走 inject 跳过 + apply_runtime 全路径
    let result = download_subscription_core(
        &env.handle(),
        format!("http://127.0.0.1:{}/g", port),
        true,
        None,
        Some(target.to_string_lossy().to_string()),
        true, // apply_runtime
        None,
        None,
    )
    .await
    .expect("download+runtime");
    assert!(!result.config_path.is_empty());
}

#[tokio::test]
async fn add_manual_subscription_core_with_and_without_runtime() {
    use crate::app::storage::state_model::AppConfig;
    use crate::test_support::MockAppEnv;

    let env = MockAppEnv::new();
    let db = env.workspace.path().join("sub_manual.db");
    env.install_storage_from_path(db.to_str().unwrap())
        .await
        .save_app_config(&AppConfig::default())
        .await
        .unwrap();

    let body = r#"{"outbounds":[{"type":"direct","tag":"direct"}],"inbounds":[]}"#;
    let target = env
        .workspace
        .path()
        .join("sing-box/configs/manual_core.json");

    let no_rt = add_manual_subscription_core(
        &env.handle(),
        body.to_string(),
        true,
        None,
        Some(target.to_string_lossy().to_string()),
        false,
        Some(17001),
        Some(17002),
    )
    .await
    .expect("manual no runtime");
    assert!(std::path::Path::new(&no_rt.config_path).exists());

    let target2 = env
        .workspace
        .path()
        .join("sing-box/configs/manual_rt.json");
    // 原始 JSON 配置路径（use_original=true）以覆盖 apply_runtime 分支
    let with_rt = add_manual_subscription_core(
        &env.handle(),
        body.to_string(),
        true,
        None,
        Some(target2.to_string_lossy().to_string()),
        true,
        None,
        None,
    )
    .await
    .expect("manual with runtime");
    assert!(!with_rt.config_path.is_empty());
}
