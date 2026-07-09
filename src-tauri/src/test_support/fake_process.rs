//! Fake kernel process controller for hermetic tests.
//!
//! Records calls and allows simulating failures / running state without spawning real processes.

use crate::app::core::kernel_service::KernelProcessControl;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct Call {
    pub method: String,
    pub config_path: Option<PathBuf>,
    pub tun_enabled: Option<bool>,
}

/// In-memory process controller for tests.
pub struct FakeProcessController {
    pub calls: Mutex<Vec<Call>>,
    pub running: Mutex<bool>,
    pub start_result: Mutex<Result<(), String>>,
    pub stop_result: Mutex<Result<(), String>>,
    pub restart_result: Mutex<Result<(), String>>,
    pub kill_existing_result: Mutex<Result<(), String>>,
    pub force_kill_result: Mutex<Result<(), String>>,
    pub stderr_output: Mutex<Option<String>>,
}

impl Default for FakeProcessController {
    fn default() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            running: Mutex::new(false),
            start_result: Mutex::new(Ok(())),
            stop_result: Mutex::new(Ok(())),
            restart_result: Mutex::new(Ok(())),
            kill_existing_result: Mutex::new(Ok(())),
            force_kill_result: Mutex::new(Ok(())),
            stderr_output: Mutex::new(None),
        }
    }
}

impl FakeProcessController {
    pub fn set_start_result(&self, result: Result<(), String>) {
        *self.start_result.lock().unwrap() = result;
    }

    pub fn set_stop_result(&self, result: Result<(), String>) {
        *self.stop_result.lock().unwrap() = result;
    }

    pub fn set_running(&self, value: bool) {
        *self.running.lock().unwrap() = value;
    }

    pub fn take_calls(&self) -> Vec<Call> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }

    fn record(&self, method: &str, config_path: Option<&Path>, tun_enabled: Option<bool>) {
        self.calls.lock().unwrap().push(Call {
            method: method.to_string(),
            config_path: config_path.map(|p| p.to_path_buf()),
            tun_enabled,
        });
    }
}

#[async_trait::async_trait]
impl<R: tauri::Runtime> KernelProcessControl<R> for FakeProcessController {
    async fn start(
        &self,
        _app_handle: Option<&tauri::AppHandle<R>>,
        config_path: &Path,
        tun_enabled: bool,
    ) -> Result<(), String> {
        self.record("start", Some(config_path), Some(tun_enabled));
        *self.running.lock().unwrap() = true;
        self.start_result.lock().unwrap().clone()
    }

    async fn stop(&self, _app_handle: Option<&tauri::AppHandle<R>>) -> Result<(), String> {
        self.record("stop", None, None);
        *self.running.lock().unwrap() = false;
        self.stop_result.lock().unwrap().clone()
    }

    async fn restart(
        &self,
        _app_handle: &tauri::AppHandle<R>,
        config_path: &Path,
        tun_enabled: bool,
    ) -> Result<(), String> {
        self.record("restart", Some(config_path), Some(tun_enabled));
        *self.running.lock().unwrap() = true;
        self.restart_result.lock().unwrap().clone()
    }

    async fn kill_existing_processes(
        &self,
        _app_handle: Option<&tauri::AppHandle<R>>,
    ) -> Result<(), String> {
        self.record("kill_existing_processes", None, None);
        self.kill_existing_result.lock().unwrap().clone()
    }

    async fn force_kill_kernel_processes_by_name(
        &self,
        _app_handle: Option<&tauri::AppHandle<R>>,
    ) -> Result<(), String> {
        self.record("force_kill_kernel_processes_by_name", None, None);
        self.force_kill_result.lock().unwrap().clone()
    }

    async fn is_running(&self) -> bool {
        *self.running.lock().unwrap()
    }

    async fn read_stderr_output(&self) -> Option<String> {
        self.stderr_output.lock().unwrap().clone()
    }
}
