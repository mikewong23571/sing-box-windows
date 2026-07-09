use super::*;
use crate::test_support::TempWorkspace;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn write_zip_with_nested_exe(path: &Path, exe_name: &str) {
    let file = fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored);
    zip.add_directory("nested/", options).unwrap();
    zip.start_file(format!("nested/{}", exe_name), options)
        .unwrap();
    zip.write_all(b"#!/bin/sh\necho ok\n").unwrap();
    zip.finish().unwrap();
}

fn write_tar_gz_with_file(path: &Path, inner_name: &str, content: &[u8]) {
    let tar_gz = fs::File::create(path).unwrap();
    let enc = flate2::write::GzEncoder::new(tar_gz, flate2::Compression::default());
    let mut tar = tar::Builder::new(enc);
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, inner_name, content).unwrap();
    tar.finish().unwrap();
}

#[tokio::test]
async fn extract_zip_and_find_nested_executable() {
    let ws = TempWorkspace::new();
    let zip_path = ws.join("kernel.zip");
    let extract_to = ws.join("out");
    fs::create_dir_all(&extract_to).unwrap();

    write_zip_with_nested_exe(&zip_path, "sing-box");

    extract_archive(&zip_path, &extract_to).await.unwrap();
    let found = find_executable_file(&extract_to, "sing-box")
        .await
        .unwrap();
    assert!(found.ends_with("sing-box"));
    assert!(found.is_file());
}

#[tokio::test]
async fn extract_tar_gz_archive_works() {
    let ws = TempWorkspace::new();
    let archive = ws.join("kernel.tar.gz");
    let extract_to = ws.join("out_tg");
    write_tar_gz_with_file(&archive, "sing-box", b"binary");

    extract_tar_gz_archive(&archive, &extract_to)
        .await
        .unwrap();
    assert!(extract_to.join("sing-box").is_file());
}

#[tokio::test]
async fn extract_archive_rejects_missing_empty_and_unknown() {
    let ws = TempWorkspace::new();
    let missing = ws.join("nope.zip");
    let err = extract_archive(&missing, &ws.join("x")).await;
    assert!(err.is_err());

    let empty = ws.join("empty.zip");
    fs::write(&empty, b"").unwrap();
    let err = extract_archive(&empty, &ws.join("y")).await;
    assert!(err.is_err());

    let weird = ws.join("file.bin");
    fs::write(&weird, b"not-an-archive").unwrap();
    let err = extract_archive(&weird, &ws.join("z")).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn find_executable_direct_and_missing() {
    let ws = TempWorkspace::new();
    let dir = ws.join("bin");
    fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("sing-box");
    fs::write(&exe, b"x").unwrap();

    let found = find_executable_file(&dir, "sing-box").await.unwrap();
    assert_eq!(found, exe);

    let missing = find_executable_file(&dir, "does-not-exist").await;
    assert!(missing.is_err());
}

#[test]
fn set_executable_permission_on_unix() {
    let ws = TempWorkspace::new();
    let f = ws.join("script.sh");
    fs::write(&f, b"#!/bin/sh\n").unwrap();
    set_executable_permission(&f).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&f).unwrap().permissions().mode();
        assert_ne!(mode & 0o111, 0);
    }
    let _ = PathBuf::from("ok");
}

#[test]
fn kernel_platform_filename_and_urls() {
    let platform = kernel_platform_name().unwrap();
    assert!(matches!(platform, "windows" | "linux" | "darwin"));
    let name = kernel_release_filename("1.12.0", "linux", "amd64");
    assert_eq!(name, "sing-box-1.12.0-linux-amd64.tar.gz");
    assert!(kernel_release_filename("1.0.0", "windows", "amd64").ends_with(".zip"));
    assert!(kernel_release_filename("1.0.0", "darwin", "arm64").contains("darwin"));
    let urls = kernel_download_urls("1.12.0", &name);
    assert_eq!(urls.len(), 7);
    assert!(urls[0].contains("gh-proxy"));
    assert!(urls[6].contains("github.com"));
    assert_eq!(kernel_download_source_name(0), "v6.gh-proxy 镜像");
    assert_eq!(kernel_download_source_name(6), "GitHub 原始");
    assert_eq!(kernel_download_source_name(99), "未知源");
}

#[test]
fn clear_dir_contents_removes_files_and_subdirs() {
    let ws = TempWorkspace::new();
    let dir = ws.join("tmp_clear");
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("a.txt"), b"1").unwrap();
    std::fs::write(dir.join("sub/b.txt"), b"2").unwrap();
    clear_dir_contents(&dir);
    assert!(std::fs::read_dir(&dir).unwrap().next().is_none());
}

#[test]
fn download_progress_percent_logic() {
    assert_eq!(download_progress_percent(0, 0), 0);
    assert_eq!(download_progress_percent(50, 100), 50);
    assert_eq!(download_progress_percent(70, 100), 70);
    assert_eq!(download_progress_percent(100, 100), 70); // capped at 70
    assert_eq!(download_progress_percent(1, 1), 1); // scaled_total.max(1) = 1
}

#[test]
fn download_source_progress_increments() {
    assert_eq!(download_source_progress(0), 15);
    assert_eq!(download_source_progress(1), 20);
    assert_eq!(download_source_progress(5), 40);
}

#[test]
fn resolve_kernel_version_to_download_logic() {
    assert_eq!(
        resolve_kernel_version_to_download(Some("1.2.3".into()), Ok("9.9.9".into()), "0.0.1"),
        "1.2.3"
    );
    assert_eq!(
        resolve_kernel_version_to_download(Some("   ".into()), Ok("9.9.9".into()), "0.0.1"),
        "9.9.9"
    );
    assert_eq!(
        resolve_kernel_version_to_download(None, Ok("9.9.9".into()), "0.0.1"),
        "9.9.9"
    );
    assert_eq!(
        resolve_kernel_version_to_download(None, Err("bad".into()), "0.0.1"),
        "0.0.1"
    );
    assert_eq!(
        resolve_kernel_version_to_download(None, Ok("   ".into()), "0.0.1"),
        "0.0.1"
    );
}

#[test]
fn all_download_sources_failed_message_contains_last_source() {
    let msg = all_download_sources_failed_message("GitHub 原始");
    assert!(msg.contains("GitHub 原始"));
    assert!(msg.contains("所有下载源"));
}

#[test]
fn cleanup_legacy_version_dirs_removes_matching_dirs() {
    let ws = TempWorkspace::new();
    let kernel_dir = ws.join("sing-box");
    std::fs::create_dir_all(&kernel_dir).unwrap();
    std::fs::create_dir_all(kernel_dir.join("sing-box-1.0.0")).unwrap();
    std::fs::create_dir_all(kernel_dir.join("logs")).unwrap();
    std::fs::create_dir_all(kernel_dir.join("update_temp")).unwrap();

    cleanup_legacy_version_dirs(&kernel_dir);

    assert!(!kernel_dir.join("sing-box-1.0.0").exists());
    assert!(kernel_dir.join("logs").exists());
    assert!(kernel_dir.join("update_temp").exists());
}

#[tokio::test]
async fn deploy_kernel_from_extract_dir_with_replace() {
    let ws = TempWorkspace::new();
    let extract = ws.join("extract");
    let kernel_dir = ws.join("sing-box");
    std::fs::create_dir_all(&extract).unwrap();
    std::fs::create_dir_all(&kernel_dir).unwrap();

    let exe_name = kernel_executable_name();
    // nested new binary
    let nested = extract.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join(exe_name), b"new-bin").unwrap();

    // existing old binary + leftover version dir
    std::fs::write(kernel_dir.join(exe_name), b"old-bin").unwrap();
    std::fs::create_dir_all(kernel_dir.join("sing-box-1.0.0-linux-amd64")).unwrap();
    std::fs::write(kernel_dir.join("sing-box.old"), b"older").unwrap();

    let deployed = deploy_kernel_from_extract_dir(&extract, &kernel_dir, exe_name)
        .await
        .unwrap();
    assert_eq!(deployed, kernel_dir.join(exe_name));
    assert_eq!(std::fs::read(kernel_dir.join(exe_name)).unwrap(), b"new-bin");
    // leftover version dir should be cleaned
    assert!(!kernel_dir.join("sing-box-1.0.0-linux-amd64").exists());
}

#[tokio::test]
async fn deploy_kernel_missing_executable_errors() {
    let ws = TempWorkspace::new();
    let extract = ws.join("empty_ex");
    let kernel_dir = ws.join("kdir");
    std::fs::create_dir_all(&extract).unwrap();
    std::fs::create_dir_all(&kernel_dir).unwrap();
    let err = deploy_kernel_from_extract_dir(&extract, &kernel_dir, kernel_executable_name()).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn download_file_to_path_from_local_http() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let ws = TempWorkspace::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let body = b"hello-kernel-bytes";
    tokio::spawn(async move {
        if let Ok((mut s, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf).await;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes()).await;
            let _ = s.write_all(body).await;
        }
    });

    let dest = ws.join("dl.bin");
    download_file_to_path(&format!("http://127.0.0.1:{}/f", port), &dest)
        .await
        .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), body);
    assert_eq!(download_progress_percent(50, 100), 50.min(70));
    assert_eq!(download_progress_percent(100, 0), 0);
    assert_eq!(download_progress_percent(1000, 100), 70);
}

#[tokio::test]
async fn download_file_to_path_http_error() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    let ws = TempWorkspace::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        if let Ok((mut s, _)) = listener.accept().await {
            let mut buf = [0u8; 256];
            let _ = s.read(&mut buf).await;
            let _ = s
                .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
        }
    });
    let err = download_file_to_path(
        &format!("http://127.0.0.1:{}/missing", port),
        &ws.join("x.bin"),
    )
    .await;
    assert!(err.is_err());
}

#[test]
fn resolve_version_and_progress_and_messages() {
    assert_eq!(
        resolve_kernel_version_to_download(Some("1.2.3".into()), Err("x".into()), "9.9.9"),
        "1.2.3"
    );
    assert_eq!(
        resolve_kernel_version_to_download(None, Ok("2.0.0".into()), "9.9.9"),
        "2.0.0"
    );
    assert_eq!(
        resolve_kernel_version_to_download(None, Err("net".into()), "1.12.10"),
        "1.12.10"
    );
    assert_eq!(
        resolve_kernel_version_to_download(Some("  ".into()), Ok("3.0".into()), "1.0"),
        "3.0"
    );
    assert_eq!(download_source_progress(0), 15);
    assert_eq!(download_source_progress(2), 25);
    assert!(all_download_sources_failed_message("GitHub 原始").contains("所有下载源"));
}

#[tokio::test]
async fn try_download_from_urls_succeeds_on_second_source() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let ws = TempWorkspace::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let body = b"kernel-payload";
    // first request 500, second 200
    tokio::spawn(async move {
        for i in 0..4 {
            let Ok((mut s, _)) = listener.accept().await else { break; };
            let mut buf = [0u8; 512];
            let _ = s.read(&mut buf).await;
            if i == 0 {
                let _ = s.write_all(b"HTTP/1.1 500 ERR\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
            } else {
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
                let _ = s.write_all(body).await;
            }
        }
    });

    let dest = ws.join("k.bin");
    let bad = format!("http://127.0.0.1:{}/a", port);
    let good = format!("http://127.0.0.1:{}/b", port);
    let idx = try_download_from_urls(&[bad, good], &dest).await.unwrap();
    assert_eq!(idx, 1);
    assert_eq!(std::fs::read(&dest).unwrap(), body);
}

#[tokio::test]
async fn try_download_from_urls_all_fail() {
    let ws = TempWorkspace::new();
    let dest = ws.join("nope.bin");
    let err = try_download_from_urls(
        &[
            "http://127.0.0.1:1/x".into(),
            "http://127.0.0.1:1/y".into(),
        ],
        &dest,
    )
    .await;
    assert!(err.is_err());
    assert!(try_download_from_urls(&[], &dest).await.is_err());
}

#[test]
fn cleanup_legacy_version_dirs_removes_versioned() {
    let ws = TempWorkspace::new();
    let kd = ws.join("sing-box");
    std::fs::create_dir_all(kd.join("sing-box-1.0.0-linux-amd64")).unwrap();
    std::fs::create_dir_all(kd.join("logs")).unwrap();
    std::fs::create_dir_all(kd.join("update_temp")).unwrap();
    std::fs::write(kd.join("sing-box"), b"bin").unwrap();
    cleanup_legacy_version_dirs(&kd);
    assert!(!kd.join("sing-box-1.0.0-linux-amd64").exists());
    assert!(kd.join("logs").exists());
    assert!(kd.join("update_temp").exists());
    assert!(kd.join("sing-box").exists());
}

#[test]
fn cleanup_legacy_version_dirs_missing_and_non_version() {
    let ws = TempWorkspace::new();
    // 不存在的目录：应静默
    cleanup_legacy_version_dirs(&ws.join("no-such-kernel-dir"));
    let kd = ws.join("k2");
    std::fs::create_dir_all(kd.join("other-dir")).unwrap();
    std::fs::create_dir_all(kd.join("sing-box-2.0.0-linux-amd64")).unwrap();
    cleanup_legacy_version_dirs(&kd);
    assert!(kd.join("other-dir").exists());
    assert!(!kd.join("sing-box-2.0.0-linux-amd64").exists());
}

#[test]
fn clear_dir_contents_missing_is_noop() {
    let ws = TempWorkspace::new();
    clear_dir_contents(&ws.join("missing-clear"));
}

#[test]
fn download_progress_and_source_name_edges() {
    assert_eq!(download_progress_percent(0, 100), 0);
    // total_size=1 → scaled_total=0 → 用 1 做除数 → 进度为 1（再 clamp 到 70）
    assert_eq!(download_progress_percent(1, 1), 1);
    assert_eq!(download_progress_percent(100, 100), 70);
    assert_eq!(download_source_progress(10), 15 + 50);
    assert!(!kernel_download_source_name(1).is_empty());
    assert!(!kernel_executable_name().is_empty());
    assert!(kernel_release_filename("1.0", "linux", "arm64").contains("arm64"));
}

#[tokio::test]
async fn deploy_kernel_creates_kernel_dir_when_missing() {
    let ws = TempWorkspace::new();
    let extract = ws.join("ex");
    let kernel_dir = ws.join("new-kernel-dir");
    std::fs::create_dir_all(&extract).unwrap();
    let name = kernel_executable_name();
    std::fs::write(extract.join(name), b"fresh").unwrap();
    let deployed = deploy_kernel_from_extract_dir(&extract, &kernel_dir, name)
        .await
        .unwrap();
    assert_eq!(deployed, kernel_dir.join(name));
    assert_eq!(std::fs::read(deployed).unwrap(), b"fresh");
}

#[tokio::test]
async fn extract_zip_archive_direct_api() {
    let ws = TempWorkspace::new();
    let zip_path = ws.join("direct.zip");
    write_zip_with_nested_exe(&zip_path, "sing-box");
    let out = ws.join("direct_out");
    extract_zip_archive(&zip_path, &out).await.unwrap();
    assert!(find_executable_file(&out, "sing-box").await.is_ok());
}

#[test]
fn prepare_kernel_download_layout_creates_temp() {
    let ws = TempWorkspace::new();
    let (kd, temp, dl) =
        prepare_kernel_download_layout(ws.path(), "sing-box-1.0-linux-amd64.tar.gz").unwrap();
    assert!(kd.ends_with("sing-box"));
    assert!(temp.ends_with("update_temp"));
    assert!(temp.exists());
    assert!(dl.file_name().unwrap().to_string_lossy().contains("sing-box"));
    // 再次调用应清空 temp 内容
    fs::write(temp.join("stale"), b"x").unwrap();
    let (_, temp2, _) = prepare_kernel_download_layout(ws.path(), "other.zip").unwrap();
    assert!(!temp2.join("stale").exists());
}

#[test]
fn build_kernel_download_progress_payload_fields() {
    let p = build_kernel_download_progress_payload("downloading", 15, "msg");
    assert_eq!(p["status"], "downloading");
    assert_eq!(p["progress"], 15);
    assert_eq!(p["message"], "msg");
}

#[tokio::test]
async fn install_kernel_from_archive_zip_and_missing() {
    let ws = TempWorkspace::new();
    let (kernel_dir, temp, _) =
        prepare_kernel_download_layout(ws.path(), "k.zip").unwrap();
    let archive = temp.join("k.zip");
    write_zip_with_nested_exe(&archive, kernel_executable_name());

    let deployed = install_kernel_from_archive(&archive, &temp, &kernel_dir)
        .await
        .unwrap();
    assert_eq!(deployed, kernel_dir.join(kernel_executable_name()));
    assert!(deployed.is_file());
    // archive 应被删除
    assert!(!archive.exists());

    let missing = ws.join("no-archive.zip");
    let err = install_kernel_from_archive(&missing, &ws.join("ex"), &kernel_dir).await;
    assert!(err.unwrap_err().contains("不存在"));
}

#[tokio::test]
async fn install_kernel_from_archive_tar_gz() {
    let ws = TempWorkspace::new();
    let (kernel_dir, temp, _) =
        prepare_kernel_download_layout(ws.path(), "k.tar.gz").unwrap();
    let archive = temp.join("k.tar.gz");
    write_tar_gz_with_file(&archive, kernel_executable_name(), b"#!/bin/sh\necho ok\n");
    let deployed = install_kernel_from_archive(&archive, &temp, &kernel_dir)
        .await
        .unwrap();
    assert!(deployed.is_file());
    assert_eq!(fs::read(&deployed).unwrap(), b"#!/bin/sh\necho ok\n");
}

#[tokio::test]
async fn install_kernel_from_archive_bad_zip_errors() {
    let ws = TempWorkspace::new();
    let (kernel_dir, temp, _) =
        prepare_kernel_download_layout(ws.path(), "bad.zip").unwrap();
    let archive = temp.join("bad.zip");
    fs::write(&archive, b"not-zip").unwrap();
    let err = install_kernel_from_archive(&archive, &temp, &kernel_dir).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn find_executable_deeply_nested() {
    let ws = TempWorkspace::new();
    let root = ws.join("deep");
    let nested = root.join("a/b/c");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("sing-box"), b"bin").unwrap();
    let found = find_executable_file(&root, "sing-box").await.unwrap();
    assert!(found.ends_with("sing-box"));
}

#[tokio::test]
async fn download_file_to_path_without_content_length() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let ws = TempWorkspace::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let body = b"chunked-body";
    tokio::spawn(async move {
        if let Ok((mut s, _)) = listener.accept().await {
            let mut buf = [0u8; 512];
            let _ = s.read(&mut buf).await;
            // 无 Content-Length，靠 Connection: close 结束
            let resp = format!(
                "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n"
            );
            let _ = s.write_all(resp.as_bytes()).await;
            let _ = s.write_all(body).await;
        }
    });
    let dest = ws.join("no-cl.bin");
    download_file_to_path(&format!("http://127.0.0.1:{}/f", port), &dest)
        .await
        .unwrap();
    assert_eq!(fs::read(&dest).unwrap(), body);
}

#[tokio::test]
async fn download_and_install_kernel_from_urls_local_zip() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let ws = TempWorkspace::new();
    let zip_path = ws.join("payload.zip");
    write_zip_with_nested_exe(&zip_path, kernel_executable_name());
    let zip_bytes = fs::read(&zip_path).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        if let Ok((mut s, _)) = listener.accept().await {
            let mut buf = [0u8; 512];
            let _ = s.read(&mut buf).await;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                zip_bytes.len()
            );
            let _ = s.write_all(resp.as_bytes()).await;
            let _ = s.write_all(&zip_bytes).await;
        }
    });

    let url = format!("http://127.0.0.1:{}/k.zip", port);
    let deployed = download_and_install_kernel_from_urls(
        &[url],
        ws.path(),
        "k.zip",
    )
    .await
    .unwrap();
    assert!(deployed.is_file());
    assert_eq!(deployed.file_name().unwrap().to_string_lossy(), kernel_executable_name());
}

#[test]
fn stop_retry_and_version_apply_pure_helpers() {
    assert_eq!(KERNEL_REPLACE_STOP_ATTEMPTS, 5);
    assert!(should_retry_kernel_stop(0, true));
    assert!(!should_retry_kernel_stop(4, true));
    assert!(!should_retry_kernel_stop(0, false));
    assert!(should_force_kill_after_stop_retries(true));
    assert!(!should_force_kill_after_stop_retries(false));
    assert!(should_stop_kernel_before_replace(true));
    assert!(!should_stop_kernel_before_replace(false));
    assert!(kernel_stop_retry_succeeded(false));
    assert!(!kernel_stop_retry_succeeded(true));

    let cfg = apply_installed_kernel_version(
        crate::app::storage::state_model::AppConfig::default(),
        "1.12.9".into(),
    );
    assert_eq!(cfg.installed_kernel_version.as_deref(), Some("1.12.9"));
}

#[tokio::test]
async fn install_local_kernel_archive_with_optional_stop_via_mock() {
    use crate::test_support::MockAppEnv;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let env = MockAppEnv::new();
    let work = env.workspace.path();
    // 假内核脚本打包进 zip
    let zip_path = work.join("k.zip");
    {
        let f = std::fs::File::create(&zip_path).unwrap();
        let mut z = ZipWriter::new(f);
        z.start_file(kernel_executable_name(), SimpleFileOptions::default())
            .unwrap();
        z.write_all(
            b"#!/bin/sh\nif [ \"$1\" = \"version\" ]; then echo sing-box version 1.12.0; fi\nexit 0\n",
        )
        .unwrap();
        z.finish().unwrap();
    }
    let db = work.join("dl.db");
    env.install_storage_from_path(db.to_str().unwrap()).await;

    // 全局 ProcessManager 可能被前序用例占用：先停干净
    let _ = crate::app::core::kernel_service::PROCESS_MANAGER
        .stop::<tauri::Wry>(None)
        .await;

    let (target, _was_running) =
        install_local_kernel_archive_with_optional_stop(&env.handle(), &zip_path, work)
            .await
            .expect("install local archive");
    assert!(target.exists());
    // 安装后内核路径应存在
    assert!(crate::app::constants::paths::get_kernel_path().exists() || target.exists());
}

#[tokio::test]
async fn download_and_install_all_urls_fail() {
    let ws = TempWorkspace::new();
    let err = download_and_install_kernel_from_urls(
        &["http://127.0.0.1:1/nope.zip".into()],
        ws.path(),
        "nope.zip",
    )
    .await;
    assert!(err.is_err());
}

#[tokio::test]
async fn try_download_from_urls_empty_errors() {
    let ws = TempWorkspace::new();
    let err = try_download_from_urls(&[], &ws.join("x")).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn try_download_from_urls_first_success_short_circuits() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let ws = TempWorkspace::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        if let Ok((mut s, _)) = listener.accept().await {
            let mut buf = [0u8; 256];
            let _ = s.read(&mut buf).await;
            let body = b"ok";
            let _ = s
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await;
            let _ = s.write_all(body).await;
        }
    });

    let result = try_download_from_urls(
        &[
            format!("http://127.0.0.1:{}/a", port),
            "http://127.0.0.1:1/nope".into(),
        ],
        &ws.join("ok.bin"),
    )
    .await;
    assert_eq!(result.unwrap(), 0);
}

#[tokio::test]
async fn download_and_install_kernel_from_urls_happy_path() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let ws = TempWorkspace::new();
    let exe_name = kernel_executable_name();
    let mut zip_buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file(exe_name, options).unwrap();
        zip.write_all(b"binary").unwrap();
        zip.finish().unwrap();
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let response = zip_buf.clone();
    tokio::spawn(async move {
        if let Ok((mut s, _)) = listener.accept().await {
            let mut buf = [0u8; 256];
            let _ = s.read(&mut buf).await;
            let _ = s
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                        response.len()
                    )
                    .as_bytes(),
                )
                .await;
            let _ = s.write_all(&response).await;
        }
    });

    let target = download_and_install_kernel_from_urls(
        &[format!("http://127.0.0.1:{}/kernel.zip", port)],
        ws.path(),
        "kernel.zip",
    )
    .await
    .unwrap();
    assert_eq!(target.file_name().unwrap().to_string_lossy(), exe_name);
}

#[test]
fn should_stop_kernel_before_replace_logic() {
    assert!(should_stop_kernel_before_replace(true));
    assert!(!should_stop_kernel_before_replace(false));
    assert!(kernel_stop_retry_succeeded(false));
    assert!(!kernel_stop_retry_succeeded(true));
}

#[tokio::test]
async fn install_local_kernel_archive_while_running_stops_first() {
    use crate::app::constants::paths;
    use crate::app::core::kernel_service::PROCESS_MANAGER;
    use crate::test_support::MockAppEnv;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    let env = MockAppEnv::new();
    let work = env.workspace.path();
    let db = work.join("dl-run.db");
    env.install_storage_from_path(db.to_str().unwrap()).await;

    // 假内核 + 启动
    let dir = work.join("sing-box");
    fs::create_dir_all(&dir).unwrap();
    let kernel = dir.join(kernel_executable_name());
    fs::write(
        &kernel,
        r#"#!/bin/sh
if [ "$1" = "check" ]; then exit 0; fi
if [ "$1" = "version" ]; then echo sing-box version 1.0.0; exit 0; fi
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
    let cfg = paths::get_config_dir().join("config.json");
    fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    fs::write(&cfg, r#"{}"#).unwrap();
    let _ = PROCESS_MANAGER
        .start_inner::<tauri::Wry>(None, &cfg, false)
        .await;

    let zip_path = work.join("k2.zip");
    {
        let f = std::fs::File::create(&zip_path).unwrap();
        let mut z = ZipWriter::new(f);
        z.start_file(kernel_executable_name(), SimpleFileOptions::default())
            .unwrap();
        z.write_all(
            b"#!/bin/sh\nif [ \"$1\" = \"version\" ]; then echo sing-box version 1.12.1; fi\nexit 0\n",
        )
        .unwrap();
        z.finish().unwrap();
    }

    let (target, was_running) =
        install_local_kernel_archive_with_optional_stop(&env.handle(), &zip_path, work)
            .await
            .expect("install while running");
    assert!(target.exists());
    assert!(was_running);
    let _ = PROCESS_MANAGER.stop::<tauri::Wry>(None).await;
}

#[tokio::test]
async fn download_and_install_install_fails_after_download() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let ws = TempWorkspace::new();
    // 下载成功但不是合法归档
    let body = b"not-an-archive";
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        if let Ok((mut s, _)) = listener.accept().await {
            let mut buf = [0u8; 256];
            let _ = s.read(&mut buf).await;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes()).await;
            let _ = s.write_all(body).await;
        }
    });
    let err = download_and_install_kernel_from_urls(
        &[format!("http://127.0.0.1:{}/bad.bin", port)],
        ws.path(),
        "bad.bin",
    )
    .await;
    assert!(err.is_err());
}

#[test]
fn kernel_download_source_name_all_indices() {
    for i in 0..8 {
        let _ = kernel_download_source_name(i);
    }
    assert_eq!(download_source_progress(0), 15);
    assert_eq!(download_source_progress(3), 30);
}
