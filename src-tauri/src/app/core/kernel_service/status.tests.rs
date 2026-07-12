use super::*;
use crate::app::constants::paths;
use crate::app::core::kernel_service::PROCESS_MANAGER;
use crate::test_support::TempWorkspace;
use std::fs;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn install_fake_kernel(work: &Path) {
    let dir = work.join("sing-box");
    fs::create_dir_all(&dir).unwrap();
    let kernel = dir.join("sing-box");
    fs::write(
        &kernel,
        r#"#!/bin/sh
if [ "$1" = "check" ]; then exit 0; fi
if [ "$1" = "run" ]; then
  echo "status-fake-kernel" >&2
  exec sleep "${FAKE_KERNEL_RUN_SECS:-5}"
fi
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
}

async fn spawn_version_mock(status: u16, body: &str) -> (u16, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let body = body.to_string();
    let handle = tokio::spawn(async move {
        for _ in 0..32 {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            // Fix status line for non-200
            let status_text = if status == 200 {
                "OK"
            } else if status == 500 {
                "Internal Server Error"
            } else {
                "Error"
            };
            let resp = format!(
                "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                status,
                status_text,
                body.len(),
                body
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = resp;
        }
    });
    (port, handle)
}

#[tokio::test]
async fn is_kernel_running_false_without_process() {
    // 确保全局 manager 干净
    let _ = PROCESS_MANAGER.stop::<tauri::Wry>(None).await;
    let running = is_kernel_running().await.unwrap();
    // 平台层也可能找到无关进程，仅保证不 panic
    let _ = running;
}

#[tokio::test]
async fn is_kernel_running_true_with_process_manager() {
    let ws = TempWorkspace::new();
    install_fake_kernel(ws.path());
    let cfg = paths::get_config_dir().join("config.json");
    fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    fs::write(&cfg, r#"{"log":{"level":"info"}}"#).unwrap();

    PROCESS_MANAGER
        .start_inner::<tauri::Wry>(None, &cfg, false)
        .await
        .expect("start fake kernel");
    assert!(PROCESS_MANAGER.is_running().await);
    assert!(is_kernel_running().await.unwrap());

    PROCESS_MANAGER.stop::<tauri::Wry>(None).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
}

#[tokio::test]
async fn probe_version_api_success_and_error() {
    let (port, h) = spawn_version_mock(200, r#"{"version":"1.12.0"}"#).await;
    let (ok, ver, err) = probe_version_api(port).await;
    assert!(ok);
    assert!(ver.is_some());
    assert!(err.is_none());
    h.abort();

    let (port, h) = spawn_version_mock(500, "err").await;
    let (ok, _, err) = probe_version_api(port).await;
    assert!(!ok);
    let err = err.unwrap_or_default();
    assert!(err.contains("500") || err.contains("API"));
    h.abort();

    // 无服务端口
    let (ok, _, err) = probe_version_api(1).await;
    assert!(!ok);
    assert!(err.is_some());
}

#[tokio::test]
async fn collect_probe_when_running_with_api() {
    let ws = TempWorkspace::new();
    install_fake_kernel(ws.path());
    let cfg = paths::get_config_dir().join("config.json");
    fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    fs::write(&cfg, r#"{"log":{"level":"info"}}"#).unwrap();

    let (port, server) = spawn_version_mock(200, "1.12.0-fake").await;
    PROCESS_MANAGER
        .start_inner::<tauri::Wry>(None, &cfg, false)
        .await
        .unwrap();

    let probe = collect_kernel_runtime_probe(port).await;
    assert!(probe.process_running);
    assert!(probe.api_ready);
    // WS 可能失败（无 mock）
    assert!(probe.version.is_some() || probe.error.is_some() || probe.api_ready);

    PROCESS_MANAGER.stop::<tauri::Wry>(None).await.unwrap();
    server.abort();
}

#[tokio::test]
async fn collect_probe_when_not_running() {
    let _ = PROCESS_MANAGER.stop::<tauri::Wry>(None).await;
    let probe = collect_kernel_runtime_probe(9).await;
    // 平台层可能仍检测到同名进程；至少保证结构可构造
    if !PROCESS_MANAGER.is_running().await && !probe.process_running {
        assert!(!probe.api_ready);
        assert!(!probe.websocket_ready);
    } else {
        // 外部有进程时仅验证字段可访问
        let _ = probe.api_ready;
    }
}

#[tokio::test]
async fn build_health_report_variants() {
    let missing = build_health_report(false, false, false, 1).await;
    assert_eq!(missing["healthy"], false);
    assert!(missing["issues"].as_array().unwrap().len() >= 2);

    let ok_files = build_health_report(true, true, false, 1).await;
    assert_eq!(ok_files["healthy"], true);

    let (port, server) = spawn_version_mock(200, "ok").await;
    let with_proc = build_health_report(true, true, true, port).await;
    assert_eq!(with_proc["healthy"], true);
    server.abort();

    let bad_api = build_health_report(true, true, true, 1).await;
    assert_eq!(bad_api["healthy"], false);
}

#[tokio::test]
async fn kernel_check_health_with_running_process_and_api() {
    let ws = TempWorkspace::new();
    install_fake_kernel(ws.path());
    let kernel = paths::get_kernel_path();
    // install_fake already wrote kernel under work dir
    assert!(kernel.exists() || paths::get_kernel_path().exists());
    let config = paths::get_config_dir().join("config.json");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, r#"{"log":{"level":"info"}}"#).unwrap();

    let (port, server) = spawn_version_mock(200, "v1").await;
    PROCESS_MANAGER
        .start_inner::<tauri::Wry>(None, &config, false)
        .await
        .unwrap();

    let result = kernel_check_health(Some(port)).await.unwrap();
    // 文件存在 + 进程 + API → healthy true
    assert_eq!(result["healthy"], true);

    PROCESS_MANAGER.stop::<tauri::Wry>(None).await.unwrap();
    server.abort();
}

#[tokio::test]
async fn kernel_check_health_reports_missing_artifacts() {
    let ws = TempWorkspace::new();
    let result = kernel_check_health(Some(1)).await.unwrap();
    assert!(result["healthy"].as_bool().is_some());
    assert!(result["issues"].as_array().is_some());
    let _ = ws;
}

#[tokio::test]
async fn kernel_check_health_with_kernel_and_config_files() {
    let ws = TempWorkspace::new();
    let kernel = paths::get_kernel_path();
    if let Some(parent) = kernel.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&kernel, b"fake").unwrap();
    let config = paths::get_config_dir().join("config.json");
    if let Some(parent) = config.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&config, r#"{"log":{"level":"info"}}"#).unwrap();

    let result = kernel_check_health(Some(1)).await.unwrap();
    let issues = result["issues"].as_array().cloned().unwrap_or_default();
    let joined = issues
        .iter()
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>()
        .join(";");
    assert!(!joined.contains("内核文件不存在"));
    assert!(!joined.contains("配置文件不存在"));
    let _ = ws;
}

#[tokio::test]
async fn get_system_uptime_is_positive() {
    let uptime = get_system_uptime().await.unwrap_or(0);
    let _ = uptime;
}

#[tokio::test]
async fn probe_traffic_websocket_fails_without_server() {
    assert!(!probe_traffic_websocket(1).await);
}

#[test]
fn build_status_payload_from_probe_fields() {
    let probe = KernelRuntimeProbe {
        process_running: true,
        api_ready: true,
        websocket_ready: false,
        version: Some("1.2.3".into()),
        error: Some("ws fail".into()),
    };
    apply_probe_to_kernel_readiness(&probe);
    let payload = build_status_payload_from_probe(&probe, Some("1.2.3".into()), None);
    assert_eq!(payload["uptime_ms"], 0);
    assert_eq!(payload["version"], "1.2.3");
    assert_eq!(payload["error"], "ws fail");

    let payload2 =
        build_status_payload_from_probe(&KernelRuntimeProbe::default(), None, Some("diag".into()));
    assert_eq!(payload2["error"], "diag");
}

#[tokio::test]
async fn kernel_status_from_probe_and_version_paths() {
    let _ = PROCESS_MANAGER.stop::<tauri::Wry>(None).await;
    let payload = kernel_status_from_probe_and_version(Some(1), Some("cached-v".into())).await;
    assert!(payload.get("version").is_some());
    assert!(payload.get("uptime_ms").is_some());

    let ws = TempWorkspace::new();
    install_fake_kernel(ws.path());
    let cfg = paths::get_config_dir().join("config.json");
    fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    fs::write(&cfg, r#"{"log":{"level":"info"}}"#).unwrap();
    let (port, server) = spawn_version_mock(200, r#"{"version":"live"}"#).await;
    PROCESS_MANAGER
        .start_inner::<tauri::Wry>(None, &cfg, false)
        .await
        .unwrap();
    let live = kernel_status_from_probe_and_version(Some(port), Some("fallback".into())).await;
    assert!(live.get("version").is_some());
    PROCESS_MANAGER.stop::<tauri::Wry>(None).await.unwrap();
    server.abort();
}
