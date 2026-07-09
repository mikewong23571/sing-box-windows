//! Shared hermetic fixtures for Backend E2E (L3).
//! No real network / sudo / OS system proxy.

use app_lib::app::singbox::config_generator::generate_base_config;
use app_lib::app::storage::database::DatabaseService;
use app_lib::app::storage::enhanced_storage_service::EnhancedStorageService;
use app_lib::app::storage::state_model::AppConfig;
use app_lib::test_support::TempWorkspace;
use app_lib::utils::app_util::WORK_DIR_ENV;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[allow(dead_code)]
pub struct E2eEnv {
    pub workspace: TempWorkspace,
    pub storage: EnhancedStorageService,
    pub config_path: PathBuf,
}

impl E2eEnv {
    pub async fn new() -> Self {
        let workspace = TempWorkspace::new();
        // sing-box config dir under work dir
        let sing_box = workspace.join("sing-box");
        fs::create_dir_all(&sing_box).unwrap();
        let config_path = sing_box.join("config.json");
        let cfg = AppConfig {
            active_config_path: Some(config_path.to_string_lossy().to_string()),
            ..AppConfig::default()
        };
        let generated = generate_base_config(&cfg);
        fs::write(
            &config_path,
            serde_json::to_string_pretty(&generated).unwrap(),
        )
        .unwrap();

        let db_path = workspace.join("app_data.db");
        let storage = EnhancedStorageService::from_path(db_path.to_str().unwrap())
            .await
            .expect("open storage");
        storage.save_app_config(&cfg).await.expect("save app config");

        Self {
            workspace,
            storage,
            config_path,
        }
    }

    #[allow(dead_code)]
    pub fn work_dir(&self) -> &Path {
        self.workspace.path()
    }

    #[allow(dead_code)]
    pub fn assert_hermetic_env() {
        assert!(
            std::env::var(WORK_DIR_ENV).is_ok(),
            "E2E must run under WORK_DIR_ENV isolation"
        );
    }
}

#[allow(dead_code)]
pub async fn open_db(path: &Path) -> Arc<DatabaseService> {
    Arc::new(
        DatabaseService::new(path.to_str().unwrap())
            .await
            .expect("db"),
    )
}
