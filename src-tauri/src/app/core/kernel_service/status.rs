use crate::app::constants::paths;
use crate::app::core::kernel_service::state::KERNEL_STATE;
use crate::app::core::kernel_service::utils::KernelStatusPayload;
use crate::app::core::kernel_service::{process_controller, KernelProcessControl};
use crate::platform;
use crate::utils::http_client;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tracing::info;

/// 平台层进程检测开关（test-util 下可关闭以保证 hermetic）。
static PLATFORM_KERNEL_DETECTION_ENABLED: AtomicBool = AtomicBool::new(true);

#[cfg(any(test, feature = "test-util"))]
#[allow(dead_code)]
pub(crate) fn set_platform_kernel_detection_enabled_for_tests(enabled: bool) {
    PLATFORM_KERNEL_DETECTION_ENABLED.store(enabled, Ordering::Relaxed);
}

#[cfg(any(test, feature = "test-util"))]
#[allow(dead_code)]
pub(crate) fn reset_platform_kernel_detection_for_tests() {
    PLATFORM_KERNEL_DETECTION_ENABLED.store(true, Ordering::Relaxed);
}

/// 运行时状态探测结果（无 AppHandle，便于单测）。
#[derive(Debug, Clone, Default)]
pub(crate) struct KernelRuntimeProbe {
    pub process_running: bool,
    pub api_ready: bool,
    pub websocket_ready: bool,
    pub version: Option<String>,
    pub error: Option<String>,
}

/// 探测 Clash API `/version`（短超时）。
pub(crate) async fn probe_version_api(port: u16) -> (bool, Option<String>, Option<String>) {
    let client = http_client::get_client();
    let api_url = format!("http://127.0.0.1:{}/version", port);
    match client
        .get(&api_url)
        .timeout(Duration::from_millis(500))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => {
            let version = response
                .text()
                .await
                .ok()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty());
            (true, version, None)
        }
        Ok(response) => (
            false,
            None,
            Some(format!("API返回错误状态码: {}", response.status())),
        ),
        Err(e) => (false, None, Some(format!("API连接失败: {}", e))),
    }
}

/// 探测 traffic WebSocket 是否可连（1s 超时）。
pub(crate) async fn probe_traffic_websocket(port: u16) -> bool {
    let token = crate::app::core::proxy_service::get_api_token();
    let url_str = format!("ws://127.0.0.1:{}/traffic?token={}", port, token);
    // timeout 完成但连接失败时仍为 false（不能只用 timeout.is_ok()）
    matches!(
        tokio::time::timeout(
            Duration::from_secs(1),
            tokio_tungstenite::connect_async(&url_str),
        )
        .await,
        Ok(Ok(_))
    )
}

/// 组装进程 + API + WS 探测（不读 DB、不依赖 AppHandle，process 可注入）。
pub(crate) async fn collect_kernel_runtime_probe_with_process<R: tauri::Runtime>(
    process: &dyn KernelProcessControl<R>,
    port: u16,
) -> KernelRuntimeProbe {
    let process_running = is_kernel_running_with_process(process)
        .await
        .unwrap_or(false);
    let mut probe = KernelRuntimeProbe {
        process_running,
        ..Default::default()
    };

    if !process_running {
        return probe;
    }

    let (api_ready, version, api_err) = probe_version_api(port).await;
    probe.api_ready = api_ready;
    probe.version = version;
    probe.error = api_err;

    if api_ready {
        probe.websocket_ready = probe_traffic_websocket(port).await;
        if !probe.websocket_ready && probe.error.is_none() {
            probe.error = Some("WebSocket连接失败".to_string());
        }
    } else if probe.error.is_none() {
        probe.error = Some("内核进程运行中但API服务不可用".to_string());
    }

    probe
}

/// 组装进程 + API + WS 探测（使用生产进程控制器）。
pub(crate) async fn collect_kernel_runtime_probe(port: u16) -> KernelRuntimeProbe {
    collect_kernel_runtime_probe_with_process(&*process_controller(), port).await
}

/// 健康检查核心：给定进程是否运行时，是否还要查 API。
pub(crate) async fn build_health_report(
    kernel_exists: bool,
    config_exists: bool,
    process_running: bool,
    api_port: u16,
) -> serde_json::Value {
    let mut issues = Vec::new();
    let mut healthy = true;

    if !kernel_exists {
        issues.push("内核文件不存在".to_string());
        healthy = false;
    }
    if !config_exists {
        issues.push("配置文件不存在".to_string());
        healthy = false;
    }

    if process_running {
        let (api_ready, _, _) = probe_version_api(api_port).await;
        // probe_version_api 用 500ms；健康检查原为 2s，这里再补一次较长探测以贴近原语义
        let api_ready = if api_ready {
            true
        } else {
            let client = http_client::get_client();
            let api_url = format!("http://127.0.0.1:{}/version", api_port);
            matches!(
                client
                    .get(&api_url)
                    .timeout(Duration::from_secs(2))
                    .send()
                    .await,
                Ok(response) if response.status().is_success()
            )
        };
        if !api_ready {
            issues.push(format!("内核进程运行但API不可用（端口: {}）", api_port));
            healthy = false;
        }
    }

    serde_json::json!({
        "healthy": healthy,
        "issues": issues
    })
}

/// 内核是否运行（process 可注入，便于测试）。
pub async fn is_kernel_running_with_process<R: tauri::Runtime>(
    process: &dyn KernelProcessControl<R>,
) -> Result<bool, String> {
    // 首先检查进程控制器中的进程句柄
    if process.is_running().await {
        return Ok(true);
    }

    // test-util 下可关闭平台层检测，避免系统残留 sing-box 进程干扰 hermetic 测试
    if !PLATFORM_KERNEL_DETECTION_ENABLED.load(Ordering::Relaxed) {
        return Ok(false);
    }

    // 使用平台抽象层检测外部启动的内核进程
    let kernel_name = platform::get_kernel_executable_name();
    match platform::is_process_running(kernel_name).await {
        Ok(running) => {
            if running {
                info!("通过平台抽象层检测到内核进程");
            } else {
                info!("内核运行状态检查: false (未找到相关进程)");
            }
            Ok(running)
        }
        Err(e) => {
            info!("平台进程检测失败: {}, 返回 false", e);
            Ok(false)
        }
    }
}

#[tauri::command]
pub async fn is_kernel_running() -> Result<bool, String> {
    is_kernel_running_with_process(&*process_controller()).await
}

#[tauri::command]
pub async fn get_system_uptime() -> Result<u64, String> {
    platform::get_system_uptime_ms().await
}

/// 将探测结果写回 KERNEL_STATE readiness（纯状态副作用，无 AppHandle）。
pub(crate) fn apply_probe_to_kernel_readiness(probe: &KernelRuntimeProbe) {
    let mut readiness = KERNEL_STATE.get_readiness();
    readiness.process_alive = probe.process_running;
    readiness.api_ready = probe.api_ready;
    readiness.relay_ready = probe.websocket_ready;
    KERNEL_STATE.set_readiness(readiness);
}

/// 组装状态 JSON（无 AppHandle；version/error 由调用方注入）。
pub(crate) fn build_status_payload_from_probe(
    probe: &KernelRuntimeProbe,
    version: Option<String>,
    diagnosis_message: Option<String>,
) -> serde_json::Value {
    let mut payload = KernelStatusPayload::from_state().to_json();
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("uptime_ms".to_string(), serde_json::json!(0));
        obj.insert("version".to_string(), serde_json::json!(version));
        obj.insert(
            "error".to_string(),
            serde_json::json!(diagnosis_message.or_else(|| probe.error.clone())),
        );
    }
    payload
}

/// 状态增强核心：探测 + readiness + payload（版本可注入，避免强依赖 Wry）。
#[allow(dead_code)]
pub(crate) async fn kernel_status_from_probe_and_version(
    api_port: Option<u16>,
    version_fallback: Option<String>,
) -> serde_json::Value {
    let port = api_port.unwrap_or(12081);
    let probe = collect_kernel_runtime_probe(port).await;
    apply_probe_to_kernel_readiness(&probe);

    let version = probe
        .version
        .clone()
        .or_else(|| version_fallback.map(|v| v.trim().to_string()));
    let diagnosis_message = KERNEL_STATE
        .get_startup_diagnosis()
        .map(|d| d.message.clone());
    build_status_payload_from_probe(&probe, version, diagnosis_message)
}

#[tauri::command]
pub async fn kernel_get_status_enhanced(
    app_handle: tauri::AppHandle,
    api_port: Option<u16>,
) -> Result<serde_json::Value, String> {
    let port = api_port.unwrap_or(12081);
    let probe = collect_kernel_runtime_probe(port).await;
    apply_probe_to_kernel_readiness(&probe);

    let mut version = probe.version.clone();
    // 运行时未获取到版本时回退到检查安装版本（DB/文件）
    if version.is_none() {
        if let Ok(v) =
            crate::app::core::kernel_service::versioning::check_kernel_version(app_handle).await
        {
            version = Some(v.trim().to_string());
        }
    }

    let diagnosis_message = KERNEL_STATE
        .get_startup_diagnosis()
        .map(|d| d.message.clone());
    Ok(build_status_payload_from_probe(
        &probe,
        version,
        diagnosis_message,
    ))
}

#[tauri::command]
pub async fn kernel_get_snapshot(
    app_handle: tauri::AppHandle,
    api_port: Option<u16>,
) -> Result<serde_json::Value, String> {
    // 快照接口复用增强状态接口，避免维护两套语义。
    kernel_get_status_enhanced(app_handle, api_port).await
}

#[tauri::command]
pub async fn kernel_check_health(api_port: Option<u16>) -> Result<serde_json::Value, String> {
    let kernel_path = paths::get_kernel_path();
    let config_path = paths::get_config_dir().join("config.json");
    let process_running = is_kernel_running().await.unwrap_or(false);
    let port = api_port.unwrap_or(12081);
    Ok(build_health_report(
        kernel_path.exists(),
        config_path.exists(),
        process_running,
        port,
    )
    .await)
}

#[cfg(test)]
#[path = "status.tests.rs"]
mod tests;
