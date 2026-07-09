//! DatabaseService 全量 CRUD 与 schema 多实例测试（D-SCHEMA）。
use super::*;
use crate::app::storage::state_model::{
    AppConfig, LocaleConfig, ThemeConfig, UpdateConfig, WindowConfig,
};

async fn open_temp_db() -> (tempfile::TempDir, DatabaseService) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("app_data.db");
    let db = DatabaseService::new(path.to_str().unwrap())
        .await
        .expect("open db");
    (dir, db)
}

#[tokio::test]
async fn two_independent_databases_each_get_schema() {
    // 并行两库：若全局 SCHEMA_INIT 仍存在，第二库会缺表
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let db_a = DatabaseService::new(dir_a.path().join("a.db").to_str().unwrap())
        .await
        .unwrap();
    let db_b = DatabaseService::new(dir_b.path().join("b.db").to_str().unwrap())
        .await
        .unwrap();

    let mut cfg = AppConfig::default();
    cfg.proxy_port = 18080;
    db_a.save_app_config(&cfg).await.unwrap();
    cfg.proxy_port = 28080;
    db_b.save_app_config(&cfg).await.unwrap();

    assert_eq!(db_a.load_app_config().await.unwrap().unwrap().proxy_port, 18080);
    assert_eq!(db_b.load_app_config().await.unwrap().unwrap().proxy_port, 28080);
    db_a.close().await.unwrap();
    db_b.close().await.unwrap();
}

#[tokio::test]
async fn app_config_roundtrip_and_defaults_when_empty() {
    let (_dir, db) = open_temp_db().await;
    assert!(db.load_app_config().await.unwrap().is_none());

    let mut cfg = AppConfig::default();
    cfg.prefer_ipv6 = true;
    cfg.allow_lan_access = true;
    cfg.system_proxy_enabled = true;
    cfg.tun_enabled = true;
    cfg.active_config_path = Some("/tmp/cfg.json".into());
    cfg.installed_kernel_version = Some("1.2.3".into());
    cfg.tun_route_exclude_address = Some(vec!["10.0.0.0/8".into()]);
    cfg.tray_instance_id = Some("tray-1".into());
    db.save_app_config(&cfg).await.unwrap();

    let loaded = db.load_app_config().await.unwrap().unwrap();
    assert!(loaded.prefer_ipv6);
    assert!(loaded.allow_lan_access);
    assert!(loaded.system_proxy_enabled);
    assert!(loaded.tun_enabled);
    assert_eq!(loaded.active_config_path.as_deref(), Some("/tmp/cfg.json"));
    assert_eq!(loaded.installed_kernel_version.as_deref(), Some("1.2.3"));
    assert_eq!(
        loaded.tun_route_exclude_address,
        Some(vec!["10.0.0.0/8".into()])
    );
    db.close().await.unwrap();
}

#[tokio::test]
async fn theme_locale_window_update_and_generic_kv() {
    let (_dir, db) = open_temp_db().await;

    let theme = ThemeConfig {
        is_dark: true,
        mode: "dark".into(),
        accent_color: "#ff0000".into(),
        compact_mode: true,
    };
    db.save_theme_config(&theme).await.unwrap();
    let t2 = db.load_theme_config().await.unwrap().unwrap();
    assert_eq!(t2.mode, "dark");
    assert_eq!(t2.accent_color, "#ff0000");
    assert!(t2.compact_mode);

    // invalid mode normalizes to system
    let mut theme_bad = theme.clone();
    theme_bad.mode = "rainbow".into();
    db.save_theme_config(&theme_bad).await.unwrap();
    assert_eq!(db.load_theme_config().await.unwrap().unwrap().mode, "system");

    let locale = LocaleConfig {
        locale: "en-US".into(),
    };
    db.save_locale_config(&locale).await.unwrap();
    assert_eq!(
        db.load_locale_config().await.unwrap().unwrap().locale,
        "en-US"
    );

    let window = WindowConfig {
        is_maximized: true,
        width: 1280,
        height: 800,
    };
    db.save_window_config(&window).await.unwrap();
    let w2 = db.load_window_config().await.unwrap().unwrap();
    assert!(w2.is_maximized);
    assert_eq!(w2.width, 1280);

    let update = UpdateConfig {
        auto_check: false,
        last_check: 99,
        last_version: Some("2.0.0".into()),
        skip_version: Some("1.9.0".into()),
        accept_prerelease: true,
        update_channel: Some("beta".into()),
    };
    db.save_update_config(&update).await.unwrap();
    let u2 = db.load_update_config().await.unwrap().unwrap();
    assert!(!u2.auto_check);
    assert_eq!(u2.last_check, 99);
    assert_eq!(u2.update_channel.as_deref(), Some("beta"));

    // generic KV
    #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
    struct Item {
        n: i32,
    }
    db.save_config("k1", &Item { n: 7 }).await.unwrap();
    let got: Item = db.load_config("k1").await.unwrap().unwrap();
    assert_eq!(got.n, 7);
    db.remove_config("k1").await.unwrap();
    let none: Option<Item> = db.load_config("k1").await.unwrap();
    assert!(none.is_none());

    assert!(db.load_theme_config().await.unwrap().is_some());
    db.close().await.unwrap();
}

#[tokio::test]
async fn empty_loads_return_none() {
    let (_dir, db) = open_temp_db().await;
    assert!(db.load_theme_config().await.unwrap().is_none());
    assert!(db.load_locale_config().await.unwrap().is_none());
    assert!(db.load_window_config().await.unwrap().is_none());
    assert!(db.load_update_config().await.unwrap().is_none());
    let none: Option<String> = db.load_config("missing").await.unwrap();
    assert!(none.is_none());
    db.close().await.unwrap();
}

#[test]
fn parse_tun_route_exclude_address_column_handles_edge_cases() {
    assert!(parse_tun_route_exclude_address_column(None).is_none());
    assert!(parse_tun_route_exclude_address_column(Some("".into())).is_none());
    assert!(parse_tun_route_exclude_address_column(Some("   ".into())).is_none());
    assert!(parse_tun_route_exclude_address_column(Some("not-json".into())).is_none());
    let ok = parse_tun_route_exclude_address_column(Some(r#"["10.0.0.0/8"]"#.into()));
    assert!(ok.is_some());
}

#[test]
fn serialize_optional_json_works() {
    let some = serialize_optional_json(&Some(vec!["a".to_string()])).unwrap();
    assert!(some.unwrap().contains("a"));
    assert!(serialize_optional_json::<Vec<String>>(&None).unwrap().is_none());
}
