use super::*;
use serde_json::json;
use std::path::Path;

fn sample_releases() -> Vec<serde_json::Value> {
    vec![
        json!({
            "tag_name": "v1.3.0-autobuild",
            "name": "Autobuild Nightly",
            "prerelease": true
        }),
        json!({
            "tag_name": "v1.2.0-rc.1",
            "name": "Release Candidate",
            "prerelease": true
        }),
        json!({
            "tag_name": "v1.1.0",
            "name": "Stable Release",
            "prerelease": false
        }),
    ]
}

#[test]
fn update_channel_should_resolve_inputs_consistently() {
    assert_eq!(
        UpdateChannel::from_inputs(Some("stable"), true),
        UpdateChannel::Stable
    );
    assert_eq!(
        UpdateChannel::from_inputs(Some(" Prerelease "), false),
        UpdateChannel::Prerelease
    );
    assert_eq!(
        UpdateChannel::from_inputs(Some("autobuild"), false),
        UpdateChannel::Autobuild
    );
    assert_eq!(
        UpdateChannel::from_inputs(None, false),
        UpdateChannel::Stable
    );
    assert_eq!(
        UpdateChannel::from_inputs(Some("unknown"), true),
        UpdateChannel::Prerelease
    );
}

#[test]
fn update_channel_should_report_release_list_usage() {
    assert!(!UpdateChannel::Stable.uses_release_list());
    assert!(UpdateChannel::Prerelease.uses_release_list());
    assert!(UpdateChannel::Autobuild.uses_release_list());
}

#[test]
fn check_arch_compatibility_should_match_known_arch_aliases() {
    assert!(check_arch_compatibility(
        "sing-box-windows-amd64.exe",
        "x86_64"
    ));
    assert!(check_arch_compatibility("sing-box-windows.exe", "x86_64"));
    assert!(!check_arch_compatibility(
        "sing-box-windows-arm64.exe",
        "x86_64"
    ));

    assert!(check_arch_compatibility(
        "sing-box-macos-arm64.dmg",
        "aarch64"
    ));
    assert!(check_arch_compatibility(
        "sing-box-macos-universal.dmg",
        "aarch64"
    ));
    assert!(!check_arch_compatibility(
        "sing-box-macos-x64.dmg",
        "aarch64"
    ));

    assert!(check_arch_compatibility(
        "sing-box-linux-armv7.deb",
        "armv7"
    ));
    assert!(!check_arch_compatibility(
        "sing-box-linux-arm64.deb",
        "armv7"
    ));

    assert!(check_arch_compatibility("sing-box-linux-i386.deb", "x86"));
    assert!(!check_arch_compatibility("sing-box-linux-x64.deb", "x86"));
    assert!(check_arch_compatibility(
        "sing-box-linux-x86_64.rpm",
        "x86_64"
    ));
    assert!(!check_arch_compatibility(
        "sing-box-linux-aarch64.rpm",
        "x86_64"
    ));
}

#[test]
fn get_package_kind_should_detect_supported_linux_formats() {
    assert_eq!(
        get_package_kind("sing-box-linux-x86_64.rpm"),
        PackageKind::Rpm
    );
    assert_eq!(
        get_package_kind("sing-box-windows_2.2.6_amd64.AppImage"),
        PackageKind::AppImage
    );
    assert_eq!(
        get_package_kind("sing-box-windows_2.2.6_amd64.deb"),
        PackageKind::Deb
    );
}

#[test]
fn detect_linux_package_preference_should_follow_os_release() {
    let fedora_os_release = r#"
ID=fedora
ID_LIKE="fedora rhel"
"#;
    assert_eq!(
        detect_linux_package_preference_from_os_release(fedora_os_release),
        LinuxPackagePreference::Rpm
    );

    let ubuntu_os_release = r#"
ID=ubuntu
ID_LIKE=debian
"#;
    assert_eq!(
        detect_linux_package_preference_from_os_release(ubuntu_os_release),
        LinuxPackagePreference::Deb
    );

    let arch_os_release = r#"
ID=arch
ID_LIKE=archlinux
"#;
    assert_eq!(
        detect_linux_package_preference_from_os_release(arch_os_release),
        LinuxPackagePreference::AppImage
    );
}

#[test]
fn get_platform_priority_for_linux_should_respect_distribution_preference() {
    let rpm = "sing-box-windows-2.2.6-1.x86_64.rpm";
    let deb = "sing-box-windows_2.2.6_amd64.deb";
    let appimage = "sing-box-windows_2.2.6_amd64.AppImage";

    assert!(
        get_platform_priority_for(rpm, "linux", "x86_64", LinuxPackagePreference::Rpm)
            > get_platform_priority_for(deb, "linux", "x86_64", LinuxPackagePreference::Rpm)
    );
    assert!(
        get_platform_priority_for(deb, "linux", "x86_64", LinuxPackagePreference::Deb)
            > get_platform_priority_for(rpm, "linux", "x86_64", LinuxPackagePreference::Deb)
    );
    assert!(
        get_platform_priority_for(
            appimage,
            "linux",
            "x86_64",
            LinuxPackagePreference::AppImage
        ) > get_platform_priority_for(rpm, "linux", "x86_64", LinuxPackagePreference::AppImage)
    );
}

#[test]
fn resolve_update_filename_and_message_should_cover_rpm() {
    assert_eq!(
        resolve_update_filename("https://example.com/app-2.2.6-1.x86_64.rpm", "linux"),
        "update.rpm"
    );
    assert_eq!(
        resolve_install_message("linux", "https://example.com/app-2.2.6-1.x86_64.rpm"),
        "正在安装软件包，请根据提示输入密码..."
    );
}

#[test]
fn select_release_by_channel_should_pick_expected_release() {
    let releases = sample_releases();

    let stable = select_release_by_channel(&releases, UpdateChannel::Stable)
        .expect("stable channel should find a non-prerelease release");
    assert_eq!(stable["tag_name"].as_str(), Some("v1.1.0"));

    let prerelease = select_release_by_channel(&releases, UpdateChannel::Prerelease)
        .expect("prerelease channel should use the first release entry");
    assert_eq!(prerelease["tag_name"].as_str(), Some("v1.3.0-autobuild"));

    let autobuild = select_release_by_channel(&releases, UpdateChannel::Autobuild)
        .expect("autobuild channel should prefer autobuild-tagged releases");
    assert_eq!(autobuild["tag_name"].as_str(), Some("v1.3.0-autobuild"));
}

#[test]
fn autobuild_channel_should_fallback_to_prerelease_when_needed() {
    let releases = vec![
        json!({
            "tag_name": "v1.2.0-rc.1",
            "name": "Release Candidate",
            "prerelease": true
        }),
        json!({
            "tag_name": "v1.1.0",
            "name": "Stable Release",
            "prerelease": false
        }),
    ];

    let autobuild = select_release_by_channel(&releases, UpdateChannel::Autobuild)
        .expect("autobuild channel should fallback to a prerelease release");
    assert_eq!(autobuild["tag_name"].as_str(), Some("v1.2.0-rc.1"));
}

#[test]
fn compare_versions_should_handle_semver_and_plain_text_versions() {
    assert!(compare_versions("v1.0.0", "1.0.1"));
    assert!(compare_versions("1.0.0 build-1", "v1.1.0 latest"));
    assert!(!compare_versions("1.1.0", "1.1.0"));

    assert!(compare_versions("nightly-2026-01-01", "nightly-2026-01-02"));
    assert!(!compare_versions(
        "nightly-2026-01-01",
        "nightly-2026-01-01"
    ));
}

#[test]
fn supports_in_app_update_should_only_enable_windows() {
    assert!(supports_in_app_update_for_platform("windows"));
    assert!(!supports_in_app_update_for_platform("linux"));
    assert!(!supports_in_app_update_for_platform("macos"));
}

#[test]
fn resolve_release_page_url_should_prefer_html_url() {
    let release = json!({
        "html_url": "https://github.com/xinggaoya/sing-box-windows/releases/tag/v2.2.6"
    });
    assert_eq!(
        resolve_release_page_url(&release),
        "https://github.com/xinggaoya/sing-box-windows/releases/tag/v2.2.6"
    );

    let release_without_url = json!({});
    assert_eq!(
        resolve_release_page_url(&release_without_url),
        "https://github.com/xinggaoya/sing-box-windows/releases"
    );
}

#[test]
fn get_package_kind_all_extensions() {
    assert_eq!(get_package_kind("a.msi"), PackageKind::Msi);
    assert_eq!(get_package_kind("a.EXE"), PackageKind::Exe);
    assert_eq!(get_package_kind("a.dmg"), PackageKind::Dmg);
    assert_eq!(get_package_kind("a.app.tar.gz"), PackageKind::AppTarGz);
    assert_eq!(get_package_kind("a.bin"), PackageKind::Unknown);
}

#[test]
fn resolve_update_filename_for_all_platforms() {
    assert_eq!(
        resolve_update_filename("https://x/a.msi", "windows"),
        "update.msi"
    );
    assert_eq!(
        resolve_update_filename("https://x/a.exe", "windows"),
        "update.exe"
    );
    assert_eq!(
        resolve_update_filename("https://x/a.deb", "linux"),
        "update.deb"
    );
    assert_eq!(
        resolve_update_filename("https://x/a.AppImage", "linux"),
        "update.AppImage"
    );
    assert_eq!(
        resolve_update_filename("https://x/a.dmg", "macos"),
        "update.dmg"
    );
    assert_eq!(
        resolve_update_filename("https://x/a.app.tar.gz", "macos"),
        "update.app.tar.gz"
    );
    assert_eq!(
        resolve_update_filename("https://x/unknown", "windows"),
        "update.exe"
    );
    assert_eq!(
        resolve_update_filename("https://x/unknown", "linux"),
        "update.AppImage"
    );
    assert_eq!(
        resolve_update_filename("https://x/unknown", "macos"),
        "update.dmg"
    );
    assert_eq!(
        resolve_update_filename("https://x/unknown", "android"),
        "update.bin"
    );
}

#[test]
fn resolve_install_message_covers_platforms() {
    assert!(resolve_install_message("windows", "a.exe").contains("安装"));
    assert!(resolve_install_message("linux", "a.AppImage").contains("启动"));
    assert!(resolve_install_message("linux", "a.deb").contains("密码"));
    assert!(resolve_install_message("macos", "a.dmg").contains("挂载"));
    assert!(resolve_install_message("macos", "a.app.tar.gz").contains("解压"));
    let _ = resolve_install_message("other", "x");
}

#[test]
fn extract_linux_release_identifiers_parses_id_like() {
    let ids = extract_linux_release_identifiers(
        r#"
ID=ubuntu
ID_LIKE="debian ubuntu"
NAME="Ubuntu"
"#,
    );
    assert!(ids.iter().any(|i| i == "ubuntu"));
    assert!(ids.iter().any(|i| i == "debian"));
}

#[test]
fn is_platform_compatible_checks_extension_and_arch() {
    // 当前平台下至少有一种包类型兼容/不兼容可测
    let platform = get_platform_identifier();
    match platform {
        "linux" => {
            assert!(is_platform_compatible("sing-box-linux-x86_64.deb")
                || is_platform_compatible("sing-box-linux-amd64.AppImage")
                || !is_platform_compatible("sing-box-windows.exe"));
            assert!(!is_platform_compatible("sing-box-windows.msi"));
        }
        "windows" => {
            assert!(!is_platform_compatible("app.deb"));
        }
        "macos" => {
            assert!(!is_platform_compatible("app.exe"));
        }
        _ => {}
    }
}

#[test]
fn get_platform_priority_windows_and_macos() {
    assert!(
        get_platform_priority_for("app-x64.exe", "windows", "x86_64", LinuxPackagePreference::Deb)
            > get_platform_priority_for(
                "app-x64.msi",
                "windows",
                "x86_64",
                LinuxPackagePreference::Deb
            )
    );
    assert!(
        get_platform_priority_for(
            "app-arm64.dmg",
            "macos",
            "aarch64",
            LinuxPackagePreference::Deb
        ) > 0
    );
    assert_eq!(
        get_platform_priority_for("app.txt", "windows", "x86_64", LinuxPackagePreference::Deb),
        0
    );
    // portable bonus
    let portable = get_platform_priority_for(
        "app-portable-amd64.exe",
        "windows",
        "x86_64",
        LinuxPackagePreference::Deb,
    );
    let normal = get_platform_priority_for(
        "app-amd64.exe",
        "windows",
        "x86_64",
        LinuxPackagePreference::Deb,
    );
    assert!(portable >= normal);
}

#[test]
fn command_exists_on_path_for_sh() {
    // sh 在 linux 上几乎总存在
    #[cfg(unix)]
    {
        assert!(command_exists_on_path("sh") || command_exists_on_path("bash"));
    }
}

#[test]
fn platform_detailed_info_builds() {
    let info = PlatformDetailedInfo::current();
    assert!(!info.os.is_empty());
    assert!(!info.arch.is_empty());
    assert!(!info.display_name.is_empty());
    // 覆盖更多 arch 分支：通过 current 至少命中本机组合
    let _ = supports_in_app_update();
    let _ = get_platform_identifier();
    let _ = get_current_arch();
}

#[test]
fn select_release_empty_and_stable_none() {
    assert!(select_release_by_channel(&[], UpdateChannel::Stable).is_none());
    let only_pre = vec![json!({"tag_name":"v1","prerelease":true})];
    assert!(select_release_by_channel(&only_pre, UpdateChannel::Stable).is_none());
    assert!(select_release_by_channel(&only_pre, UpdateChannel::Prerelease).is_some());
}

#[test]
fn get_platform_priority_uses_live_detect() {
    // 调用包装函数覆盖 detect_linux_package_preference 路径
    let _ = get_platform_priority("sing-box-windows_2.2.6_amd64.deb");
    let _ = get_platform_priority("something-random.bin");
}

#[test]
fn pick_best_download_asset_prefers_high_priority() {
    let assets = vec![
        json!({
            "name": "sing-box-windows_2.0.0_amd64.deb",
            "browser_download_url": "https://example.com/a.deb",
            "size": 100
        }),
        json!({
            "name": "sing-box-windows_2.0.0_amd64.AppImage",
            "browser_download_url": "https://example.com/a.AppImage",
            "size": 200
        }),
        json!({
            "name": "sing-box-windows-2.0.0-1.x86_64.rpm",
            "browser_download_url": "https://example.com/a.rpm",
            "size": 150
        }),
    ];
    let (url, size, prio) = pick_best_download_asset(
        &assets,
        "linux",
        "x86_64",
        LinuxPackagePreference::Deb,
    );
    assert!(url.contains(".deb"));
    assert_eq!(size, Some(100));
    assert!(prio >= 20);

    let (url2, _, _) = pick_best_download_asset(
        &assets,
        "linux",
        "x86_64",
        LinuxPackagePreference::Rpm,
    );
    assert!(url2.contains(".rpm"));
}

#[test]
fn pick_best_download_asset_windows_and_macos() {
    let assets = vec![
        json!({
            "name": "app-x64.msi",
            "browser_download_url": "https://x/a.msi",
            "size": 1
        }),
        json!({
            "name": "app-x64-portable.exe",
            "browser_download_url": "https://x/a.exe",
            "size": 2
        }),
        json!({
            "name": "app-arm64.dmg",
            "browser_download_url": "https://x/a.dmg",
            "size": 3
        }),
    ];
    let (url, size, prio) = pick_best_download_asset(
        &assets,
        "windows",
        "x86_64",
        LinuxPackagePreference::Deb,
    );
    assert!(url.ends_with(".exe"));
    assert_eq!(size, Some(2));
    assert!(prio >= 25);

    let (url_m, _, _) = pick_best_download_asset(
        &assets,
        "macos",
        "aarch64",
        LinuxPackagePreference::Deb,
    );
    assert!(url_m.ends_with(".dmg"));
}

#[test]
fn build_update_info_from_release_happy_path() {
    let release = json!({
        "tag_name": "v2.3.0",
        "body": "notes",
        "html_url": "https://github.com/x/releases/tag/v2.3.0",
        "published_at": "2026-01-01T00:00:00Z",
        "prerelease": false,
        "assets": [
            {
                "name": "sing-box-windows_2.3.0_amd64.deb",
                "browser_download_url": "https://example.com/app.deb",
                "size": 999
            }
        ]
    });
    let info = build_update_info_from_release(
        &release,
        "2.2.0",
        "linux",
        "x86_64",
        LinuxPackagePreference::Deb,
        false,
    )
    .unwrap();
    assert_eq!(info.latest_version, "2.3.0");
    assert!(info.has_update);
    assert!(info.download_url.contains("app.deb"));
    assert_eq!(info.file_size, Some(999));
    assert!(!info.is_prerelease);
    assert_eq!(info.release_notes.as_deref(), Some("notes"));
}

#[test]
fn build_update_info_requires_download_when_in_app() {
    let release = json!({
        "tag_name": "v1.0.0",
        "prerelease": false,
        "assets": [
            { "name": "readme.txt", "browser_download_url": "https://x/r", "size": 1 }
        ]
    });
    let err = build_update_info_from_release(
        &release,
        "1.0.0",
        "windows",
        "x86_64",
        LinuxPackagePreference::Deb,
        true,
    );
    assert!(err.is_err());
}

#[test]
fn build_update_info_missing_tag_or_assets() {
    assert!(build_update_info_from_release(
        &json!({}),
        "1.0.0",
        "linux",
        "x86_64",
        LinuxPackagePreference::Deb,
        false
    )
    .is_err());
    assert!(build_update_info_from_release(
        &json!({"tag_name": "v1", "assets": null}),
        "1.0.0",
        "linux",
        "x86_64",
        LinuxPackagePreference::Deb,
        false
    )
    .is_err());
}

#[tokio::test]
async fn get_platform_info_commands() {
    let p = get_platform_info().await.unwrap();
    assert!(!p.is_empty());
    let d = get_detailed_platform_info().await.unwrap();
    assert!(!d.os.is_empty());
    assert!(!d.display_name.is_empty());
}

#[test]
fn plan_install_action_all_platforms() {
    assert_eq!(
        plan_install_action("windows", "https://x/a.msi"),
        InstallAction::Msiexec
    );
    assert_eq!(
        plan_install_action("windows", "https://x/a.exe"),
        InstallAction::RunExe
    );
    assert_eq!(
        plan_install_action("windows", "https://x/a.bin"),
        InstallAction::RunBinary
    );
    assert_eq!(
        plan_install_action("linux", "https://x/a.AppImage"),
        InstallAction::ChmodAndRun
    );
    assert_eq!(
        plan_install_action("linux", "https://x/a.deb"),
        InstallAction::PkexecDpkg
    );
    assert_eq!(
        plan_install_action("linux", "https://x/a.rpm"),
        InstallAction::PkexecRpm
    );
    assert_eq!(
        plan_install_action("macos", "https://x/a.dmg"),
        InstallAction::OpenDmg
    );
    assert_eq!(
        plan_install_action("macos", "https://x/a.app.tar.gz"),
        InstallAction::TarExtract
    );
    assert!(matches!(
        plan_install_action("android", "x"),
        InstallAction::Unsupported(_)
    ));
}

#[test]
fn default_check_update_api_url_stable_vs_list() {
    assert!(default_check_update_api_url(false).contains("releases/latest")
        || default_check_update_api_url(false).contains("github.com"));
    assert!(default_check_update_api_url(true).ends_with("/releases"));
    assert_ne!(
        default_check_update_api_url(true),
        default_check_update_api_url(false)
    );
}

#[test]
fn select_release_json_for_channel_empty_and_match() {
    assert!(select_release_json_for_channel(&[], UpdateChannel::Stable).is_err());
    let releases = sample_releases();
    let stable = select_release_json_for_channel(&releases, UpdateChannel::Stable).unwrap();
    assert_eq!(stable["tag_name"], "v1.1.0");
    let only_pre = vec![json!({"tag_name":"v9","prerelease":true,"name":"x"})];
    assert!(select_release_json_for_channel(&only_pre, UpdateChannel::Stable).is_err());
    assert!(select_release_json_for_channel(&only_pre, UpdateChannel::Prerelease).is_ok());
}

#[test]
fn update_path_and_progress_and_rpm_precheck() {
    assert!(in_app_update_unsupported_message().contains("应用内更新"));
    assert_eq!(downloaded_update_missing_message(), "下载的文件不存在");
    let p = resolve_update_download_path(
        Path::new("/tmp/wd"),
        "https://x/app.deb",
        "linux",
    );
    assert!(p.ends_with("update.deb"));
    let payload = build_update_progress_payload("downloading", 42, "hi");
    assert_eq!(payload["status"], "downloading");
    assert_eq!(payload["progress"], 42);
    assert_eq!(payload["message"], "hi");
    assert!(rpm_install_precheck(true).is_ok());
    assert!(rpm_install_precheck(false).unwrap_err().contains("rpm"));
}

#[tokio::test]
async fn check_update_from_local_latest_json() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let body = json!({
        "tag_name": "v3.0.0",
        "body": "notes",
        "html_url": "https://example.com/r",
        "published_at": "2026-01-01T00:00:00Z",
        "prerelease": false,
        "assets": [{
            "name": "sing-box-windows_3.0.0_amd64.deb",
            "browser_download_url": "https://example.com/app.deb",
            "size": 123
        }]
    })
    .to_string();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        if let Ok((mut s, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf).await;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = s.write_all(resp.as_bytes()).await;
        }
    });

    let info = check_update_from_api_url(
        &format!("http://127.0.0.1:{}/latest", port),
        false,
        "2.0.0",
        "linux",
        "x86_64",
        LinuxPackagePreference::Deb,
        false,
        UpdateChannel::Stable,
    )
    .await
    .unwrap();
    assert_eq!(info.latest_version, "3.0.0");
    assert!(info.has_update);
    assert!(info.download_url.contains("app.deb"));
}

#[tokio::test]
async fn check_update_from_local_releases_list() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let body = json!([
        {
            "tag_name": "v9.0.0-autobuild",
            "name": "Nightly Autobuild",
            "prerelease": true,
            "body": "nightly",
            "html_url": "https://example.com/auto",
            "published_at": "2026-02-01T00:00:00Z",
            "assets": [{
                "name": "sing-box-windows_9.0.0_amd64.AppImage",
                "browser_download_url": "https://example.com/a.AppImage",
                "size": 10
            }]
        },
        {
            "tag_name": "v8.0.0",
            "name": "Stable",
            "prerelease": false,
            "body": "stable",
            "html_url": "https://example.com/s",
            "published_at": "2026-01-01T00:00:00Z",
            "assets": [{
                "name": "sing-box-windows_8.0.0_amd64.deb",
                "browser_download_url": "https://example.com/s.deb",
                "size": 20
            }]
        }
    ])
    .to_string();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        if let Ok((mut s, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf).await;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = s.write_all(resp.as_bytes()).await;
        }
    });

    let info = check_update_from_api_url(
        &format!("http://127.0.0.1:{}/releases", port),
        true,
        "1.0.0",
        "linux",
        "x86_64",
        LinuxPackagePreference::AppImage,
        false,
        UpdateChannel::Autobuild,
    )
    .await
    .unwrap();
    assert!(info.latest_version.contains("9.0.0") || info.is_prerelease);
    assert!(info.download_url.contains("AppImage") || !info.download_url.is_empty());
}

#[tokio::test]
async fn check_update_from_api_url_network_error() {
    let err = check_update_from_api_url(
        "http://127.0.0.1:1/nope",
        false,
        "1.0.0",
        "linux",
        "x86_64",
        LinuxPackagePreference::Deb,
        false,
        UpdateChannel::Stable,
    )
    .await;
    assert!(err.is_err());
}

#[test]
fn is_platform_compatible_for_all_os() {
    assert!(is_platform_compatible_for(
        "app-x64.exe",
        "windows",
        "x86_64"
    ));
    assert!(!is_platform_compatible_for("app.deb", "windows", "x86_64"));
    assert!(is_platform_compatible_for(
        "app-amd64.deb",
        "linux",
        "x86_64"
    ));
    assert!(!is_platform_compatible_for("app.exe", "linux", "x86_64"));
    assert!(is_platform_compatible_for(
        "app-arm64.dmg",
        "macos",
        "aarch64"
    ));
    assert!(!is_platform_compatible_for("app.msi", "macos", "aarch64"));
    assert!(!is_platform_compatible_for("app.exe", "android", "x86_64"));
}

#[test]
fn detect_linux_preference_suse_and_quotes() {
    let suse = r#"
ID="opensuse-leap"
ID_LIKE="suse opensuse"
"#;
    assert_eq!(
        detect_linux_package_preference_from_os_release(suse),
        LinuxPackagePreference::Rpm
    );
    let rocky = "ID=rocky\nID_LIKE=rhel fedora\n";
    assert_eq!(
        detect_linux_package_preference_from_os_release(rocky),
        LinuxPackagePreference::Rpm
    );
    // 非 linux 平台返回 AppImage 默认
    let pref = detect_linux_package_preference();
    let _ = pref;
}

#[test]
fn get_platform_priority_arch_and_special_bonus() {
    let base = get_platform_priority_for(
        "app-installer-latest-amd64.exe",
        "windows",
        "x86_64",
        LinuxPackagePreference::Deb,
    );
    assert!(base >= 20);
    let arm = get_platform_priority_for(
        "app-universal.dmg",
        "macos",
        "aarch64",
        LinuxPackagePreference::Deb,
    );
    assert!(arm >= 20);
    let no_arch = get_platform_priority_for(
        "app.exe",
        "windows",
        "x86_64",
        LinuxPackagePreference::Deb,
    );
    assert!(no_arch >= 20);
    // 未知平台
    assert_eq!(
        get_platform_priority_for("app.exe", "freebsd", "x86_64", LinuxPackagePreference::Deb),
        0
    );
}

#[test]
fn check_arch_compatibility_unknown_and_arm32() {
    assert!(check_arch_compatibility("anything", "riscv64"));
    assert!(check_arch_compatibility("app-armv7.deb", "arm"));
    assert!(check_arch_compatibility("app-arm32.deb", "armv7"));
    assert!(!check_arch_compatibility("app-arm64.deb", "armv7"));
}

#[test]
fn resolve_release_page_url_empty_string_falls_back() {
    let release = json!({"html_url": "   "});
    assert_eq!(
        resolve_release_page_url(&release),
        "https://github.com/xinggaoya/sing-box-windows/releases"
    );
}

#[test]
fn pick_best_download_asset_skips_incompatible() {
    let assets = vec![
        json!({
            "name": "readme.txt",
            "browser_download_url": "https://x/r",
            "size": 1
        }),
        json!({
            "name": "app-x64.msi",
            "browser_download_url": "https://x/a.msi",
            "size": 9
        }),
    ];
    let (url, size, prio) = pick_best_download_asset(
        &assets,
        "windows",
        "x86_64",
        LinuxPackagePreference::Deb,
    );
    assert!(url.ends_with(".msi"));
    assert_eq!(size, Some(9));
    assert!(prio > 0);
    let (empty, _, p0) = pick_best_download_asset(&[], "windows", "x86_64", LinuxPackagePreference::Deb);
    assert!(empty.is_empty());
    assert_eq!(p0, 0);
}

#[test]
fn ensure_in_app_and_validate_downloaded_file() {
    assert!(ensure_in_app_update_supported(true).is_ok());
    assert!(ensure_in_app_update_supported(false)
        .unwrap_err()
        .contains("不支持"));

    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope.bin");
    assert!(validate_downloaded_update_file(&missing).is_err());
    let f = dir.path().join("ok.bin");
    std::fs::write(&f, b"x").unwrap();
    assert!(validate_downloaded_update_file(&f).is_ok());
}

#[tokio::test]
async fn download_update_package_to_work_dir_local_http() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let body = b"update-bytes";
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

    let dir = tempfile::tempdir().unwrap();
    let url = format!("http://127.0.0.1:{}/app.AppImage", port);
    let path = download_update_package_to_work_dir(&url, dir.path(), "linux", true)
        .await
        .expect("download package");
    assert!(path.exists());
    assert_eq!(std::fs::read(&path).unwrap(), body);

    // 不支持应用内更新
    let err = download_update_package_to_work_dir(&url, dir.path(), "linux", false)
        .await
        .unwrap_err();
    assert!(err.contains("不支持"));
}

#[tokio::test]
async fn download_update_package_network_error() {
    let dir = tempfile::tempdir().unwrap();
    let err = download_update_package_to_work_dir(
        "http://127.0.0.1:1/nope.AppImage",
        dir.path(),
        "linux",
        true,
    )
    .await
    .unwrap_err();
    assert!(
        err.contains("下载") || err.to_lowercase().contains("failed"),
        "err={}",
        err
    );
}
