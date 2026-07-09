use super::*;
use crate::app::storage::state_model::Subscription;

fn sample_sub() -> Subscription {
    Subscription {
        name: "s1".into(),
        url: "https://example.com/sub".into(),
        is_loading: false,
        last_update: None,
        is_manual: false,
        manual_content: None,
        use_original_config: false,
        config_path: Some("configs/s1.json".into()),
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

#[test]
fn classify_error_categories() {
    assert_eq!(classify_error("connection timed out"), "timeout");
    assert_eq!(classify_error("DNS resolve failed"), "network_dns");
    assert_eq!(classify_error("HTTP 401 Unauthorized"), "auth");
    assert_eq!(classify_error("invalid JSON body"), "config_parse");
    assert_eq!(classify_error("yaml parse error"), "config_parse");
    assert_eq!(classify_error("TLS handshake failed"), "network");
    assert_eq!(classify_error("connection reset"), "network");
    assert_eq!(classify_error("something else"), "unknown");
}

#[test]
fn calc_backoff_minutes_caps_and_grows() {
    assert_eq!(calc_backoff_minutes(0, 1), 5); // base max(5)
    assert_eq!(calc_backoff_minutes(10, 1), 10);
    assert_eq!(calc_backoff_minutes(10, 2), 20);
    assert_eq!(calc_backoff_minutes(10, 3), 40);
    // base 较大且 fail_count 高时封顶 24h
    assert_eq!(calc_backoff_minutes(60, 100), MAX_BACKOFF_MINUTES);
}

#[test]
fn should_run_for_subscription_interval_and_backoff() {
    let mut sub = sample_sub();
    let now = 1_000_000_u64;

    // interval=0 永不跑
    sub.auto_update_interval_minutes = Some(0);
    assert!(!should_run_for_subscription(&sub, now));

    // 无历史 → 应跑
    sub.auto_update_interval_minutes = Some(60);
    sub.last_update = None;
    sub.last_auto_update_attempt = None;
    assert!(should_run_for_subscription(&sub, now));

    // 刚刚更新过 → 不跑
    sub.last_update = Some(now);
    assert!(!should_run_for_subscription(&sub, now));

    // 超过 interval → 跑
    let interval_ms = 60 * 60 * 1000;
    sub.last_update = Some(now.saturating_sub(interval_ms + 1));
    assert!(should_run_for_subscription(&sub, now));

    // backoff 未到期 → 不跑
    sub.last_auto_update_backoff_until = Some(now + 10_000);
    assert!(!should_run_for_subscription(&sub, now));

    // backoff 已过 → 跑
    sub.last_auto_update_backoff_until = Some(now - 1);
    assert!(should_run_for_subscription(&sub, now));
}

#[test]
fn patch_match_and_apply_health() {
    let mut sub = sample_sub();
    let patch = SubscriptionHealthPatch {
        config_path: Some("configs/s1.json".into()),
        url: "https://example.com/sub".into(),
        fail_count: 2,
        last_attempt_ms: 99,
        last_error: Some("boom".into()),
        last_error_type: Some("unknown".into()),
        backoff_until_ms: Some(1234),
    };
    assert!(subscription_matches_patch(&sub, &patch));

    let by_url = SubscriptionHealthPatch {
        config_path: None,
        url: "https://example.com/sub".into(),
        ..patch.clone()
    };
    assert!(subscription_matches_patch(&sub, &by_url));

    let no_match = SubscriptionHealthPatch {
        config_path: Some("other.json".into()),
        url: "https://other".into(),
        ..patch.clone()
    };
    assert!(!subscription_matches_patch(&sub, &no_match));

    apply_health_patch(&mut sub, &patch);
    assert_eq!(sub.auto_update_fail_count, Some(2));
    assert_eq!(sub.last_auto_update_attempt, Some(99));
    assert_eq!(sub.last_auto_update_error.as_deref(), Some("boom"));
    assert_eq!(sub.last_auto_update_error_type.as_deref(), Some("unknown"));
    assert_eq!(sub.last_auto_update_backoff_until, Some(1234));
}

#[test]
fn now_millis_is_nonzero() {
    assert!(now_millis() > 0);
}

#[test]
fn calc_min_interval_and_collect_due() {
    let mut a = sample_sub();
    a.auto_update_interval_minutes = Some(30);
    let mut b = sample_sub();
    b.name = "b".into();
    b.url = "https://b".into();
    b.auto_update_interval_minutes = Some(0);
    let mut c = sample_sub();
    c.name = "c".into();
    c.url = "https://c".into();
    c.auto_update_interval_minutes = Some(120);
    c.last_update = Some(1);

    assert_eq!(calc_min_interval_minutes(&[a.clone(), b.clone(), c.clone()]), 30);
    assert_eq!(calc_min_interval_minutes(&[]), DEFAULT_INTERVAL_MINUTES.max(5));

    let now = 10_000_000u64;
    let list = [a, b, c];
    let due = collect_subscriptions_due(&list, now);
    // b interval 0 skipped; a no last_update due; c depends on interval
    assert!(due.iter().any(|s| s.name == "s1" || s.url.contains("example")));
}

#[test]
fn build_success_and_failure_health_patches() {
    let ok = build_success_health_patch(Some("p.json".into()), " https://x ", 1000);
    assert_eq!(ok.fail_count, 0);
    assert!(ok.last_error.is_none());
    assert_eq!(ok.url, "https://x");

    let fail = build_failure_health_patch(
        Some("p.json".into()),
        "https://x",
        1000,
        1,
        10,
        "connection timeout",
    );
    assert_eq!(fail.fail_count, 2);
    assert_eq!(fail.last_error_type.as_deref(), Some("timeout"));
    assert!(fail.backoff_until_ms.unwrap() > 1000);
}

#[test]
fn should_apply_runtime_for_subscription_matrix() {
    assert!(should_apply_runtime_for_subscription(
        Some("/a.json"),
        Some("/a.json")
    ));
    assert!(!should_apply_runtime_for_subscription(
        Some("/a.json"),
        Some("/b.json")
    ));
    assert!(!should_apply_runtime_for_subscription(None, Some("/a.json")));
    assert!(!should_apply_runtime_for_subscription(Some("/a.json"), None));
    assert!(!should_apply_runtime_for_subscription(None, None));
}

#[tokio::test]
async fn save_health_patches_empty_and_apply() {
    use crate::test_support::MockAppEnv;

    let env = MockAppEnv::new();
    let db = env.workspace.path().join("health.db");
    let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
    let mut sub = sample_sub();
    sub.config_path = Some(env.workspace.path().join("s1.json").to_string_lossy().to_string());
    storage.save_subscriptions(&[sub.clone()]).await.unwrap();

    // empty patches → Ok
    save_health_patches(&env.handle(), &[]).await.unwrap();

    let patch = build_failure_health_patch(
        sub.config_path.clone(),
        &sub.url,
        12345,
        0,
        60,
        "timeout",
    );
    save_health_patches(&env.handle(), &[patch]).await.unwrap();
    let loaded = storage.get_subscriptions().await.unwrap();
    assert_eq!(loaded[0].auto_update_fail_count, Some(1));
}

#[tokio::test]
async fn run_once_with_due_subscription_local_http() {
    use crate::app::storage::state_model::AppConfig;
    use crate::test_support::MockAppEnv;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let env = MockAppEnv::new();
    let db = env.workspace.path().join("auto.db");
    let storage = env.install_storage_from_path(db.to_str().unwrap()).await;

    let body = b"ss://YWVzLTI1Ni1nY206cGFzcw@127.0.0.1:8388#n\n";
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    // 多连接：run_once 可能重试
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else {
                break;
            };
            let mut buf = [0u8; 512];
            let _ = s.read(&mut buf).await;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes()).await;
            let _ = s.write_all(body).await;
        }
    });

    let cfg_path = env.workspace.path().join("configs/due.json");
    std::fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
    std::fs::write(&cfg_path, b"{}").unwrap();

    let mut cfg = AppConfig::default();
    cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
    storage.save_app_config(&cfg).await.unwrap();

    let mut sub = sample_sub();
    sub.name = "due".into();
    sub.url = format!("http://127.0.0.1:{}/sub", port);
    sub.config_path = Some(cfg_path.to_string_lossy().to_string());
    sub.auto_update_interval_minutes = Some(60);
    sub.last_update = None;
    sub.last_auto_update_attempt = None;
    storage.save_subscriptions(&[sub]).await.unwrap();

    run_once(&env.handle()).await.expect("run_once ok");
}

#[tokio::test]
async fn run_once_failure_records_health_patch() {
    use crate::app::storage::state_model::AppConfig;
    use crate::test_support::MockAppEnv;

    let env = MockAppEnv::new();
    let db = env.workspace.path().join("auto-fail.db");
    let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
    storage.save_app_config(&AppConfig::default()).await.unwrap();

    let mut sub = sample_sub();
    sub.name = "fail".into();
    sub.url = "http://127.0.0.1:1/nope".into();
    sub.config_path = Some("fail.json".into());
    sub.auto_update_interval_minutes = Some(60);
    sub.last_update = None;
    storage.save_subscriptions(&[sub]).await.unwrap();

    run_once(&env.handle()).await.expect("run_once handles failure");
    let loaded = storage.get_subscriptions().await.unwrap();
    // 失败应写入 fail_count
    assert!(
        loaded[0].auto_update_fail_count.unwrap_or(0) >= 1
            || loaded[0].last_auto_update_error.is_some()
    );
}

#[tokio::test]
async fn run_once_no_due_subscriptions_is_ok() {
    use crate::app::storage::state_model::AppConfig;
    use crate::test_support::MockAppEnv;

    let env = MockAppEnv::new();
    let db = env.workspace.path().join("auto-empty.db");
    let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
    storage.save_app_config(&AppConfig::default()).await.unwrap();
    let mut sub = sample_sub();
    sub.auto_update_interval_minutes = Some(0); // disabled
    storage.save_subscriptions(&[sub]).await.unwrap();
    run_once(&env.handle()).await.expect("no due");
}
