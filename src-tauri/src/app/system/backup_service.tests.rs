use super::*;
use crate::app::storage::state_model::Subscription;

fn build_subscription(path: &str) -> Subscription {
    Subscription {
        name: "test-sub".to_string(),
        url: "https://example.com/sub".to_string(),
        is_loading: false,
        last_update: None,
        is_manual: false,
        manual_content: None,
        use_original_config: false,
        config_path: Some(path.to_string()),
        backup_path: None,
        auto_update_interval_minutes: Some(720),
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

fn legacy_absolute_path(file_name: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        format!(
            "C:\\Users\\legacy-user\\AppData\\Local\\sing-box-windows\\sing-box\\configs\\{}",
            file_name
        )
    }

    #[cfg(not(target_os = "windows"))]
    {
        format!(
            "/tmp/legacy-user/sing-box-windows/sing-box/configs/{}",
            file_name
        )
    }
}

#[test]
fn encode_absolute_path_for_snapshot_should_return_relative_path() {
    let local_abs = paths::get_config_dir().join("configs").join("sample.json");
    let encoded = encode_path_for_snapshot(
        &local_abs.to_string_lossy(),
        SnapshotPathKind::SubscriptionConfig,
    );
    assert_eq!(encoded, "configs/sample.json");
}

#[test]
fn rewrite_paths_for_snapshot_should_migrate_legacy_absolute_paths() {
    let snapshot = BackupSnapshot {
        format_version: 1,
        app_config: crate::app::storage::state_model::AppConfig {
            active_config_path: Some(legacy_absolute_path("active.json")),
            ..Default::default()
        },
        subscriptions: vec![build_subscription(&legacy_absolute_path("sub.json"))],
        ..Default::default()
    };

    let (app_config, subscriptions, stats) = rewrite_paths_for_snapshot(&snapshot);
    let active_path = PathBuf::from(app_config.active_config_path.unwrap_or_default());
    let sub_path = PathBuf::from(subscriptions[0].config_path.clone().unwrap_or_default());

    assert!(active_path.starts_with(paths::get_config_dir()));
    assert!(sub_path.starts_with(paths::get_config_dir()));
    assert!(stats.absolute_rewrites >= 2);
    assert!(stats.active_path_rewritten);
}

#[test]
fn sanitize_and_json_extension_helpers() {
    assert_eq!(sanitize_file_name("ok.json", "d.json"), "ok.json");
    assert_eq!(sanitize_file_name("../x", "d.json"), "..-x");
    assert_eq!(sanitize_file_name("", "d.json"), "d.json");
    assert_eq!(sanitize_file_name(".", "d.json"), "d.json");
    assert_eq!(sanitize_file_name("..", "d.json"), "d.json");
    let p = with_json_extension(PathBuf::from("a"));
    assert!(p.to_string_lossy().ends_with(".json"));
}

#[test]
fn write_and_parse_snapshot_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("snap.json");
    let snap = BackupSnapshot {
        format_version: 1,
        app_config: crate::app::storage::state_model::AppConfig::default(),
        subscriptions: vec![build_subscription("configs/a.json")],
        ..Default::default()
    };
    let content = serde_json::to_string_pretty(&snap).unwrap();
    write_config_content(&path, &content).unwrap();
    let parsed = parse_snapshot(&content).unwrap();
    assert_eq!(parsed.format_version, 1);
    assert_eq!(parsed.subscriptions.len(), 1);
}

#[test]
fn path_snapshot_encode_relative() {
    let rel = encode_path_for_snapshot("configs/foo.json", SnapshotPathKind::SubscriptionConfig);
    assert!(rel.contains("foo") || rel.contains("configs"));
}

#[test]
fn resolve_export_import_paths_with_explicit_file() {
    let exp = resolve_export_path(Some("  /tmp/backup.json  ".into())).unwrap();
    assert!(exp.to_string_lossy().contains("backup.json"));

    let exp2 = resolve_export_path(Some("backup".into())).unwrap();
    assert!(exp2.to_string_lossy().ends_with(".json"));

    let imp = resolve_import_path(Some("/tmp/in.json".into())).unwrap();
    assert_eq!(imp, PathBuf::from("/tmp/in.json"));
}

#[test]
fn sanitize_file_name_edge_cases() {
    assert_eq!(sanitize_file_name("", "d.json"), "d.json");
    assert_eq!(sanitize_file_name(".", "d.json"), "d.json");
    assert_eq!(sanitize_file_name("..", "d.json"), "d.json");
    let s = sanitize_file_name("a/b\\c:d", "x.json");
    assert!(!s.contains('/'));
    assert!(!s.contains('\\'));
}

#[test]
fn normalize_and_policy_relative_paths() {
    assert_eq!(
        normalize_relative_snapshot_path(Path::new("configs/a.json")).as_deref(),
        Some("configs/a.json")
    );
    assert!(normalize_relative_snapshot_path(Path::new("../x")).is_none());
    assert!(normalize_relative_snapshot_path(Path::new("/abs/x")).is_none());

    assert_eq!(
        enforce_snapshot_relative_policy("config.json", SnapshotPathKind::ActiveConfig),
        "config.json"
    );
    assert_eq!(
        enforce_snapshot_relative_policy("foo.json", SnapshotPathKind::ActiveConfig),
        "configs/foo.json"
    );
    assert_eq!(
        enforce_snapshot_relative_policy("bar.json", SnapshotPathKind::SubscriptionConfig),
        "configs/bar.json"
    );
    assert!(enforce_snapshot_relative_policy("configs/ok.json", SnapshotPathKind::SubscriptionConfig)
        .starts_with("configs/"));
}

#[test]
fn relative_path_from_kind_and_snapshot_buf() {
    assert_eq!(
        relative_path_from_kind(Path::new("x.json"), SnapshotPathKind::ActiveConfig),
        "configs/x.json"
    );
    assert_eq!(
        relative_path_from_kind(Path::new("config.json"), SnapshotPathKind::ActiveConfig),
        "config.json"
    );
    assert_eq!(
        relative_path_from_kind(Path::new("a.bak"), SnapshotPathKind::SubscriptionBackup),
        "configs/a.bak"
    );
    let buf = snapshot_relative_to_path_buf("configs/a/b.json");
    assert_eq!(buf, PathBuf::from("configs").join("a").join("b.json"));
}

#[test]
fn path_to_snapshot_string_joins_segments() {
    let p = PathBuf::from("a").join("b").join("c.json");
    let s = path_to_snapshot_string(&p);
    assert!(s.contains("a") && s.contains("c.json"));
}

#[test]
fn rewrite_stats_warnings_covers_all_fields() {
    let stats = PathRewriteStats {
        absolute_rewrites: 2,
        policy_rewrites: 1,
        subscription_path_rewrites: 3,
        backup_path_rewrites: 1,
        active_path_rewritten: true,
    };
    let dry = rewrite_stats_warnings(&stats, true);
    assert!(dry.iter().any(|w| w.contains("预检")));
    let live = rewrite_stats_warnings(&stats, false);
    assert!(!live.is_empty());
    assert!(live.iter().any(|w| w.contains("绝对路径")));
}

#[test]
fn resolve_active_config_file_path_variants() {
    let def = resolve_active_config_file_path(None);
    assert!(def.ends_with("config.json"));

    let abs = resolve_active_config_file_path(Some("/tmp/abs.json"));
    assert_eq!(abs, PathBuf::from("/tmp/abs.json"));

    let rel = resolve_active_config_file_path(Some("rel.json"));
    assert!(rel.ends_with("rel.json"));
}

#[test]
fn parse_snapshot_rejects_invalid_json() {
    assert!(parse_snapshot("not-json").is_err());
}

#[test]
fn rewrite_paths_with_relative_and_backup() {
    let mut sub = build_subscription("configs/rel.json");
    sub.backup_path = Some("configs/rel.bak".into());
    let snapshot = BackupSnapshot {
        format_version: 2,
        app_config: crate::app::storage::state_model::AppConfig {
            active_config_path: Some("config.json".into()),
            ..Default::default()
        },
        subscriptions: vec![sub],
        ..Default::default()
    };
    let (app_config, subs, _stats) = rewrite_paths_for_snapshot(&snapshot);
    assert!(app_config.active_config_path.unwrap().contains("config"));
    assert!(subs[0].config_path.as_ref().unwrap().contains("rel.json"));
    assert!(subs[0].backup_path.as_ref().unwrap().contains("rel.bak"));
}

#[test]
fn default_paths_under_config_dir() {
    assert!(default_active_config_path().ends_with("config.json"));
    assert!(default_configs_dir().ends_with("configs") || default_configs_dir().to_string_lossy().contains("configs"));
}

#[tokio::test]
async fn apply_snapshot_to_storage_roundtrip() {
    use crate::app::storage::enhanced_storage_service::EnhancedStorageService;
    use crate::app::storage::state_model::{
        AppConfig, LocaleConfig, ThemeConfig, UpdateConfig, WindowConfig,
    };
    use crate::test_support::TempWorkspace;

    let ws = TempWorkspace::new();
    let db = ws.join("backup_apply.db");
    let storage = EnhancedStorageService::from_path(db.to_str().unwrap())
        .await
        .unwrap();

    let app = AppConfig {
        active_config_path: Some("config.json".into()),
        ..AppConfig::default()
    };
    let content = r#"{"log":{"level":"info"},"inbounds":[],"outbounds":[{"type":"direct","tag":"direct"}]}"#;
    let snap_json = serde_json::json!({
        "format_version": 2,
        "app_config": app,
        "theme_config": ThemeConfig::default(),
        "locale_config": LocaleConfig::default(),
        "window_config": WindowConfig::default(),
        "update_config": UpdateConfig::default(),
        "subscriptions": [],
        "active_config_content": content,
    });
    let snapshot = parse_snapshot(&snap_json.to_string()).unwrap();
    let warnings = apply_snapshot_to_storage(&storage, &snapshot).await.unwrap();
    assert!(storage.get_app_config().await.unwrap().active_config_path.is_some());
    let _ = warnings;

    let snap2 = serde_json::json!({
        "format_version": 2,
        "app_config": AppConfig::default(),
        "theme_config": ThemeConfig::default(),
        "locale_config": LocaleConfig::default(),
        "window_config": WindowConfig::default(),
        "update_config": UpdateConfig { update_channel: None, ..UpdateConfig::default() },
        "subscriptions": []
    });
    let snapshot2 = parse_snapshot(&snap2.to_string()).unwrap();
    let w2 = apply_snapshot_to_storage(&storage, &snapshot2).await.unwrap();
    assert!(w2.iter().any(|w| {
        w.contains("active_config_content")
            || w.contains("active_config_path")
            || w.contains("update_channel")
    }));
}

#[test]
fn parse_snapshot_version_guards() {
    assert!(parse_snapshot(r#"{"format_version":0}"#).is_err());
    let ok = serde_json::json!({
        "format_version": 2,
        "app_config": crate::app::storage::state_model::AppConfig::default(),
        "theme_config": crate::app::storage::state_model::ThemeConfig::default(),
        "locale_config": crate::app::storage::state_model::LocaleConfig::default(),
        "window_config": crate::app::storage::state_model::WindowConfig::default(),
        "update_config": crate::app::storage::state_model::UpdateConfig::default(),
    });
    assert!(parse_snapshot(&ok.to_string()).is_ok());
    let high = serde_json::json!({
        "format_version": 999,
        "app_config": crate::app::storage::state_model::AppConfig::default(),
    });
    assert!(parse_snapshot(&high.to_string()).is_err());
}

#[test]
fn decode_snapshot_path_empty_and_relative() {
    let mut stats = PathRewriteStats::default();
    let p = decode_snapshot_path_to_local("", SnapshotPathKind::ActiveConfig, &mut stats);
    assert!(p.ends_with("config.json"));
    assert!(stats.policy_rewrites >= 1);

    let mut stats2 = PathRewriteStats::default();
    let p2 = decode_snapshot_path_to_local(
        "configs/a.json",
        SnapshotPathKind::SubscriptionConfig,
        &mut stats2,
    );
    assert!(p2.to_string_lossy().contains("a.json"));
}

#[test]
fn ensure_parent_dir_creates() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("a/b/c.json");
    ensure_parent_dir(&nested).unwrap();
    assert!(nested.parent().unwrap().exists());
}

#[test]
fn encode_path_absolute_outside_config_dir_falls_back() {
    let encoded = encode_path_for_snapshot(
        "/totally/outside/path/sub.json",
        SnapshotPathKind::SubscriptionConfig,
    );
    assert!(encoded.contains("sub.json") || encoded.starts_with("configs/"));
    let encoded2 = encode_path_for_snapshot(
        "/totally/outside/config.json",
        SnapshotPathKind::ActiveConfig,
    );
    assert!(encoded2.contains("config") || encoded2 == "config.json");
}

#[test]
fn encode_path_with_backslash_and_parent_dir() {
    let enc = encode_path_for_snapshot(
        r"configs\nested\file.json",
        SnapshotPathKind::SubscriptionConfig,
    );
    assert!(enc.contains("file.json") || enc.contains("configs"));
    // parent dir 相对路径应走 policy fallback
    let enc2 = encode_path_for_snapshot("../escape.json", SnapshotPathKind::ActiveConfig);
    assert!(!enc2.is_empty());
}

#[test]
fn decode_snapshot_path_absolute_and_empty_subscription() {
    let mut stats = PathRewriteStats::default();
    let abs = decode_snapshot_path_to_local(
        legacy_absolute_path("x.json").as_str(),
        SnapshotPathKind::SubscriptionConfig,
        &mut stats,
    );
    assert!(abs.to_string_lossy().contains("x.json") || abs.is_absolute());
    assert!(stats.absolute_rewrites >= 1 || stats.policy_rewrites >= 1);

    let mut stats2 = PathRewriteStats::default();
    let empty_sub = decode_snapshot_path_to_local("", SnapshotPathKind::SubscriptionBackup, &mut stats2);
    assert!(empty_sub.to_string_lossy().contains("subscription") || empty_sub.is_absolute());
}

#[test]
fn rewrite_stats_warnings_zero_fields() {
    let stats = PathRewriteStats::default();
    let w = rewrite_stats_warnings(&stats, false);
    assert!(w.is_empty());
    let w2 = rewrite_stats_warnings(&stats, true);
    assert!(w2.is_empty());
}

#[test]
fn with_json_extension_preserves_existing() {
    let p = with_json_extension(PathBuf::from("a.json"));
    assert_eq!(p.extension().and_then(|e| e.to_str()), Some("json"));
    let p2 = with_json_extension(PathBuf::from("a.bak"));
    assert_eq!(p2.extension().and_then(|e| e.to_str()), Some("bak"));
}

#[test]
fn enforce_policy_active_config_already_under_configs() {
    assert_eq!(
        enforce_snapshot_relative_policy("configs/extra.json", SnapshotPathKind::ActiveConfig),
        "configs/extra.json"
    );
    assert_eq!(
        enforce_snapshot_relative_policy("CONFIG.JSON", SnapshotPathKind::ActiveConfig),
        "CONFIG.JSON"
    );
}

#[test]
fn normalize_relative_empty_and_curdir() {
    assert!(normalize_relative_snapshot_path(Path::new("")).is_none());
    assert_eq!(
        normalize_relative_snapshot_path(Path::new("./configs/a.json")).as_deref(),
        Some("configs/a.json")
    );
}

// 注意：resolve_export_path/import_path 在空路径时会打开文件对话框，单测中禁止调用。

#[test]
fn collect_import_precheck_warnings_dry_run_and_live() {
    let mut snap = BackupSnapshot::default();
    snap.format_version = 1;
    snap.created_at = 0;
    snap.subscriptions.clear();
    let live = collect_import_precheck_warnings(&snap, false);
    assert!(live.iter().any(|w| w.contains("旧版备份")));
    assert!(live.iter().any(|w| w.contains("created_at")));
    assert!(live.iter().any(|w| w.contains("没有订阅")));

    let dry = collect_import_precheck_warnings(&snap, true);
    assert!(dry.iter().any(|w| w.contains("预检") || w.contains("active_config")));
    assert!(dry.len() >= live.len());
}

#[test]
fn write_and_read_snapshot_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backup.json");
    let mut snap = BackupSnapshot::default();
    snap.created_at = 12345;
    snap.active_config_content = Some(r#"{"log":{"level":"info"}}"#.into());
    snap.subscriptions.push(build_subscription("configs/a.json"));
    let exported = write_snapshot_to_path(&snap, &path).unwrap();
    assert_eq!(exported.subscriptions_count, 1);
    assert!(path.exists());

    let loaded = read_snapshot_from_path(&path).unwrap();
    assert_eq!(loaded.created_at, 12345);
    assert_eq!(loaded.subscriptions.len(), 1);
    assert!(loaded.active_config_content.is_some());

    let missing = read_snapshot_from_path(&dir.path().join("nope.json"));
    assert!(missing.is_err());
}

#[tokio::test]
async fn build_snapshot_from_storage_and_export_import_dry_run() {
    use crate::app::storage::state_model::AppConfig;
    use crate::test_support::MockAppEnv;
    use std::fs;

    let env = MockAppEnv::new();
    let db = env.workspace.path().join("backup.db");
    let storage = env.install_storage_from_path(db.to_str().unwrap()).await;

    let cfg_path = env.workspace.path().join("sing-box/config.json");
    fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
    fs::write(&cfg_path, r#"{"log":{"level":"info"}}"#).unwrap();

    let mut cfg = AppConfig::default();
    cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
    storage.save_app_config(&cfg).await.unwrap();
    storage
        .save_subscriptions(&[build_subscription(&cfg_path.to_string_lossy())])
        .await
        .unwrap();
    storage.save_active_subscription_index(Some(0)).await.unwrap();

    let snapshot = build_snapshot_from_storage(storage.as_ref()).await.unwrap();
    assert_eq!(snapshot.format_version, SNAPSHOT_FORMAT_VERSION);
    assert_eq!(snapshot.subscriptions.len(), 1);
    assert!(snapshot.active_config_content.is_some());

    let out = env.workspace.path().join("export/snap.json");
    write_snapshot_to_path(&snapshot, &out).unwrap();

    let loaded = read_snapshot_from_path(&out).unwrap();
    let warnings = collect_import_precheck_warnings(&loaded, true);
    // dry-run 路径覆盖：同一份 storage 生成的快照应无警告
    assert!(warnings.is_empty(), "unexpected warnings: {:?}", warnings);

    // apply_snapshot_to_storage 再走一轮
    let warnings2 = apply_snapshot_to_storage(storage.as_ref(), &loaded)
        .await
        .unwrap();
    let _ = warnings2;
}

#[test]
fn write_config_content_creates_parent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/x/config.json");
    write_config_content(&path, "{\"a\":1}").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"a\":1}");
}
