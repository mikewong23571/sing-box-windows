use super::DatabaseService;
use crate::app::core::tun_profile::normalize_tun_route_exclude_address;
use crate::app::storage::error::StorageResult;
use crate::app::storage::state_model::{
    AppConfig, LocaleConfig, StartupPreferences, Subscription, ThemeConfig, UpdateConfig,
    WindowConfig,
};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::OnceCell;

const STARTUP_PREFERENCES_FILE: &str = "startup_preferences.json";

/// 获取数据库服务的辅助函数（单例初始化）
pub async fn get_enhanced_storage<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Result<Arc<EnhancedStorageService>, String> {
    let cell_state = app.state::<Arc<OnceCell<Arc<EnhancedStorageService>>>>();
    let cell = Arc::clone(&*cell_state);

    cell.get_or_try_init(|| async {
        tracing::info!("?? 初始化新的数据库服务...");
        EnhancedStorageService::new(app).await.map(Arc::new)
    })
    .await
    .map(|svc| {
        tracing::info!("? 使用已初始化的数据库服务");
        svc.clone()
    })
    .map_err(|e| {
        tracing::error!("? 数据库服务初始化失败: {}", e);
        format!("Failed to initialize enhanced storage: {}", e)
    })
}

/// 增强版存储服务 - 使用 SQLite 数据库
#[derive(Debug, Clone)]
pub struct EnhancedStorageService {
    database: Arc<DatabaseService>,
}

impl EnhancedStorageService {
    pub async fn new<R: tauri::Runtime>(app_handle: &AppHandle<R>) -> StorageResult<Self> {
        let app_data_dir = resolve_app_data_dir(app_handle);

        // 确保目录存在
        std::fs::create_dir_all(&app_data_dir)?;

        let database_path = app_data_dir.join("app_data.db");
        let database = Arc::new(DatabaseService::new(database_path.to_str().unwrap()).await?);

        Ok(Self { database })
    }

    /// 从已有 DatabaseService 构造（测试/E2E 用，无 AppHandle）。
    pub fn from_database(database: Arc<DatabaseService>) -> Self {
        Self { database }
    }

    /// 打开指定路径的 SQLite 文件（测试/E2E hermetic 存储）。
    pub async fn from_path(database_path: &str) -> StorageResult<Self> {
        if let Some(parent) = std::path::Path::new(database_path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let database = Arc::new(DatabaseService::new(database_path).await?);
        Ok(Self { database })
    }

    /// 底层数据库访问（测试夹具）。
    pub fn database(&self) -> &Arc<DatabaseService> {
        &self.database
    }

    // 应用配置
    pub async fn get_app_config(&self) -> StorageResult<AppConfig> {
        match self.database.load_app_config().await? {
            Some(config) => Ok(config),
            None => Ok(AppConfig::default()),
        }
    }

    pub async fn save_app_config(&self, config: &AppConfig) -> StorageResult<()> {
        self.database.save_app_config(config).await
    }

    // 通用 KV 配置（custom_rules 等结构化数据复用此通道，避免新表/迁移）
    pub async fn load_generic_config<T>(&self, key: &str) -> StorageResult<Option<T>>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        self.database.load_config(key).await
    }

    pub async fn save_generic_config<T>(&self, key: &str, value: &T) -> StorageResult<()>
    where
        T: serde::Serialize,
    {
        self.database.save_config(key, value).await
    }

    // 主题配置
    pub async fn get_theme_config(&self) -> StorageResult<ThemeConfig> {
        match self.database.load_theme_config().await? {
            Some(config) => Ok(config),
            None => Ok(ThemeConfig::default()),
        }
    }

    pub async fn save_theme_config(&self, config: &ThemeConfig) -> StorageResult<()> {
        self.database.save_theme_config(config).await
    }

    // 语言配置
    pub async fn get_locale_config(&self) -> StorageResult<LocaleConfig> {
        match self.database.load_locale_config().await? {
            Some(config) => Ok(config),
            None => Ok(LocaleConfig::default()),
        }
    }

    pub async fn save_locale_config(&self, config: &LocaleConfig) -> StorageResult<()> {
        self.database.save_locale_config(config).await
    }

    // 窗口配置
    pub async fn get_window_config(&self) -> StorageResult<WindowConfig> {
        match self.database.load_window_config().await? {
            Some(config) => Ok(config),
            None => Ok(WindowConfig::default()),
        }
    }

    pub async fn save_window_config(&self, config: &WindowConfig) -> StorageResult<()> {
        self.database.save_window_config(config).await
    }

    // 更新配置
    pub async fn get_update_config(&self) -> StorageResult<UpdateConfig> {
        match self.database.load_update_config().await? {
            Some(config) => Ok(config),
            None => Ok(UpdateConfig::default()),
        }
    }

    pub async fn save_update_config(&self, config: &UpdateConfig) -> StorageResult<()> {
        self.database.save_update_config(config).await
    }

    // 订阅管理
    pub async fn get_subscriptions(&self) -> StorageResult<Vec<Subscription>> {
        match self
            .database
            .load_config::<Vec<Subscription>>("subscriptions")
            .await?
        {
            Some(subscriptions) => Ok(subscriptions),
            None => Ok(Vec::new()),
        }
    }

    pub async fn save_subscriptions(&self, subscriptions: &[Subscription]) -> StorageResult<()> {
        self.database
            .save_config("subscriptions", &subscriptions)
            .await
    }

    // 激活订阅索引
    pub async fn get_active_subscription_index(&self) -> StorageResult<Option<i64>> {
        match self
            .database
            .load_config::<i64>("active_subscription_index")
            .await?
        {
            Some(index) => Ok(Some(index)),
            None => Ok(None),
        }
    }

    pub async fn save_active_subscription_index(&self, index: Option<i64>) -> StorageResult<()> {
        if let Some(idx) = index {
            self.database
                .save_config("active_subscription_index", &idx)
                .await
        } else {
            self.database
                .remove_config("active_subscription_index")
                .await
        }
    }

    // 通用配置
    pub async fn get_config<T>(&self, key: &str) -> StorageResult<Option<T>>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        self.database.load_config(key).await
    }

    pub async fn save_config<T>(&self, key: &str, value: &T) -> StorageResult<()>
    where
        T: serde::Serialize,
    {
        self.database.save_config(key, value).await
    }

    pub async fn remove_config(&self, key: &str) -> StorageResult<()> {
        self.database.remove_config(key).await
    }
}

fn resolve_app_data_dir<R: tauri::Runtime>(app_handle: &AppHandle<R>) -> std::path::PathBuf {
    app_handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::current_dir().unwrap())
}

fn resolve_startup_preferences_path<R: tauri::Runtime>(
    app_handle: &AppHandle<R>,
) -> std::path::PathBuf {
    resolve_app_data_dir(app_handle).join(STARTUP_PREFERENCES_FILE)
}

pub(crate) fn build_startup_preferences(config: &AppConfig) -> StartupPreferences {
    StartupPreferences {
        auto_start_app: config.auto_start_app,
        auto_hide_to_tray_on_autostart: config.auto_hide_to_tray_on_autostart,
        tray_close_behavior: config.tray_close_behavior.clone(),
    }
}

fn normalize_app_config_for_persistence(mut config: AppConfig) -> Result<AppConfig, String> {
    config.tun_route_exclude_address =
        normalize_tun_route_exclude_address(config.tun_route_exclude_address)?;
    Ok(config)
}

pub fn read_startup_preferences_sync<R: tauri::Runtime>(
    app_handle: &AppHandle<R>,
) -> StartupPreferences {
    let path = resolve_startup_preferences_path(app_handle);
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) => {
            tracing::debug!("读取启动偏好失败，使用默认值: {}", error);
            return StartupPreferences::default();
        }
    };

    serde_json::from_str(&content).unwrap_or_else(|error| {
        tracing::warn!("解析启动偏好失败，使用默认值: {}", error);
        StartupPreferences::default()
    })
}

pub fn save_startup_preferences_sync<R: tauri::Runtime>(
    app_handle: &AppHandle<R>,
    config: &AppConfig,
) -> Result<(), String> {
    let app_data_dir = resolve_app_data_dir(app_handle);
    std::fs::create_dir_all(&app_data_dir).map_err(|e| format!("创建应用数据目录失败: {}", e))?;

    let path = resolve_startup_preferences_path(app_handle);
    let payload = build_startup_preferences(config);
    let content =
        serde_json::to_string_pretty(&payload).map_err(|e| format!("序列化启动偏好失败: {}", e))?;
    std::fs::write(&path, content).map_err(|e| format!("写入启动偏好失败: {}", e))
}

// Tauri 命令实现
#[tauri::command]
pub async fn db_get_app_config<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<AppConfig, String> {
    db_get_app_config_internal(&app).await
}

pub async fn db_get_app_config_internal<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Result<AppConfig, String> {
    let storage = get_enhanced_storage(app).await?;
    #[allow(unused_mut)]
    let mut config = storage.get_app_config().await.map_err(|e| e.to_string())?;

    // Windows：非管理员启动时自动关闭 TUN，避免因缺少权限导致内核无法拉起
    // Linux/macOS：内核可通过 sudo 提权启动（应用本身无需 root），因此不在这里强制关闭。
    #[cfg(target_os = "windows")]
    if config.tun_enabled && !crate::app::system::system_service::check_admin() {
        let previous_mode = config.proxy_mode.clone();
        config.tun_enabled = false;
        config.proxy_mode = if config.system_proxy_enabled {
            "system".to_string()
        } else {
            "manual".to_string()
        };

        if let Err(err) = storage.save_app_config(&config).await {
            tracing::warn!("在非管理员模式下写入关闭 TUN 设置失败: {}", err);
        } else {
            tracing::info!(
                "检测到当前未获得管理员权限，已自动关闭 TUN 模式（原模式: {}）",
                previous_mode
            );
        }
    }

    Ok(config)
}

pub async fn db_save_app_config_internal<R: tauri::Runtime>(
    config: AppConfig,
    app: &AppHandle<R>,
) -> Result<(), String> {
    let config = normalize_app_config_for_persistence(config)?;
    let storage = get_enhanced_storage(app).await?;
    storage
        .save_app_config(&config)
        .await
        .map_err(|e| e.to_string())?;
    save_startup_preferences_sync(app, &config)?;
    Ok(())
}

#[tauri::command]
pub async fn db_get_theme_config<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<ThemeConfig, String> {
    let storage = get_enhanced_storage(&app).await?;
    storage.get_theme_config().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn db_save_theme_config<R: tauri::Runtime>(
    config: ThemeConfig,
    app: AppHandle<R>,
) -> Result<(), String> {
    let storage = get_enhanced_storage(&app).await?;
    storage
        .save_theme_config(&config)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn db_get_locale_config<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<LocaleConfig, String> {
    let storage = get_enhanced_storage(&app).await?;
    storage.get_locale_config().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn db_save_locale_config<R: tauri::Runtime>(
    config: LocaleConfig,
    app: AppHandle<R>,
) -> Result<(), String> {
    let storage = get_enhanced_storage(&app).await?;
    storage
        .save_locale_config(&config)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn db_get_window_config<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<WindowConfig, String> {
    let storage = get_enhanced_storage(&app).await?;
    storage.get_window_config().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn db_save_window_config<R: tauri::Runtime>(
    config: WindowConfig,
    app: AppHandle<R>,
) -> Result<(), String> {
    let storage = get_enhanced_storage(&app).await?;
    storage
        .save_window_config(&config)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn db_get_update_config<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<UpdateConfig, String> {
    let storage = get_enhanced_storage(&app).await?;
    storage.get_update_config().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn db_save_update_config<R: tauri::Runtime>(
    config: UpdateConfig,
    app: AppHandle<R>,
) -> Result<(), String> {
    let storage = get_enhanced_storage(&app).await?;
    storage
        .save_update_config(&config)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn db_get_subscriptions<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<Subscription>, String> {
    let storage = get_enhanced_storage(&app).await?;
    storage.get_subscriptions().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn db_save_subscriptions<R: tauri::Runtime>(
    subscriptions: Vec<Subscription>,
    app: AppHandle<R>,
) -> Result<(), String> {
    let storage = get_enhanced_storage(&app).await?;
    storage
        .save_subscriptions(&subscriptions)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn db_get_active_subscription_index<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<Option<i64>, String> {
    let storage = get_enhanced_storage(&app).await?;
    storage
        .get_active_subscription_index()
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::storage::state_model::{
        LocaleConfig, Subscription, ThemeConfig, UpdateConfig, WindowConfig,
    };
    use crate::test_support::TempWorkspace;

    #[test]
    fn should_normalize_blank_tun_route_exclude_address_to_none() {
        let normalized =
            normalize_app_config_for_persistence(crate::app::storage::state_model::AppConfig {
                tun_route_exclude_address: Some(vec!["  ".to_string(), "".to_string()]),
                ..crate::app::storage::state_model::AppConfig::default()
            })
            .expect("blank route exclude address should normalize");

        assert_eq!(normalized.tun_route_exclude_address, None);
    }

    #[test]
    fn should_reject_invalid_tun_route_exclude_address_on_save() {
        let error =
            normalize_app_config_for_persistence(crate::app::storage::state_model::AppConfig {
                tun_route_exclude_address: Some(vec!["invalid".to_string()]),
                ..crate::app::storage::state_model::AppConfig::default()
            })
            .expect_err("invalid route exclude address should be rejected");

        assert!(
            error.contains("invalid"),
            "error should mention invalid CIDR, got: {}",
            error
        );
    }

    #[tokio::test]
    async fn full_crud_roundtrip_all_config_types() {
        let ws = TempWorkspace::new();
        let db = ws.join("full.db");
        let storage = EnhancedStorageService::from_path(db.to_str().unwrap())
            .await
            .unwrap();

        let mut app = AppConfig::default();
        app.proxy_port = 12345;
        storage.save_app_config(&app).await.unwrap();
        assert_eq!(storage.get_app_config().await.unwrap().proxy_port, 12345);

        let theme = ThemeConfig {
            is_dark: true,
            ..ThemeConfig::default()
        };
        storage.save_theme_config(&theme).await.unwrap();
        assert!(storage.get_theme_config().await.unwrap().is_dark);

        let locale = LocaleConfig {
            locale: "zh-CN".into(),
        };
        storage.save_locale_config(&locale).await.unwrap();
        assert_eq!(storage.get_locale_config().await.unwrap().locale, "zh-CN");

        let window = WindowConfig::default();
        storage.save_window_config(&window).await.unwrap();
        let _ = storage.get_window_config().await.unwrap();

        let update = UpdateConfig::default();
        storage.save_update_config(&update).await.unwrap();
        let _ = storage.get_update_config().await.unwrap();

        let subs = vec![Subscription {
            name: "s".into(),
            url: "https://x".into(),
            is_loading: false,
            last_update: None,
            is_manual: false,
            manual_content: None,
            use_original_config: false,
            config_path: Some("c.json".into()),
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
        }];
        storage.save_subscriptions(&subs).await.unwrap();
        assert_eq!(storage.get_subscriptions().await.unwrap().len(), 1);

        storage
            .save_active_subscription_index(Some(0))
            .await
            .unwrap();
        assert_eq!(
            storage.get_active_subscription_index().await.unwrap(),
            Some(0)
        );
        storage.save_active_subscription_index(None).await.unwrap();
        assert_eq!(
            storage.get_active_subscription_index().await.unwrap(),
            None
        );

        storage.save_config("k", &"v".to_string()).await.unwrap();
        let v: Option<String> = storage.get_config("k").await.unwrap();
        assert_eq!(v.as_deref(), Some("v"));
        storage.remove_config("k").await.unwrap();
        let v2: Option<String> = storage.get_config("k").await.unwrap();
        assert!(v2.is_none());

        storage
            .save_generic_config("g", &vec![1u32, 2])
            .await
            .unwrap();
        let g: Option<Vec<u32>> = storage.load_generic_config("g").await.unwrap();
        assert_eq!(g, Some(vec![1, 2]));

        let _ = storage.database();
        let _ = EnhancedStorageService::from_database(storage.database().clone());
    }

    #[test]
    fn build_startup_preferences_from_app_config() {
        let cfg = AppConfig {
            auto_start_app: true,
            auto_hide_to_tray_on_autostart: true,
            tray_close_behavior: "lightweight".into(),
            ..AppConfig::default()
        };
        let prefs = build_startup_preferences(&cfg);
        assert!(prefs.auto_start_app);
        assert!(prefs.auto_hide_to_tray_on_autostart);
        assert_eq!(prefs.tray_close_behavior, "lightweight");
    }
}
