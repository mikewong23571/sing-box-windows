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

#[test]
fn normalize_and_alias_proxy_modes() {
    assert_eq!(normalize_proxy_mode("rule"), Some("rule"));
    assert_eq!(normalize_proxy_mode(" global "), Some("global"));
    assert!(normalize_proxy_mode("direct").is_none()); // 当前仅 rule/global
    assert_eq!(normalize_proxy_mode("RULE"), Some("rule"));
    assert!(normalize_proxy_mode("nope").is_none());

    assert_eq!(clash_api_mode_alias("rule"), Some("Rule"));
    assert_eq!(clash_api_mode_alias("global"), Some("Global"));
    assert!(clash_api_mode_alias("direct").is_none());
    assert!(clash_api_mode_alias("x").is_none());
}

#[test]
fn read_mode_and_api_port_from_config_file() {
    let dir = create_temp_dir("mode-read");
    let path = dir.join("cfg.json");
    write_mode_config(&path, "global", Some("127.0.0.1:19090".into()));
    assert_eq!(read_proxy_mode_from_config(&path).unwrap(), "global");
    assert_eq!(read_api_port_from_config(&path).unwrap(), Some(19090));

    write_mode_config(&path, "rule", None);
    assert_eq!(read_proxy_mode_from_config(&path).unwrap(), "rule");
    assert!(read_api_port_from_config(&path).unwrap().is_none());

    let resolved = resolve_proxy_mode_config_path(Some(path.to_str().unwrap()));
    assert_eq!(resolved, path);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn modify_default_mode_creates_experimental_block() {
    let dir = create_temp_dir("mode-mod");
    let path = dir.join("empty.json");
    std::fs::write(&path, r#"{}"#).unwrap();
    modify_default_mode(&path, "rule".into(), Some(12081)).unwrap();
    let content: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(content["experimental"]["clash_api"]["default_mode"], "rule");
    assert!(content["experimental"]["clash_api"]["external_controller"]
        .as_str()
        .unwrap()
        .contains("12081"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn read_proxy_mode_defaults_and_invalid_mode() {
    let dir = create_temp_dir("mode-def");
    let path = dir.join("cfg.json");
    // 无 experimental → rule
    std::fs::write(&path, r#"{}"#).unwrap();
    assert_eq!(read_proxy_mode_from_config(&path).unwrap(), "rule");

    // 非法 mode 字符串回退 rule
    write_mode_config(&path, "Direct", Some("127.0.0.1:1".into()));
    assert_eq!(read_proxy_mode_from_config(&path).unwrap(), "rule");

    // 缺文件
    assert!(read_proxy_mode_from_config(&dir.join("missing.json")).is_err());

    // 非法 JSON
    std::fs::write(&path, "not-json").unwrap();
    assert!(read_proxy_mode_from_config(&path).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn read_api_port_variants() {
    let dir = create_temp_dir("mode-port");
    let path = dir.join("cfg.json");

    assert_eq!(
        read_api_port_from_config(&dir.join("nope.json")).unwrap(),
        None
    );

    // host:port
    write_mode_config(&path, "rule", Some("0.0.0.0:19091".into()));
    assert_eq!(read_api_port_from_config(&path).unwrap(), Some(19091));

    // 仅端口数字（rsplit 解析）
    write_mode_config(&path, "rule", Some("19092".into()));
    assert_eq!(read_api_port_from_config(&path).unwrap(), Some(19092));

    // 无端口字段
    std::fs::write(&path, r#"{"experimental":{"clash_api":{}}}"#).unwrap();
    assert_eq!(read_api_port_from_config(&path).unwrap(), None);

    // 非法 JSON
    std::fs::write(&path, "{").unwrap();
    assert!(read_api_port_from_config(&path).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn collect_proxy_mode_config_paths_dedupes_default() {
    let same = PathBuf::from("/tmp/config.json");
    let paths = collect_proxy_mode_config_paths(Some(same.as_path()), same.as_path());
    assert_eq!(paths.len(), 1);

    let paths2 = collect_proxy_mode_config_paths(None, same.as_path());
    assert_eq!(paths2, vec![same]);
}

#[test]
fn resolve_proxy_mode_config_path_none_uses_default() {
    let p = resolve_proxy_mode_config_path(None);
    assert!(p.ends_with("config.json"));
}

#[tokio::test]
async fn read_current_proxy_mode_empty_paths_falls_back() {
    // 空配置路径列表 + 无 API → 默认 rule
    let mode = read_current_proxy_mode_from_configs(&[], None)
        .await
        .expect("default");
    assert_eq!(mode, "rule");
}

#[tokio::test]
async fn patch_clash_api_mode_fails_when_server_rejects_both() {
    let (port, server) =
        spawn_sequence_mock_server(vec![("400 Bad Request", ""), ("400 Bad Request", "")]).await;
    let err = patch_clash_api_mode(port, "global").await;
    assert!(err.is_err());
    let _ = server.await;
}

#[tokio::test]
async fn toggle_and_get_proxy_mode_via_mock_app() {
    use crate::app::singbox::config_generator::generate_base_config;
    use crate::app::storage::enhanced_storage_service::get_enhanced_storage;
    use crate::app::storage::state_model::AppConfig;
    use crate::test_support::MockAppEnv;
    use std::fs;

    let env = MockAppEnv::new();
    let cfg_path = env.workspace.path().join("sing-box/config.json");
    fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
    let mut cfg = AppConfig::default();
    cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
    cfg.api_port = 19191;
    fs::write(
        &cfg_path,
        serde_json::to_string_pretty(&generate_base_config(&cfg)).unwrap(),
    )
    .unwrap();
    let db = env.workspace.path().join("mode.db");
    env.install_storage_from_path(db.to_str().unwrap()).await;
    get_enhanced_storage(&env.handle())
        .await
        .unwrap()
        .save_app_config(&cfg)
        .await
        .unwrap();

    // 无效模式
    assert!(toggle_proxy_mode_impl(env.handle(), "nope".into())
        .await
        .is_err());

    // 切换 rule/global（运行时同步可能失败，但磁盘必须更新）
    let msg = toggle_proxy_mode_impl(env.handle(), "global".into())
        .await
        .unwrap();
    assert!(msg.contains("global") || msg.contains("GLOBAL") || msg.contains("模式"));
    let content = fs::read_to_string(&cfg_path).unwrap();
    assert!(content.contains("global") || content.contains("Global"));

    let mode = get_current_proxy_mode_impl(env.handle()).await.unwrap();
    assert!(!mode.is_empty());
}
