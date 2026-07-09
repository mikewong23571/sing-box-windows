use super::*;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

struct CapturedRequest {
    method: String,
    path: String,
    body: String,
}

fn create_temp_dir(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "sing-box-windows-{label}-{}-{unique}-{counter}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("should create temp dir");
    dir
}

fn write_mode_config(config_path: &Path, mode: &str, external_controller: Option<String>) {
    let mut clash_api = serde_json::Map::new();
    clash_api.insert("default_mode".to_string(), json!(mode));

    if let Some(controller) = external_controller {
        clash_api.insert("external_controller".to_string(), json!(controller));
    }

    let config = json!({
        "experimental": {
            "clash_api": clash_api
        }
    });

    std::fs::write(
        config_path,
        serde_json::to_string_pretty(&config).expect("config should serialize"),
    )
    .expect("config should be written");
}

async fn spawn_mock_server(
    status_line: &str,
    body: &str,
) -> (u16, tokio::task::JoinHandle<CapturedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let port = listener
        .local_addr()
        .expect("listener should have local addr")
        .port();
    let status_line = status_line.to_string();
    let body = body.to_string();

    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("should accept request");
        let mut buffer = vec![0_u8; 4096];
        let size = socket.read(&mut buffer).await.expect("should read request");
        let request = String::from_utf8_lossy(&buffer[..size]).to_string();
        let mut parts = request.split("\r\n\r\n");
        let head = parts.next().unwrap_or_default();
        let body_text = parts.next().unwrap_or_default().to_string();
        let mut request_line = head.lines().next().unwrap_or_default().split_whitespace();
        let method = request_line.next().unwrap_or_default().to_string();
        let path = request_line.next().unwrap_or_default().to_string();

        let response = format!(
            "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("should write response");

        CapturedRequest {
            method,
            path,
            body: body_text,
        }
    });

    (port, handle)
}

async fn spawn_sequence_mock_server(
    responses: Vec<(&str, &str)>,
) -> (u16, tokio::task::JoinHandle<Vec<CapturedRequest>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let port = listener
        .local_addr()
        .expect("listener should have local addr")
        .port();
    let responses = responses
        .into_iter()
        .map(|(status, body)| (status.to_string(), body.to_string()))
        .collect::<Vec<_>>();

    let handle = tokio::spawn(async move {
        let mut requests = Vec::new();

        for (status_line, body) in responses {
            let (mut socket, _) = listener.accept().await.expect("should accept request");
            let mut buffer = vec![0_u8; 4096];
            let size = socket.read(&mut buffer).await.expect("should read request");
            let request = String::from_utf8_lossy(&buffer[..size]).to_string();
            let mut parts = request.split("\r\n\r\n");
            let head = parts.next().unwrap_or_default();
            let body_text = parts.next().unwrap_or_default().to_string();
            let mut request_line = head.lines().next().unwrap_or_default().split_whitespace();
            let method = request_line.next().unwrap_or_default().to_string();
            let path = request_line.next().unwrap_or_default().to_string();

            let response = format!(
                "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("should write response");

            requests.push(CapturedRequest {
                method,
                path,
                body: body_text,
            });
        }

        requests
    });

    (port, handle)
}

#[test]
fn collect_proxy_mode_config_paths_prefers_active_then_default() {
    let active_path = PathBuf::from("/tmp/active.json");
    let default_path = PathBuf::from("/tmp/config.json");

    let paths =
        collect_proxy_mode_config_paths(Some(active_path.as_path()), default_path.as_path());

    assert_eq!(paths, vec![active_path, default_path]);
}

#[tokio::test]
async fn read_current_proxy_mode_prefers_live_api_mode() {
    let temp_dir = create_temp_dir("mode-live");
    let config_path = temp_dir.join("active.json");
    let (port, server) = spawn_mock_server("200 OK", r#"{"mode":"Global"}"#).await;
    write_mode_config(&config_path, "rule", Some(format!("127.0.0.1:{port}")));

    let mode = read_current_proxy_mode_from_configs(&[config_path], None)
        .await
        .expect("mode should be read");

    assert_eq!(mode, "global");
    let request = server.await.expect("server should finish");
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/configs");
}

#[tokio::test]
async fn read_current_proxy_mode_falls_back_to_active_config_file() {
    let temp_dir = create_temp_dir("mode-fallback");
    let config_path = temp_dir.join("active.json");
    write_mode_config(&config_path, "Global", Some("127.0.0.1:1".to_string()));

    let mode = read_current_proxy_mode_from_configs(&[config_path], None)
        .await
        .expect("mode should fall back to config file");

    assert_eq!(mode, "global");
}

#[tokio::test]
async fn patch_clash_api_mode_sends_patch_request() {
    let (port, server) = spawn_mock_server("204 No Content", "").await;

    patch_clash_api_mode(port, "global")
        .await
        .expect("patch should succeed");

    let request = server.await.expect("server should finish");
    assert_eq!(request.method, "PATCH");
    assert_eq!(request.path, "/configs");
    assert!(request.body.contains(r#""mode":"global""#));
}

#[tokio::test]
async fn patch_clash_api_mode_retries_title_case_when_lowercase_is_rejected() {
    let (port, server) =
        spawn_sequence_mock_server(vec![("400 Bad Request", ""), ("204 No Content", "")]).await;

    patch_clash_api_mode(port, "global")
        .await
        .expect("patch should retry with compatibility casing");

    let requests = server.await.expect("server should finish");
    assert_eq!(requests.len(), 2);
    assert!(requests[0].body.contains(r#""mode":"global""#));
    assert!(requests[1].body.contains(r#""mode":"Global""#));
}
