//! L3-01: storage multi-table hermetic journey.
mod common;

use app_lib::app::storage::state_model::{
    LocaleConfig, Subscription, ThemeConfig, WindowConfig,
};
use common::E2eEnv;

fn sample_sub(path: &str) -> Subscription {
    Subscription {
        name: "test".into(),
        url: "https://example.invalid/sub".into(),
        is_loading: false,
        last_update: None,
        is_manual: false,
        manual_content: None,
        use_original_config: false,
        config_path: Some(path.into()),
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

/// L3-01: empty DB → save/load app/theme/subs
#[tokio::test]
async fn l3_01_storage_crud_roundtrip() {
    let env = E2eEnv::new().await;
    E2eEnv::assert_hermetic_env();

    let mut app = env.storage.get_app_config().await.unwrap();
    app.proxy_port = 19090;
    env.storage.save_app_config(&app).await.unwrap();
    assert_eq!(
        env.storage.get_app_config().await.unwrap().proxy_port,
        19090
    );

    let theme = ThemeConfig {
        is_dark: false,
        mode: "light".into(),
        accent_color: "#00ff00".into(),
        compact_mode: true,
    };
    env.storage.save_theme_config(&theme).await.unwrap();
    assert_eq!(
        env.storage.get_theme_config().await.unwrap().mode,
        "light"
    );

    env.storage
        .save_locale_config(&LocaleConfig {
            locale: "ja-JP".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        env.storage.get_locale_config().await.unwrap().locale,
        "ja-JP"
    );

    env.storage
        .save_window_config(&WindowConfig {
            is_maximized: true,
            width: 800,
            height: 600,
        })
        .await
        .unwrap();

    let path = env.config_path.to_string_lossy().to_string();
    env.storage
        .save_subscriptions(&[sample_sub(&path)])
        .await
        .unwrap();
    let loaded = env.storage.get_subscriptions().await.unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "test");

    env.storage
        .save_active_subscription_index(Some(0))
        .await
        .unwrap();
    assert_eq!(
        env.storage.get_active_subscription_index().await.unwrap(),
        Some(0)
    );

    // generic config channel used by custom rules
    env.storage
        .save_generic_config("custom_rules", &Vec::<serde_json::Value>::new())
        .await
        .unwrap();
    let rules: Option<Vec<serde_json::Value>> =
        env.storage.load_generic_config("custom_rules").await.unwrap();
    assert_eq!(rules.unwrap().len(), 0);
}

/// L3-12: active config path + app config stay consistent after rewrite of defaults
#[tokio::test]
async fn l3_12_active_config_and_defaults_consistent() {
    let env = E2eEnv::new().await;
    let cfg = env.storage.get_app_config().await.unwrap();
    assert!(env.config_path.exists());
    assert_eq!(
        cfg.active_config_path.as_deref(),
        Some(env.config_path.to_string_lossy().as_ref())
    );
    // ensure AppConfig::default ports applied
    assert!(cfg.proxy_port > 0);
    assert!(cfg.api_port > 0);
}
