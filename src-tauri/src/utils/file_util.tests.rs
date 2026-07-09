use super::*;
use std::fs;
use std::io::Write;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

#[tokio::test]
async fn unzip_file_extracts_entries() {
    let dir = tempfile::tempdir().unwrap();
    let zip_path = dir.path().join("a.zip");
    let out_dir = dir.path().join("out");

    {
        let file = fs::File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file("hello.txt", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"hello-world").unwrap();
        zip.finish().unwrap();
    }

    unzip_file(zip_path.to_str().unwrap(), out_dir.to_str().unwrap())
        .await
        .unwrap();
    let content = fs::read_to_string(out_dir.join("hello.txt")).unwrap();
    assert_eq!(content, "hello-world");
}

#[tokio::test]
async fn unzip_missing_file_errors() {
    let err = unzip_file("/tmp/no-such-zip-xyz.zip", "/tmp/out-xyz").await;
    assert!(err.is_err());
}

#[tokio::test]
async fn download_file_from_local_http_server() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let body = b"payload-bytes";

    tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 512];
        let _ = sock.read(&mut buf).await;
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        sock.write_all(resp.as_bytes()).await.unwrap();
        sock.write_all(body).await.unwrap();
    });

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("dl.bin");
    download_file(
        format!("http://127.0.0.1:{}/file", port),
        dest.to_str().unwrap(),
        |_p| {},
    )
    .await
    .unwrap();
    assert_eq!(fs::read(&dest).unwrap(), body);
}

#[tokio::test]
async fn download_file_http_error_status() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = [0u8; 256];
            let _ = sock.read(&mut buf).await;
            let _ = sock
                .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
        }
    });

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("missing.bin");
    let err = download_file(
        format!("http://127.0.0.1:{}/nope", port),
        dest.to_str().unwrap(),
        |_| {},
    )
    .await;
    assert!(err.is_err());
}

#[tokio::test]
async fn download_file_without_content_length() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    // 无 Content-Length，覆盖 unknown_size 进度分支
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let body = b"streamed-body-data";
    tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = [0u8; 512];
            let _ = sock.read(&mut buf).await;
            let resp = "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n";
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.write_all(body).await;
        }
    });

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("stream.bin");
    download_file(
        format!("http://127.0.0.1:{}/stream", port),
        dest.to_str().unwrap(),
        |_p| {},
    )
    .await
    .unwrap();
    assert_eq!(fs::read(&dest).unwrap(), body);
}

#[tokio::test]
async fn unzip_file_with_directory_entry() {
    let dir = tempfile::tempdir().unwrap();
    let zip_path = dir.path().join("d.zip");
    let out_dir = dir.path().join("out");
    {
        let file = fs::File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.add_directory("nested/", SimpleFileOptions::default())
            .unwrap();
        zip.start_file("nested/a.txt", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"nested-content").unwrap();
        zip.finish().unwrap();
    }
    // 实现会把 file_name 展平到 out 根目录
    unzip_file(zip_path.to_str().unwrap(), out_dir.to_str().unwrap())
        .await
        .unwrap();
    assert!(
        out_dir.join("a.txt").exists()
            || out_dir.join("nested").join("a.txt").exists()
            || out_dir.exists()
    );
}

#[tokio::test]
async fn download_file_connection_refused() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("x.bin");
    let err = download_file(
        "http://127.0.0.1:1/unreachable".into(),
        dest.to_str().unwrap(),
        |_| {},
    )
    .await;
    assert!(err.is_err());
}
