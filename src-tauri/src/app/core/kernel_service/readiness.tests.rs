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
  echo "ready-fake" >&2
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

#[test]
fn classify_startup_stability_failure_variants() {
    let (c, msg) = classify_startup_stability_failure("API status 500");
    assert_eq!(c, "KERNEL_API_HTTP_ERROR");
    assert!(!msg.is_empty());
    let (c, _) = classify_startup_stability_failure("exited immediately after start");
    assert_eq!(c, "KERNEL_PROCESS_EXITED_EARLY");
    let (c, _) = classify_startup_stability_failure("something else");
    assert_eq!(c, "KERNEL_API_TIMEOUT");
}

#[test]
fn stability_check_config_default() {
    let d = StabilityCheckConfig::default();
    assert_eq!(d.max_checks, 10);
    assert_eq!(d.api_timeout_ms, 1000);
}

#[tokio::test]
async fn verify_stability_fails_when_process_not_running() {
    let _ = PROCESS_MANAGER.stop::<tauri::Wry>(None).await;
    // 收紧配置：1 次尝试、短超时
    let cfg = StabilityCheckConfig {
        max_checks: 1,
        initial_retry_interval_ms: 10,
        max_retry_interval_ms: 10,
        api_timeout_ms: 50,
    };
    // 仅当全局确实无进程时断言
    if !PROCESS_MANAGER.is_running().await {
        let err = verify_kernel_startup_stability_with_config(9, cfg)
            .await
            .unwrap_err();
        assert!(err.contains("exited immediately") || err.contains("API"));
    }
}

#[tokio::test]
async fn verify_stability_succeeds_with_running_kernel_and_mock_api() {
    let ws = TempWorkspace::new();
    install_fake_kernel(ws.path());
    let cfg_path = paths::get_config_dir().join("config.json");
    fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
    fs::write(&cfg_path, r#"{"log":{"level":"info"}}"#).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        for _ in 0..8 {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let mut buf = [0u8; 512];
            let _ = sock.read(&mut buf).await;
            let body = r#"{"version":"1.0.0"}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });

    PROCESS_MANAGER
        .start_inner::<tauri::Wry>(None, &cfg_path, false)
        .await
        .unwrap();
    assert!(PROCESS_MANAGER.is_running().await);

    let cfg = StabilityCheckConfig {
        max_checks: 3,
        initial_retry_interval_ms: 20,
        max_retry_interval_ms: 50,
        api_timeout_ms: 500,
    };
    verify_kernel_startup_stability_with_config(port, cfg)
        .await
        .expect("stability should pass");

    // 默认入口也走一遍（可能稍慢）
    let _ = verify_kernel_startup_stability(port).await;

    PROCESS_MANAGER.stop::<tauri::Wry>(None).await.unwrap();
    server.abort();
}

#[tokio::test]
async fn verify_stability_reports_http_error_status() {
    let ws = TempWorkspace::new();
    install_fake_kernel(ws.path());
    let cfg_path = paths::get_config_dir().join("config.json");
    fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
    fs::write(&cfg_path, r#"{"log":{"level":"info"}}"#).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        for _ in 0..16 {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let mut buf = [0u8; 512];
            let _ = sock.read(&mut buf).await;
            let resp =
                "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });

    PROCESS_MANAGER
        .start_inner::<tauri::Wry>(None, &cfg_path, false)
        .await
        .unwrap();

    let cfg = StabilityCheckConfig {
        max_checks: 2,
        initial_retry_interval_ms: 10,
        max_retry_interval_ms: 20,
        api_timeout_ms: 200,
    };
    let err = verify_kernel_startup_stability_with_config(port, cfg)
        .await
        .unwrap_err();
    assert!(
        err.contains("API status") || err.contains("503") || err.contains("failed"),
        "unexpected err: {}",
        err
    );
    let (code, _) = classify_startup_stability_failure(&err);
    // 若文案含 API status 则映射 HTTP 错误，否则超时类
    let _ = code;

    PROCESS_MANAGER.stop::<tauri::Wry>(None).await.unwrap();
    server.abort();
}
