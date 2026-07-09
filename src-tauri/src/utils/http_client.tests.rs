use super::*;
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn spawn_json_server(body: &'static str, status: u16) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            let resp = format!(
                "HTTP/1.1 {} OK\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/json\r\n\r\n{}",
                status,
                body.len(),
                body
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });
    port
}

#[test]
fn http_client_manager_new_and_default() {
    let m = HttpClientManager::new();
    let _ = m.get_client();
    let _ = m.get_proxy_client();
    let d = HttpClientManager::default();
    let _ = d.get_client();
    // 全局单例可访问
    let _ = get_client();
    let _ = get_proxy_client();
}

#[tokio::test]
async fn http_client_get_text_and_json_local() {
    #[derive(Debug, Deserialize)]
    struct Sample {
        v: u32,
    }

    let port = spawn_json_server(r#"{"v":42}"#, 200).await;
    let url = format!("http://127.0.0.1:{}/j", port);
    let mgr = HttpClientManager::new();

    let text = mgr.get_text(&url).await.unwrap();
    assert!(text.contains("42"));

    let parsed: Sample = mgr.get_json(&url).await.unwrap();
    assert_eq!(parsed.v, 42);

    // 便捷包装
    let text2 = get_text(&url).await.unwrap();
    assert!(text2.contains("v"));
    let parsed2: Sample = get_json(&url).await.unwrap();
    assert_eq!(parsed2.v, 42);
}

#[tokio::test]
async fn http_client_download_file_local() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let body = b"http-client-bytes";
    tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = [0u8; 512];
            let _ = sock.read(&mut buf).await;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.write_all(body).await;
        }
    });

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("out.bin");
    let url = format!("http://127.0.0.1:{}/f", port);
    let mgr = HttpClientManager::new();
    mgr.download_file(&url, dest.to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), body);

    // 便捷包装再下一次（需新 server）
    let listener2 = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port2 = listener2.local_addr().unwrap().port();
    tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener2.accept().await {
            let mut buf = [0u8; 512];
            let _ = sock.read(&mut buf).await;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.write_all(body).await;
        }
    });
    let dest2 = dir.path().join("out2.bin");
    download_file(
        &format!("http://127.0.0.1:{}/f2", port2),
        dest2.to_str().unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&dest2).unwrap(), body);
}

#[tokio::test]
async fn http_client_get_error_status() {
    let port = spawn_json_server("err", 500).await;
    let url = format!("http://127.0.0.1:{}/e", port);
    let mgr = HttpClientManager::new();
    assert!(mgr.get_text(&url).await.is_err());
    assert!(mgr.get_json::<serde_json::Value>(&url).await.is_err());
}

#[tokio::test]
async fn http_client_test_connectivity_without_proxy() {
    let port = spawn_json_server("ok", 200).await;
    let url = format!("http://127.0.0.1:{}/ping", port);
    let mgr = HttpClientManager::new();
    let elapsed = mgr.test_connectivity(&url, None).await.unwrap();
    assert!(elapsed.as_secs() < 5);
    let elapsed2 = test_connectivity(&url, None).await.unwrap();
    assert!(elapsed2.as_secs() < 5);
}
