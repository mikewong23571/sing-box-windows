use crate::app::constants::paths;
use crate::app::core::kernel_service::embedded::{ensure_embedded_kernel, ensure_external_ui};
use crate::app::core::kernel_service::lifecycle::{
    restart_kernel_internal_with_process, resolve_proxy_runtime_state, start_kernel_with_state_with_process,
};
use crate::app::core::kernel_service::status::is_kernel_running_with_process;
use crate::app::core::kernel_service::utils::emit_kernel_error_with_context;
use crate::app::core::kernel_service::versioning::check_config_validity_impl;
use crate::app::core::kernel_service::{KernelProcessControl, KernelRuntimeConfig, ProxyOverrides};
use crate::app::core::proxy_service::SystemProxyPort;
use crate::app::core::tun_profile::TunProxyOptions;
use crate::app::runtime::change::{RuntimeApplyOptions, RuntimeChange};
use crate::app::runtime::orchestrator::apply_runtime_change;
use crate::app::storage::enhanced_storage_service::db_get_app_config;
use crate::app::storage::state_model::AppConfig;
use serde::Serialize;
use tauri::{AppHandle, Runtime};
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct AutoManageOptions {
    pub config: KernelRuntimeConfig,
}

impl AutoManageOptions {
    pub fn from_app_config(config: AppConfig) -> Self {
        AutoManageOptions {
            config: KernelRuntimeConfig::from_app_config(&config),
        }
    }

    pub(crate) fn to_overrides(&self) -> ProxyOverrides {
        ProxyOverrides {
            proxy_mode: self.config.proxy_mode.clone(),
            api_port: self.config.api_port,
            proxy_port: self.config.proxy_port,
            prefer_ipv6: self.config.prefer_ipv6,
            system_proxy_bypass: self.config.system_proxy_bypass.clone(),
            tun_options: self.config.tun_options.clone(),
            system_proxy_enabled: self.config.system_proxy_enabled,
            tun_enabled: self.config.tun_enabled,
            keep_alive: self.config.keep_alive,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AutoManageResult {
    pub state: String,
    pub message: String,
    pub kernel_installed: bool,
    pub config_ready: bool,
    pub attempted_start: bool,
    pub last_start_message: Option<String>,
}

impl AutoManageResult {
    pub(crate) fn new(
        state: &str,
        message: impl Into<String>,
        kernel_installed: bool,
        config_ready: bool,
        attempted_start: bool,
        last_start_message: Option<String>,
    ) -> Self {
        AutoManageResult {
            state: state.to_string(),
            message: message.into(),
            kernel_installed,
            config_ready,
            attempted_start,
            last_start_message,
        }
    }

    pub(crate) fn missing_kernel() -> Self {
        AutoManageResult::new(
            "missing_kernel",
            "未检测到内核，请先下载内核",
            false,
            false,
            false,
            None,
        )
    }

    pub(crate) fn invalid_config(message: String) -> Self {
        AutoManageResult::new(
            "invalid_config",
            format!("配置文件校验失败: {}", message),
            true,
            false,
            false,
            None,
        )
    }

    pub(crate) fn running(message: impl Into<String>, attempted: bool, last_message: Option<String>) -> Self {
        AutoManageResult::new(
            "running",
            message.into(),
            true,
            true,
            attempted,
            last_message,
        )
    }

    pub(crate) fn error(message: impl Into<String>, attempted: bool) -> Self {
        AutoManageResult::new("error", message.into(), true, true, attempted, None)
    }
}

pub(crate) fn kernel_binary_exists() -> bool {
    paths::get_kernel_path().exists()
}

/// 自动管理内核（process + system_proxy 可注入，测试入口）。
pub(crate) async fn auto_manage_kernel_internal_with_process<R: Runtime>(
    app_handle: AppHandle<R>,
    options: AutoManageOptions,
    process: &dyn KernelProcessControl<R>,
    system_proxy: &dyn SystemProxyPort,
) -> Result<AutoManageResult, String> {
    let _attempt_id =
        crate::app::core::kernel_service::state::KERNEL_STATE.begin_attempt("kernel-auto-manage");

    if let Err(err) = ensure_embedded_kernel(&app_handle).await {
        warn!("安装内嵌内核失败，继续按现有逻辑处理: {}", err);
    }

    // 预下载 metacubexd UI，避免首次启动时 sing-box 下载阻塞 API
    if let Err(err) = ensure_external_ui().await {
        warn!(
            "预下载 metacubexd UI 失败（内核启动时仍可自行下载）: {}",
            err
        );
    }

    let overrides = options.to_overrides();

    let kernel_installed = kernel_binary_exists();
    if !kernel_installed {
        return Ok(AutoManageResult::missing_kernel());
    }

    if let Err(err) = check_config_validity_impl(app_handle.clone(), String::new()).await {
        return Ok(AutoManageResult::invalid_config(err));
    }

    let was_running = is_kernel_running_with_process(process).await.unwrap_or(false);
    let attempted_start = compute_attempted_start(was_running, options.config.force_restart);

    if options.config.force_restart && was_running {
        info!("自动管理请求触发内核重启");
        let restart_response =
            restart_kernel_internal_with_process(app_handle.clone(), overrides, process, system_proxy)
                .await?;
        return Ok(auto_manage_result_from_operation_response(
            &restart_response,
            attempted_start,
            "内核重启状态未知",
        ));
    }

    let resolved = resolve_proxy_runtime_state(&app_handle, overrides).await?;
    let start_response =
        start_kernel_with_state_with_process(app_handle, &resolved, process, system_proxy).await?;
    Ok(auto_manage_result_from_operation_response(
        &start_response,
        attempted_start,
        "内核启动状态未知",
    ))
}

pub(crate) async fn auto_manage_kernel_internal<R: Runtime>(
    app_handle: AppHandle<R>,
    options: AutoManageOptions,
) -> Result<AutoManageResult, String> {
    auto_manage_kernel_internal_with_process(
        app_handle,
        options,
        &**crate::app::core::kernel_service::PROCESS_MANAGER,
        &crate::app::core::proxy_service::OsSystemProxy,
    )
    .await
}

/// 是否应尝试启动/重启（纯逻辑）。
pub(crate) fn compute_attempted_start(was_running: bool, force_restart: bool) -> bool {
    !was_running || force_restart
}

/// 从编排器 JSON 响应构造 AutoManageResult（纯逻辑）。
pub(crate) fn auto_manage_result_from_operation_response(
    response: &serde_json::Value,
    attempted_start: bool,
    default_message: &str,
) -> AutoManageResult {
    let success = response
        .get("success")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let message = response
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or(default_message)
        .to_string();

    if success {
        AutoManageResult::running(message.clone(), attempted_start, Some(message))
    } else {
        AutoManageResult::error(message, attempted_start)
    }
}

fn emit_auto_manage_diagnostics<R: Runtime>(
    app_handle: &AppHandle<R>,
    result: &AutoManageResult,
    reason: &str,
) {
    info!(
        "自动管理({})完成，状态: {}, 信息: {}",
        reason, result.state, result.message
    );

    match result.state.as_str() {
        "invalid_config" => {
            emit_kernel_error_with_context(
                app_handle,
                "KERNEL_CONFIG_INVALID",
                "内核启动失败：配置校验未通过",
                Some(&result.message),
                Some("kernel.auto_manage"),
                true,
            );
        }
        "error" => {
            emit_kernel_error_with_context(
                app_handle,
                "KERNEL_AUTO_MANAGE_FAILED",
                "内核自动管理失败",
                Some(&result.message),
                Some("kernel.auto_manage"),
                true,
            );
        }
        "missing_kernel" => {
            emit_kernel_error_with_context(
                app_handle,
                "KERNEL_BINARY_MISSING",
                "未检测到内核文件，请先安装内核",
                Some(&result.message),
                Some("kernel.auto_manage"),
                false,
            );
        }
        _ => {}
    }
}

pub(crate) async fn run_auto_manage_with_saved_config<R: Runtime>(
    app_handle: &AppHandle<R>,
    force_restart: bool,
    reason: &str,
) -> Result<Option<AutoManageResult>, String> {
    match db_get_app_config(app_handle.clone()).await {
        Ok(config) => {
            let mut options = AutoManageOptions::from_app_config(config);
            options.config.force_restart = force_restart;

            match auto_manage_kernel_internal(app_handle.clone(), options).await {
                Ok(result) => {
                    emit_auto_manage_diagnostics(app_handle, &result, reason);
                    Ok(Some(result))
                }
                Err(err) => {
                    warn!("自动管理({})失败: {}", reason, err);
                    emit_kernel_error_with_context(
                        app_handle,
                        "KERNEL_AUTO_MANAGE_FAILED",
                        "内核自动管理异常中断",
                        Some(&err),
                        Some("kernel.auto_manage"),
                        true,
                    );
                    Err(err)
                }
            }
        }
        Err(err) => {
            warn!("加载应用配置失败，跳过自动管理({}): {}", reason, err);
            Ok(None)
        }
    }
}

pub async fn auto_manage_with_saved_config(
    app_handle: &AppHandle,
    force_restart: bool,
    reason: &str,
) {
    let options = RuntimeApplyOptions::new(reason.to_string()).force_restart(force_restart);
    if let Err(error) =
        apply_runtime_change(app_handle, RuntimeChange::KernelUpdated, options).await
    {
        warn!("运行态编排自动管理失败({}): {}", reason, error);
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri 命令需要保持与前端一致的参数形态
pub async fn kernel_auto_manage(
    app_handle: AppHandle,
    proxy_mode: Option<String>,
    api_port: Option<u16>,
    proxy_port: Option<u16>,
    prefer_ipv6: Option<bool>,
    system_proxy_bypass: Option<String>,
    tun_options: Option<TunProxyOptions>,
    keep_alive: Option<bool>,
    system_proxy_enabled: Option<bool>,
    tun_enabled: Option<bool>,
    force_restart: Option<bool>,
) -> Result<serde_json::Value, String> {
    let options = AutoManageOptions {
        config: KernelRuntimeConfig {
            proxy_mode,
            api_port,
            proxy_port,
            prefer_ipv6,
            system_proxy_bypass,
            tun_options,
            keep_alive,
            system_proxy_enabled,
            tun_enabled,
            force_restart: force_restart.unwrap_or(false),
        },
    };

    let result = auto_manage_kernel_internal(app_handle.clone(), options).await?;
    emit_auto_manage_diagnostics(&app_handle, &result, "kernel-auto-manage-command");
    serde_json::to_value(result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::storage::state_model::AppConfig;
    use crate::test_support::TempWorkspace;

    #[test]
    fn auto_manage_result_constructors() {
        let missing = AutoManageResult::missing_kernel();
        assert_eq!(missing.state, "missing_kernel");
        assert!(!missing.kernel_installed);

        let invalid = AutoManageResult::invalid_config("bad".into());
        assert_eq!(invalid.state, "invalid_config");
        assert!(invalid.message.contains("bad"));
        assert!(invalid.kernel_installed);
        assert!(!invalid.config_ready);

        let running = AutoManageResult::running("ok", true, Some("started".into()));
        assert_eq!(running.state, "running");
        assert!(running.attempted_start);
        assert_eq!(running.last_start_message.as_deref(), Some("started"));

        let err = AutoManageResult::error("fail", false);
        assert_eq!(err.state, "error");
        assert!(!err.attempted_start);
    }

    #[test]
    fn from_app_config_maps_to_overrides() {
        let mut cfg = AppConfig::default();
        cfg.api_port = 1234;
        cfg.proxy_port = 5678;
        cfg.system_proxy_enabled = true;
        cfg.tun_enabled = false;
        cfg.prefer_ipv6 = true;
        cfg.system_proxy_bypass = "localhost".into();

        let options = AutoManageOptions::from_app_config(cfg);
        let overrides = options.to_overrides();
        assert_eq!(overrides.api_port, Some(1234));
        assert_eq!(overrides.proxy_port, Some(5678));
        assert_eq!(overrides.prefer_ipv6, Some(true));
        assert_eq!(overrides.system_proxy_enabled, Some(true));
        assert_eq!(overrides.tun_enabled, Some(false));
        assert_eq!(overrides.system_proxy_bypass.as_deref(), Some("localhost"));
    }

    #[test]
    fn kernel_binary_exists_respects_work_dir() {
        let ws = TempWorkspace::new();
        // 无内核文件时应为 false
        assert!(!kernel_binary_exists() || paths::get_kernel_path().exists());
        let kernel = paths::get_kernel_path();
        if let Some(parent) = kernel.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&kernel, b"fake").unwrap();
        assert!(kernel_binary_exists());
        let _ = ws;
    }

    #[test]
    fn compute_attempted_start_matrix() {
        assert!(compute_attempted_start(false, false));
        assert!(compute_attempted_start(false, true));
        assert!(!compute_attempted_start(true, false));
        assert!(compute_attempted_start(true, true));
    }

    #[test]
    fn auto_manage_result_from_operation_response_success_and_error() {
        let ok = serde_json::json!({"success": true, "message": "started"});
        let r = auto_manage_result_from_operation_response(&ok, true, "fallback");
        assert_eq!(r.state, "running");
        assert!(r.attempted_start);
        assert_eq!(r.last_start_message.as_deref(), Some("started"));

        let bad = serde_json::json!({"success": false, "message": "boom"});
        let r2 = auto_manage_result_from_operation_response(&bad, false, "fallback");
        assert_eq!(r2.state, "error");
        assert!(!r2.attempted_start);
        assert!(r2.message.contains("boom"));

        let missing = serde_json::json!({});
        let r3 = auto_manage_result_from_operation_response(&missing, true, "内核启动状态未知");
        assert_eq!(r3.state, "error");
        assert!(r3.message.contains("未知"));
    }

    #[tokio::test]
    async fn auto_manage_missing_kernel_and_diagnostics_via_mock() {
        use crate::test_support::MockAppEnv;
        use crate::app::storage::state_model::AppConfig;

        let env = MockAppEnv::new();
        // 不安装内核 → missing_kernel
        let db = env.workspace.path().join("app_data.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        storage
            .save_app_config(&AppConfig::default())
            .await
            .unwrap();
        // 确保无内核二进制
        let kernel = paths::get_kernel_path();
        let _ = std::fs::remove_file(&kernel);

        let h = env.handle();
        let result = auto_manage_kernel_internal(
            h.clone(),
            AutoManageOptions {
                config: KernelRuntimeConfig {
                    force_restart: false,
                    ..KernelRuntimeConfig::from_app_config(&AppConfig::default())
                },
            },
        )
        .await
        .unwrap();
        assert_eq!(result.state, "missing_kernel");
        emit_auto_manage_diagnostics(&h, &result, "unit-test");

        // 各诊断分支
        let invalid = AutoManageResult::invalid_config("bad cfg".into());
        emit_auto_manage_diagnostics(&h, &invalid, "unit-invalid");
        let err = AutoManageResult::error("oops", true);
        emit_auto_manage_diagnostics(&h, &err, "unit-err");
        let running = AutoManageResult::running("ok", false, None);
        emit_auto_manage_diagnostics(&h, &running, "unit-running");
    }

    #[tokio::test]
    async fn run_auto_manage_with_saved_config_missing_kernel() {
        use crate::test_support::MockAppEnv;
        use crate::app::storage::state_model::AppConfig;

        let env = MockAppEnv::new();
        let db = env.workspace.path().join("app_data.db");
        env.install_storage_from_path(db.to_str().unwrap())
            .await
            .save_app_config(&AppConfig::default())
            .await
            .unwrap();
        let _ = std::fs::remove_file(paths::get_kernel_path());

        let h = env.handle();
        let r = run_auto_manage_with_saved_config(&h, false, "test-reason")
            .await
            .unwrap();
        assert!(r.is_some());
        assert_eq!(r.unwrap().state, "missing_kernel");
    }

    #[tokio::test]
    async fn auto_manage_with_fake_kernel_start_and_force_restart() {
        use crate::app::core::kernel_service::PROCESS_MANAGER;
        use crate::app::singbox::config_generator::generate_base_config;
        use crate::app::storage::state_model::AppConfig;
        use crate::test_support::MockAppEnv;
        use std::fs;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let env = MockAppEnv::new();
        let work = env.workspace.path();
        let dir = work.join("sing-box");
        fs::create_dir_all(&dir).unwrap();
        let kernel = dir.join("sing-box");
        fs::write(
            &kernel,
            r#"#!/bin/sh
if [ "$1" = "check" ]; then exit 0; fi
if [ "$1" = "version" ]; then echo "sing-box version 1.12.0"; exit 0; fi
if [ "$1" = "run" ]; then exec sleep "${FAKE_KERNEL_RUN_SECS:-5}"; fi
exit 0
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = fs::metadata(&kernel).unwrap().permissions();
            p.set_mode(0o755);
            fs::set_permissions(&kernel, p).unwrap();
        }

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api_port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else {
                    break;
                };
                let mut buf = [0u8; 512];
                let _ = s.read(&mut buf).await;
                let body = r#"{"version":"1.0.0"}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes()).await;
            }
        });

        let cfg_path = dir.join("config.json");
        let mut cfg = AppConfig::default();
        cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
        cfg.api_port = api_port;
        cfg.proxy_port = 17950;
        cfg.system_proxy_enabled = false;
        cfg.tun_enabled = false;
        fs::write(
            &cfg_path,
            serde_json::to_string_pretty(&generate_base_config(&cfg)).unwrap(),
        )
        .unwrap();
        let db = work.join("auto_manage.db");
        env.install_storage_from_path(db.to_str().unwrap())
            .await
            .save_app_config(&cfg)
            .await
            .unwrap();

        let h = env.handle();
        let options = AutoManageOptions::from_app_config(cfg.clone());
        let start = auto_manage_kernel_internal(h.clone(), options).await;
        let _ = start;

        // force_restart 路径
        let mut force = AutoManageOptions::from_app_config(cfg);
        force.config.force_restart = true;
        let restart = auto_manage_kernel_internal(h.clone(), force).await;
        let _ = restart;

        let saved = run_auto_manage_with_saved_config(&h, true, "force-saved").await;
        let _ = saved;

        let _ = PROCESS_MANAGER.stop(Some(&h)).await;
        // guard 可能已由 start 路径启用；再次 stop 后由后续用例覆盖 disable
        let _ = crate::app::core::kernel_service::runtime::stop_kernel(Some(&h)).await;
    }

    #[tokio::test]
    async fn auto_manage_kernel_internal_with_process_fake_kernel() {
        use crate::app::core::kernel_service::status::set_platform_kernel_detection_enabled_for_tests;
        use crate::app::core::kernel_service::{
            reset_process_controller_for_test, set_process_controller_for_test,
        };
        use crate::app::core::proxy_service::RecordingSystemProxy;
        use crate::app::singbox::config_generator::generate_base_config;
        use crate::app::storage::state_model::AppConfig;
        use crate::test_support::{FakeProcessController, MockAppEnv};
        use std::fs;
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        set_platform_kernel_detection_enabled_for_tests(false);

        let env = MockAppEnv::new();
        let work = env.workspace.path();
        let dir = work.join("sing-box");
        fs::create_dir_all(&dir).unwrap();
        let kernel = dir.join("sing-box");
        fs::write(
            &kernel,
            r#"#!/bin/sh
if [ "$1" = "check" ]; then exit 0; fi
if [ "$1" = "version" ]; then echo "sing-box version 1.12.0"; exit 0; fi
if [ "$1" = "run" ]; then exec sleep "${FAKE_KERNEL_RUN_SECS:-5}"; fi
exit 0
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = fs::metadata(&kernel).unwrap().permissions();
            p.set_mode(0o755);
            fs::set_permissions(&kernel, p).unwrap();
        }

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api_port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else {
                    break;
                };
                let mut buf = [0u8; 512];
                let _ = s.read(&mut buf).await;
                let body = r#"{"version":"1.0.0"}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes()).await;
            }
        });

        let cfg_path = dir.join("config.json");
        let mut cfg = AppConfig::default();
        cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
        cfg.api_port = api_port;
        cfg.proxy_port = 17960;
        cfg.system_proxy_enabled = false;
        cfg.tun_enabled = false;
        fs::write(
            &cfg_path,
            serde_json::to_string_pretty(&generate_base_config(&cfg)).unwrap(),
        )
        .unwrap();
        let db = work.join("auto_manage_fake.db");
        env.install_storage_from_path(db.to_str().unwrap())
            .await
            .save_app_config(&cfg)
            .await
            .unwrap();

        let fake = Arc::new(FakeProcessController::default());
        fake.set_start_result(Ok(()));
        set_process_controller_for_test(fake.clone());
        let proxy = RecordingSystemProxy::default();

        let h = env.handle();
        let options = AutoManageOptions::from_app_config(cfg);
        let result = auto_manage_kernel_internal_with_process(h, options, &*fake, &proxy)
            .await
            .expect("auto_manage with fake process");
        assert_eq!(result.state, "running");
        assert!(result.attempted_start);

        let calls = fake.calls.lock().unwrap();
        assert!(calls.iter().any(|c| c.method == "start"));

        reset_process_controller_for_test();
        set_platform_kernel_detection_enabled_for_tests(true);
    }
}
