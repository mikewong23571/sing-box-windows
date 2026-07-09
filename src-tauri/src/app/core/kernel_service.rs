use crate::process::manager::ProcessManager;
use std::sync::{Arc, RwLock};

lazy_static::lazy_static! {
    pub(crate) static ref PROCESS_MANAGER: Arc<ProcessManager> =
        Arc::new(ProcessManager::new());
}

/// 内核进程控制抽象。生产实现为 [`ProcessManager`]；测试中可替换为 Fake。
#[async_trait::async_trait]
pub trait KernelProcessControl<R: tauri::Runtime>: Send + Sync {
    async fn start(
        &self,
        app_handle: Option<&tauri::AppHandle<R>>,
        config_path: &std::path::Path,
        tun_enabled: bool,
    ) -> Result<(), String>;

    async fn stop(&self, app_handle: Option<&tauri::AppHandle<R>>) -> Result<(), String>;

    async fn restart(
        &self,
        app_handle: &tauri::AppHandle<R>,
        config_path: &std::path::Path,
        tun_enabled: bool,
    ) -> Result<(), String>;

    async fn kill_existing_processes(
        &self,
        app_handle: Option<&tauri::AppHandle<R>>,
    ) -> Result<(), String>;

    async fn force_kill_kernel_processes_by_name(
        &self,
        app_handle: Option<&tauri::AppHandle<R>>,
    ) -> Result<(), String>;

    async fn is_running(&self) -> bool;

    async fn read_stderr_output(&self) -> Option<String>;
}

static PROCESS_CONTROLLER: RwLock<Option<Arc<dyn KernelProcessControl<tauri::Wry>>>> =
    RwLock::new(None);

/// 获取全局内核进程控制器的泛型 trait object（生产使用 ProcessManager 单例）。
pub fn kernel_process_manager_singleton<R: tauri::Runtime>() -> Arc<dyn KernelProcessControl<R>> {
    Arc::clone(&PROCESS_MANAGER) as Arc<dyn KernelProcessControl<R>>
}

/// 获取全局内核进程控制器（生产 Runtime 为 Wry）。
pub fn process_controller() -> Arc<dyn KernelProcessControl<tauri::Wry>> {
    {
        let read = PROCESS_CONTROLLER.read().unwrap();
        if let Some(c) = read.as_ref() {
            return c.clone();
        }
    }
    let mut write = PROCESS_CONTROLLER.write().unwrap();
    let controller = write.get_or_insert_with(|| {
        Arc::clone(&PROCESS_MANAGER) as Arc<dyn KernelProcessControl<tauri::Wry>>
    });
    controller.clone()
}

#[cfg(feature = "test-util")]
pub fn set_process_controller_for_test(controller: Arc<dyn KernelProcessControl<tauri::Wry>>) {
    *PROCESS_CONTROLLER.write().unwrap() = Some(controller);
}

#[cfg(feature = "test-util")]
pub fn reset_process_controller_for_test() {
    *PROCESS_CONTROLLER.write().unwrap() = None;
}

pub mod download;
pub mod embedded;
pub mod event;
pub mod guard;
pub mod import;
pub mod lifecycle;
pub mod log_rotation;
pub mod orchestrator;
pub mod readiness;
pub mod relay;
pub mod runtime;
pub mod state;
pub mod status;
pub mod utils;
pub mod versioning;

pub use download::download_kernel;
pub use import::{import_kernel_executable, pick_kernel_import_file};
pub use orchestrator::current_state_version;
pub use runtime::{
    apply_proxy_settings, kernel_restart_fast, kernel_start_enhanced, kernel_stop_enhanced,
    orchestrated_restart_kernel, orchestrated_start_kernel, orchestrated_stop_kernel,
    resolve_proxy_runtime_state, start_kernel_with_state, stop_kernel, ProxyOverrides,
    ResolvedProxyState,
};
pub use state::{KernelRuntimeConfig, KernelState, KernelStateManager, KERNEL_STATE};
pub use status::{
    get_system_uptime, is_kernel_running, kernel_check_health, kernel_get_snapshot,
    kernel_get_status_enhanced,
};
pub use versioning::{check_config_validity, check_kernel_version, get_latest_kernel_version_cmd};
