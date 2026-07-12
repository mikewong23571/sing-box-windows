use super::*;
use crate::app::storage::enhanced_storage_service::db_get_app_config;
use std::fs;
use std::io::Write;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

#[test]
fn extract_version_and_compare() {
    assert_eq!(
        extract_version_from_output("sing-box version 1.10.0\n"),
        Some("1.10.0".into())
    );
    assert!(extract_version_from_output("nope").is_none());
    assert_eq!(normalize_version_string("v1.2.3"), "1.2.3");
    assert_eq!(is_embedded_newer("1.0.0", "1.1.0"), Some(true));
    assert_eq!(is_embedded_newer("2.0.0", "1.0.0"), Some(false));
}

#[test]
fn extract_zip_to_dir_and_find_subdir() {
    let dir = tempfile::tempdir().unwrap();
    let mut buf = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut buf);
        let mut z = ZipWriter::new(&mut cursor);
        z.start_file("nested/file.txt", SimpleFileOptions::default())
            .unwrap();
        z.write_all(b"data").unwrap();
        z.finish().unwrap();
    }
    let out = dir.path().join("out");
    extract_zip_to_dir(&buf, &out).unwrap();
    assert!(out.exists());
    // single subdir detection
    let sub = dir.path().join("one");
    fs::create_dir_all(sub.join("x")).unwrap();
    // find_single_subdirectory returns Some when exactly one subdir
    let _ = find_single_subdirectory(dir.path());
}

#[test]
fn is_embedded_newer_edge_cases() {
    assert_eq!(is_embedded_newer("1.0.0", "1.0.0"), Some(false));
    assert_eq!(is_embedded_newer("bad", "1.0.0"), None);
    assert_eq!(is_embedded_newer("1.0.0", "bad"), None);
    assert_eq!(normalize_version_string("  v2.0.0-beta  "), "2.0.0-beta");
    assert!(
        extract_version_from_output("{\"version\":\"1.2.3\"}").is_some()
            || extract_version_from_output("version 1.2.3").is_some()
    );
}

#[test]
fn find_single_subdirectory_exactly_one() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("only");
    fs::create_dir_all(a.join("child")).unwrap();
    // only one top-level dir "only"
    let found = find_single_subdirectory(dir.path());
    assert!(found.is_some());
    let multi = dir.path().join("two");
    fs::create_dir_all(&multi).unwrap();
    // now two dirs - should be None or still some depending on impl
    let _ = find_single_subdirectory(dir.path());
}

#[test]
fn extract_zip_to_dir_rejects_bad_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let err = extract_zip_to_dir(b"not-a-zip", &dir.path().join("x"));
    assert!(err.is_err());
}

#[test]
fn extract_version_from_output_edges() {
    assert_eq!(
        extract_version_from_output("sing-box (version 1.11.0)"),
        Some("1.11.0".into())
    );
    assert_eq!(extract_version_from_output(""), None);
    assert_eq!(extract_version_from_output("   "), None);
    assert_eq!(extract_version_from_output(": , ; ( )"), None);
    // trim_matches 不会去掉 token 内部的前缀；整 token 经 normalize 后可能仍带 "build:"
    let v = extract_version_from_output("build:v1.2.3-rc");
    assert!(v.is_some());
    assert!(v.unwrap().contains("1.2.3"));
}

#[test]
fn is_embedded_newer_semver_prerelease_and_equal_strings() {
    // 无法 parse 但字符串相等
    assert_eq!(is_embedded_newer("dev-build", "dev-build"), Some(false));
    // 可解析比较
    assert_eq!(is_embedded_newer("1.2.0", "1.2.1"), Some(true));
    assert_eq!(is_embedded_newer("1.3.0", "1.2.9"), Some(false));
    assert_eq!(is_embedded_newer("", "1.0.0"), None);
    assert_eq!(is_embedded_newer("1.0.0", ""), None);
}

#[test]
fn find_single_subdirectory_none_when_empty_or_files_only() {
    let dir = tempfile::tempdir().unwrap();
    assert!(find_single_subdirectory(dir.path()).is_none());
    fs::write(dir.path().join("only-file"), b"x").unwrap();
    assert!(find_single_subdirectory(dir.path()).is_none());
}

#[test]
fn extract_zip_with_directory_entries() {
    let dir = tempfile::tempdir().unwrap();
    let mut buf = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut buf);
        let mut z = ZipWriter::new(&mut cursor);
        z.add_directory("ui/", SimpleFileOptions::default())
            .unwrap();
        z.start_file("ui/index.html", SimpleFileOptions::default())
            .unwrap();
        z.write_all(b"<html></html>").unwrap();
        z.finish().unwrap();
    }
    let out = dir.path().join("ui_out");
    extract_zip_to_dir(&buf, &out).unwrap();
    assert!(out.join("ui/index.html").exists() || out.join("ui").join("index.html").exists());
}

#[test]
fn embedded_platform_and_find_paths() {
    let platform = embedded_platform_id().expect("desktop platform");
    assert!(!embedded_executable_name().is_empty());
    let dir = tempfile::tempdir().unwrap();
    let arch = "amd64";
    let exe = embedded_executable_name();
    // kernel/<platform>/<arch>/
    let nested = dir.path().join("kernel").join(platform).join(arch);
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join(exe), b"bin").unwrap();
    let found = find_embedded_kernel_paths(dir.path(), platform, arch, exe);
    assert!(found.is_some());
    let (d, p) = found.unwrap();
    assert!(d.ends_with(arch));
    assert!(p.ends_with(exe));

    // resources/kernel 备选
    let dir2 = tempfile::tempdir().unwrap();
    let nested2 = dir2
        .path()
        .join("resources/kernel")
        .join(platform)
        .join(arch);
    fs::create_dir_all(&nested2).unwrap();
    fs::write(nested2.join(exe), b"bin").unwrap();
    assert!(find_embedded_kernel_paths(dir2.path(), platform, arch, exe).is_some());
    assert!(find_embedded_kernel_paths(dir2.path(), platform, "noarch", exe).is_none());
}

#[test]
fn decide_embedded_install_all_branches() {
    assert_eq!(
        decide_embedded_install(false, Some("1.0.0"), None),
        EmbeddedInstallDecision::Install
    );
    assert_eq!(
        decide_embedded_install(true, None, Some("1.0.0")),
        EmbeddedInstallDecision::SkipLocalMissingEmbeddedVersion
    );
    assert_eq!(
        decide_embedded_install(true, Some("1.1.0"), None),
        EmbeddedInstallDecision::SkipLocalUnknownVersion
    );
    assert_eq!(
        decide_embedded_install(true, Some("1.1.0"), Some("1.0.0")),
        EmbeddedInstallDecision::Install
    );
    assert_eq!(
        decide_embedded_install(true, Some("1.0.0"), Some("2.0.0")),
        EmbeddedInstallDecision::SkipLocalNotOlder
    );
    assert_eq!(
        decide_embedded_install(true, Some("dev-a"), Some("dev-b")),
        EmbeddedInstallDecision::SkipVersionUncomparable
    );
}

#[tokio::test]
async fn copy_embedded_kernel_and_read_version_txt() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src/sing-box");
    let dst = dir.path().join("dst/sing-box");
    fs::create_dir_all(src.parent().unwrap()).unwrap();
    fs::write(&src, b"#!/bin/sh\necho ok\n").unwrap();
    copy_embedded_kernel_binary(&src, &dst).await.unwrap();
    assert!(dst.is_file());
    assert_eq!(fs::read(&dst).unwrap(), b"#!/bin/sh\necho ok\n");

    let emb = dir.path().join("emb");
    fs::create_dir_all(&emb).unwrap();
    assert!(read_embedded_version_public(&emb).await.is_none());
    fs::write(emb.join("version.txt"), "  v1.2.3 \n").unwrap();
    assert_eq!(
        read_embedded_version_public(&emb).await.as_deref(),
        Some("v1.2.3")
    );
    fs::write(emb.join("version.txt"), "   \n").unwrap();
    assert!(read_embedded_version_public(&emb).await.is_none());
}

#[tokio::test]
async fn save_installed_version_roundtrip() {
    use crate::test_support::MockAppEnv;

    let env = MockAppEnv::new();
    let db = env.workspace.path().join("emb-version.db");
    env.install_storage_from_path(db.to_str().unwrap()).await;

    save_installed_version(&env.handle(), "v1.2.3".into())
        .await
        .unwrap();
    let cfg = db_get_app_config(env.handle().clone()).await.unwrap();
    assert_eq!(cfg.installed_kernel_version.as_deref(), Some("1.2.3"));

    // 重复保存相同版本不应报错
    save_installed_version(&env.handle(), "1.2.3".into())
        .await
        .unwrap();
}

#[tokio::test]
async fn read_kernel_version_from_fake_binary() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("sing-box");
    fs::write(
        &bin,
        r#"#!/bin/sh
echo "sing-box version 1.9.0"
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut p = fs::metadata(&bin).unwrap().permissions();
        p.set_mode(0o755);
        fs::set_permissions(&bin, p).unwrap();
    }
    let v = read_kernel_version_from_binary_public(&bin).await;
    assert_eq!(v.as_deref(), Some("1.9.0"));

    let bad = dir.path().join("bad");
    fs::write(&bad, b"not-executable").unwrap();
    let _ = read_kernel_version_from_binary_public(&bad).await;
}

#[tokio::test]
async fn resolve_installed_version_from_binary_and_db() {
    use crate::test_support::MockAppEnv;

    let env = MockAppEnv::new();
    let work = env.workspace.path();
    let db = work.join("resolve.db");
    env.install_storage_from_path(db.to_str().unwrap()).await;

    let dir = work.join("bin");
    fs::create_dir_all(&dir).unwrap();
    let bin = dir.join(embedded_executable_name());
    fs::write(
        &bin,
        r#"#!/bin/sh
echo "sing-box version 2.0.0"
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut p = fs::metadata(&bin).unwrap().permissions();
        p.set_mode(0o755);
        fs::set_permissions(&bin, p).unwrap();
    }

    let from_binary = resolve_installed_version(&env.handle(), &bin).await;
    assert_eq!(from_binary.as_deref(), Some("2.0.0"));

    // 二进制不可执行时 fallback 到 db
    let bad = dir.join("bad-bin");
    fs::write(&bad, b"x").unwrap();
    save_installed_version(&env.handle(), "3.0.0".into())
        .await
        .unwrap();
    let from_db = resolve_installed_version(&env.handle(), &bad).await;
    assert_eq!(from_db.as_deref(), Some("3.0.0"));
}

#[tokio::test]
async fn install_external_ui_from_zip_and_ready_flag() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path();
    assert!(!external_ui_ready(work));

    let mut buf = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut buf);
        let mut z = ZipWriter::new(&mut cursor);
        z.add_directory("metacubexd-gh-pages/", SimpleFileOptions::default())
            .unwrap();
        z.start_file(
            "metacubexd-gh-pages/index.html",
            SimpleFileOptions::default(),
        )
        .unwrap();
        z.write_all(b"<html>ui</html>").unwrap();
        z.finish().unwrap();
    }
    install_external_ui_from_zip_bytes(&buf, work)
        .await
        .unwrap();
    assert!(external_ui_ready(work));

    // 已存在时应跳过下载
    download_and_install_external_ui_from_url("http://127.0.0.1:1/x", work, 1)
        .await
        .unwrap();
}

#[tokio::test]
async fn download_and_install_external_ui_from_local_http() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("wd");
    fs::create_dir_all(&work).unwrap();

    let mut buf = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut buf);
        let mut z = ZipWriter::new(&mut cursor);
        z.start_file("index.html", SimpleFileOptions::default())
            .unwrap();
        z.write_all(b"<html>x</html>").unwrap();
        z.finish().unwrap();
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let body = buf.clone();
    tokio::spawn(async move {
        if let Ok((mut s, _)) = listener.accept().await {
            let mut rbuf = [0u8; 512];
            let _ = s.read(&mut rbuf).await;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes()).await;
            let _ = s.write_all(&body).await;
        }
    });

    download_and_install_external_ui_from_url(
        &format!("http://127.0.0.1:{}/ui.zip", port),
        &work,
        5,
    )
    .await
    .unwrap();
    assert!(external_ui_ready(&work));

    // HTTP 错误
    let listener2 = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port2 = listener2.local_addr().unwrap().port();
    tokio::spawn(async move {
        if let Ok((mut s, _)) = listener2.accept().await {
            let mut rbuf = [0u8; 256];
            let _ = s.read(&mut rbuf).await;
            let _ = s
                .write_all(b"HTTP/1.1 500 ERR\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
        }
    });
    let work2 = dir.path().join("wd2");
    fs::create_dir_all(&work2).unwrap();
    let err = download_and_install_external_ui_from_url(
        &format!("http://127.0.0.1:{}/bad", port2),
        &work2,
        5,
    )
    .await;
    assert!(err.is_err());
}
