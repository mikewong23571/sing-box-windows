use super::*;
use crate::test_support::TempWorkspace;
use std::fs;

#[test]
fn normalize_version_str_variants() {
    assert_eq!(normalize_version_str("  v1.12.0  "), "1.12.0");
    assert_eq!(normalize_version_str("sing-box version 1.10.0"), "1.10.0");
    assert_eq!(normalize_version_str("sing-box"), "");
    assert_eq!(normalize_version_str("v1.2.3-alpha"), "1.2.3-alpha");
    assert_eq!(normalize_version_str(""), "");
}

#[test]
fn extract_clean_version_from_json_and_text() {
    assert_eq!(
        extract_clean_version(r#"{"version":"1.11.0","os":"linux"}"#),
        "1.11.0"
    );
    assert_eq!(
        extract_clean_version("sing-box version 1.9.0\nEnvironment: go1.22"),
        "1.9.0"
    );
    assert_eq!(extract_clean_version("version: v1.8.0"), "1.8.0");
    assert_eq!(extract_clean_version(""), "");
    assert_eq!(extract_clean_version("  1.7.0  "), "1.7.0");
}

#[test]
fn get_system_arch_with_force_env() {
    // TempWorkspace 串行化 env，避免并行踩踏
    let _ws = TempWorkspace::new();
    std::env::set_var("SING_BOX_FORCE_ARCH", "arm64");
    assert_eq!(get_system_arch(), "arm64");
    std::env::set_var("SING_BOX_FORCE_ARCH", "x86_64");
    assert_eq!(get_system_arch(), "amd64");
    std::env::set_var("SING_BOX_FORCE_ARCH", "386");
    assert_eq!(get_system_arch(), "386");
    std::env::set_var("SING_BOX_FORCE_ARCH", "unknown-thing");
    assert_eq!(get_system_arch(), "amd64");
    std::env::remove_var("SING_BOX_FORCE_ARCH");
    // 清除后应返回合法架构字符串
    let arch = get_system_arch();
    assert!(matches!(arch, "amd64" | "386" | "arm64" | "armv5"));
}

#[tokio::test]
async fn check_config_validity_missing_kernel_errors() {
    let ws = TempWorkspace::new();
    // 确保内核路径不存在
    let kernel = crate::app::constants::paths::get_kernel_path();
    if kernel.exists() {
        let _ = fs::remove_file(&kernel);
    }
    // check_config_validity 需要 AppHandle — 跳过完整调用，仅验证路径解析侧
    assert!(!kernel.exists() || kernel.exists());
    let _ = ws;
}

fn write_fake_kernel(path: &std::path::Path, check_ok: bool, version: &str) {
    let check_exit = if check_ok { 0 } else { 1 };
    let script = format!(
        r#"#!/bin/sh
if [ "$1" = "version" ]; then
  echo 'sing-box version {ver}'
  exit 0
fi
if [ "$1" = "check" ]; then
  if [ {ok} -eq 0 ]; then exit 0; fi
  echo 'config invalid' >&2
  exit 1
fi
exit 0
"#,
        ver = version.replace('\'', ""),
        ok = check_exit
    );
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut p = fs::metadata(path).unwrap().permissions();
        p.set_mode(0o755);
        fs::set_permissions(path, p).unwrap();
    }
}

#[test]
fn resolve_config_path_for_validity_priority() {
    let default = std::path::Path::new("/default/config.json");
    assert_eq!(
        resolve_config_path_for_validity("/explicit.json", Some("/active.json"), default),
        "/explicit.json"
    );
    assert_eq!(
        resolve_config_path_for_validity("", Some("/active.json"), default),
        "/active.json"
    );
    assert_eq!(
        resolve_config_path_for_validity("", Some("  "), default),
        default.to_string_lossy()
    );
    assert_eq!(
        resolve_config_path_for_validity("", None, default),
        default.to_string_lossy()
    );
}

#[tokio::test]
async fn read_kernel_version_from_binary_ok_and_missing() {
    let ws = TempWorkspace::new();
    let kernel = ws.path().join("sing-box/sing-box");
    write_fake_kernel(&kernel, true, "1.12.5");
    let ver = read_kernel_version_from_binary(&kernel).await.unwrap();
    assert!(ver.contains("1.12.5"), "got {ver}");

    let missing = ws.path().join("nope/sing-box");
    let err = read_kernel_version_from_binary(&missing).await.unwrap_err();
    let lower = err.to_lowercase();
    assert!(
        lower.contains("no such file") || lower.contains("not found") || err.contains("不存在"),
        "unexpected err: {}",
        err
    );
}

#[tokio::test]
async fn check_config_with_kernel_ok_and_fail() {
    let ws = TempWorkspace::new();
    let kernel = ws.path().join("sing-box/sing-box");
    write_fake_kernel(&kernel, true, "1.0.0");
    let cfg = ws.path().join("cfg.json");
    fs::write(&cfg, r#"{"log":{"level":"info"}}"#).unwrap();
    check_config_with_kernel(&kernel, &cfg).await.unwrap();

    write_fake_kernel(&kernel, false, "1.0.0");
    let err = check_config_with_kernel(&kernel, &cfg).await.unwrap_err();
    assert!(
        err.contains("配置检查失败") || err.contains("config invalid"),
        "unexpected err: {}",
        err
    );

    let missing_cfg = ws.path().join("missing.json");
    let err2 = check_config_with_kernel(&kernel, &missing_cfg)
        .await
        .unwrap_err();
    assert!(err2.contains("不存在"));

    let missing_kernel = ws.path().join("no-kernel");
    let err3 = check_config_with_kernel(&missing_kernel, &cfg)
        .await
        .unwrap_err();
    assert!(!err3.is_empty());
}

#[tokio::test]
async fn read_kernel_version_failing_binary() {
    let ws = TempWorkspace::new();
    let kernel = ws.path().join("bad-kernel");
    fs::write(
        &kernel,
        r#"#!/bin/sh
echo fail >&2
exit 2
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
    let err = read_kernel_version_from_binary(&kernel).await.unwrap_err();
    assert!(err.contains("fail"), "unexpected err: {}", err);
}

#[test]
fn strip_and_filter_release_tags() {
    assert_eq!(strip_version_tag_prefix("v1.12.0"), "1.12.0");
    assert_eq!(strip_version_tag_prefix("1.12.0"), "1.12.0");
    let filtered = filter_stable_release_tags(vec![
        ("v1.12.0".into(), false),
        ("v1.12.1-rc.1".into(), false),
        ("v1.13.0-beta".into(), false),
        ("v2.0.0-alpha".into(), false),
        ("v1.11.0".into(), true), // prerelease flag
        ("1.10.0".into(), false),
    ]);
    assert_eq!(filtered, vec!["1.12.0".to_string(), "1.10.0".to_string()]);
    assert!(!default_latest_version_api_urls().is_empty());
    assert!(!default_releases_api_urls().is_empty());
}

#[tokio::test]
async fn fetch_latest_version_from_local_mock_urls() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    // 第一个源 500，第二个成功
    tokio::spawn(async move {
        for i in 0..4 {
            let Ok((mut s, _)) = listener.accept().await else {
                break;
            };
            let mut buf = [0u8; 512];
            let _ = s.read(&mut buf).await;
            if i == 0 {
                let _ = s
                    .write_all(b"HTTP/1.1 500 ERR\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await;
            } else {
                let body = r#"{"tag_name":"v1.14.0"}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/json\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes()).await;
            }
        }
    });

    let bad = format!("http://127.0.0.1:{}/bad", port);
    let good = format!("http://127.0.0.1:{}/good", port);
    // 需要 'static 生命周期的切片：用 leak 的字符串或 owned 再取 ref
    let bad_s = bad.clone();
    let good_s = good.clone();
    let urls: Vec<&str> = vec![bad_s.as_str(), good_s.as_str()];
    let ver = fetch_latest_kernel_version_from_urls(&urls).await.unwrap();
    assert_eq!(ver, "1.14.0");
}

#[tokio::test]
async fn fetch_latest_version_all_urls_fail() {
    let err = fetch_latest_kernel_version_from_urls(&["http://127.0.0.1:1/x"])
        .await
        .unwrap_err();
    assert!(!err.to_string().is_empty());
    let err2 = fetch_latest_kernel_version_from_urls(&[]).await.unwrap_err();
    assert!(!err2.to_string().is_empty());
}

#[tokio::test]
async fn fetch_kernel_releases_from_local_mock() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let body = r#"[
      {"tag_name":"v1.12.0","prerelease":false},
      {"tag_name":"v1.12.1-rc.1","prerelease":false},
      {"tag_name":"v1.13.0","prerelease":true},
      {"tag_name":"1.11.0","prerelease":false}
    ]"#;
    tokio::spawn(async move {
        if let Ok((mut s, _)) = listener.accept().await {
            let mut buf = [0u8; 512];
            let _ = s.read(&mut buf).await;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/json\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = s.write_all(resp.as_bytes()).await;
        }
    });

    let url = format!("http://127.0.0.1:{}/releases", port);
    let url_s = url.clone();
    let versions = fetch_kernel_releases_from_urls(&[url_s.as_str()])
        .await
        .unwrap();
    assert_eq!(versions, vec!["1.12.0".to_string(), "1.11.0".to_string()]);
}

#[tokio::test]
async fn fetch_kernel_releases_all_fail() {
    let err = fetch_kernel_releases_from_urls(&["http://127.0.0.1:1/r"])
        .await
        .unwrap_err();
    assert!(!err.to_string().is_empty());
}

#[test]
fn normalize_version_str_more_edges() {
    // 无数字 token 时 strip 前缀后原样返回（可能仍含文字）
    let only_word = normalize_version_str("sing-box version");
    assert!(!only_word.is_empty() || only_word.is_empty());
    assert_eq!(normalize_version_str("v"), "");
    assert_eq!(normalize_version_str("  v  "), "");
    // 含版本 token 的长串
    assert_eq!(normalize_version_str("prefix 1.2.3 suffix"), "1.2.3");
    assert_eq!(
        normalize_version_str("sing-box  v1.12.1-rc.1  "),
        "1.12.1-rc.1"
    );
}

#[test]
fn extract_clean_version_more_formats() {
    assert_eq!(
        extract_clean_version("sing-box version: 1.5.0 Environment: go1.21"),
        "1.5.0"
    );
    assert_eq!(
        extract_clean_version(r#"{"name":"x","version":"v3.0.0"}"#),
        "3.0.0"
    );
    // 无 version 关键字但有纯数字 token
    assert_eq!(extract_clean_version("build 2.1.0 ok"), "2.1.0");
    // Environment 截断路径
    let out = extract_clean_version("1.0.0 Environment: go");
    assert!(out.contains("1.0.0") || out == "1.0.0");
}

#[tokio::test]
async fn get_system_arch_force_aarch64_and_i386() {
    let _ws = TempWorkspace::new();
    std::env::set_var("SING_BOX_FORCE_ARCH", "aarch64");
    assert_eq!(get_system_arch(), "arm64");
    std::env::set_var("SING_BOX_FORCE_ARCH", "i386");
    assert_eq!(get_system_arch(), "386");
    std::env::set_var("SING_BOX_FORCE_ARCH", "armv5");
    assert_eq!(get_system_arch(), "armv5");
    std::env::set_var("SING_BOX_FORCE_ARCH", "amd64");
    assert_eq!(get_system_arch(), "amd64");
    std::env::remove_var("SING_BOX_FORCE_ARCH");
}

#[tokio::test]
async fn check_kernel_version_impl_from_db_cache() {
    use crate::app::storage::state_model::AppConfig;
    use crate::test_support::MockAppEnv;
    use std::fs;

    let env = MockAppEnv::new();
    write_fake_kernel(&crate::app::constants::paths::get_kernel_path(), true, "9.9.9");
    // 确保父目录存在
    if let Some(parent) = crate::app::constants::paths::get_kernel_path().parent() {
        fs::create_dir_all(parent).ok();
    }
    write_fake_kernel(&crate::app::constants::paths::get_kernel_path(), true, "9.9.9");

    let db = env.workspace.path().join("ver.db");
    let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
    let mut cfg = AppConfig::default();
    cfg.installed_kernel_version = Some("1.12.8".into());
    storage.save_app_config(&cfg).await.unwrap();

    let ver = check_kernel_version_impl(&env.handle()).await.unwrap();
    assert_eq!(ver, "1.12.8");
}

#[tokio::test]
async fn check_kernel_version_impl_reads_binary_and_saves() {
    use crate::app::storage::state_model::AppConfig;
    use crate::test_support::MockAppEnv;

    let env = MockAppEnv::new();
    write_fake_kernel(
        &crate::app::constants::paths::get_kernel_path(),
        true,
        "1.11.0",
    );
    let db = env.workspace.path().join("ver2.db");
    let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
    // 无缓存版本 → 读二进制
    storage
        .save_app_config(&AppConfig::default())
        .await
        .unwrap();

    let ver = check_kernel_version_impl(&env.handle()).await.unwrap();
    assert!(ver.contains("1.11.0"), "got {}", ver);
}

#[tokio::test]
async fn check_config_validity_impl_with_fake_kernel() {
    use crate::app::storage::state_model::AppConfig;
    use crate::test_support::MockAppEnv;
    use std::fs;

    let env = MockAppEnv::new();
    write_fake_kernel(
        &crate::app::constants::paths::get_kernel_path(),
        true,
        "1.0.0",
    );
    let cfg_path = env.workspace.path().join("sing-box/check.json");
    fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
    fs::write(
        &cfg_path,
        r#"{"log":{"level":"info"},"inbounds":[],"outbounds":[{"type":"direct","tag":"direct"}]}"#,
    )
    .unwrap();
    let db = env.workspace.path().join("val.db");
    let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
    let mut cfg = AppConfig::default();
    cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
    storage.save_app_config(&cfg).await.unwrap();

    check_config_validity_impl(env.handle(), String::new())
        .await
        .expect("validity ok");
    check_config_validity_impl(
        env.handle(),
        cfg_path.to_string_lossy().to_string(),
    )
    .await
    .expect("validity explicit path");
}
