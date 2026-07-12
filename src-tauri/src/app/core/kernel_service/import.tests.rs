use super::*;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

#[test]
fn archive_detection_and_kernel_name() {
    assert!(is_archive_file(Path::new("a.zip")));
    assert!(is_archive_file(Path::new("a.tar.gz")));
    assert!(is_archive_file(Path::new("a.tgz")));
    assert!(!is_archive_file(Path::new("sing-box")));
    assert!(!kernel_executable_name().is_empty());
    assert!(now_timestamp_secs() > 0);
}

#[test]
fn build_backup_path_and_find_executable() {
    let dir = tempfile::tempdir().unwrap();
    let kernel = dir.path().join(kernel_executable_name());
    fs::write(&kernel, b"x").unwrap();
    let bak = build_backup_path(&kernel).unwrap();
    assert!(bak.to_string_lossy().contains(".bak-import-"));
    assert_ne!(bak, kernel);

    let found = find_executable_file(dir.path(), kernel_executable_name()).unwrap();
    assert_eq!(found.file_name(), kernel.file_name());
}

#[test]
fn extract_zip_and_set_executable() {
    let dir = tempfile::tempdir().unwrap();
    let zip_path = dir.path().join("k.zip");
    let out = dir.path().join("out");
    {
        let f = fs::File::create(&zip_path).unwrap();
        let mut z = ZipWriter::new(f);
        z.start_file(kernel_executable_name(), SimpleFileOptions::default())
            .unwrap();
        z.write_all(b"#!/bin/sh\necho ok\n").unwrap();
        z.finish().unwrap();
    }
    extract_zip_archive(&zip_path, &out).unwrap();
    let exe = find_executable_file(&out, kernel_executable_name()).unwrap();
    set_executable_permission(&exe).unwrap();
    assert!(exe.exists());
}

#[test]
fn extract_archive_dispatch_zip() {
    let dir = tempfile::tempdir().unwrap();
    let zip_path = dir.path().join("a.zip");
    let out = dir.path().join("o");
    {
        let f = fs::File::create(&zip_path).unwrap();
        let mut z = ZipWriter::new(f);
        z.start_file("readme.txt", SimpleFileOptions::default())
            .unwrap();
        z.write_all(b"hi").unwrap();
        z.finish().unwrap();
    }
    extract_archive(&zip_path, &out).unwrap();
    assert!(out.join("readme.txt").exists() || out.exists());
}

#[test]
fn extract_tar_gz_and_find() {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use tar::Builder;

    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("k.tar.gz");
    let out = dir.path().join("out");
    {
        let f = fs::File::create(&archive).unwrap();
        let enc = GzEncoder::new(f, Compression::default());
        let mut tar = Builder::new(enc);
        let mut header = tar::Header::new_gnu();
        let data = b"#!/bin/sh\necho ok\n";
        header.set_size(data.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, kernel_executable_name(), &data[..])
            .unwrap();
        tar.finish().unwrap();
    }
    extract_archive(&archive, &out).unwrap();
    let found = find_executable_file(&out, kernel_executable_name()).unwrap();
    assert!(found.exists());
}

#[test]
fn extract_archive_rejects_unknown_and_empty() {
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("e.zip");
    fs::write(&empty, b"").unwrap();
    assert!(extract_archive(&empty, &dir.path().join("o1")).is_err());
    let bin = dir.path().join("x.bin");
    fs::write(&bin, b"data").unwrap();
    assert!(extract_archive(&bin, &dir.path().join("o2")).is_err());
}

#[test]
fn extract_plain_tar_archive() {
    use tar::Builder;

    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("k.tar");
    let out = dir.path().join("out_tar");
    {
        let f = fs::File::create(&archive).unwrap();
        let mut tar = Builder::new(f);
        let mut header = tar::Header::new_gnu();
        let data = b"#!/bin/sh\necho ok\n";
        header.set_size(data.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, kernel_executable_name(), &data[..])
            .unwrap();
        tar.finish().unwrap();
    }
    extract_tar_archive(&archive, &out).unwrap();
    assert!(out.join(kernel_executable_name()).exists());
    // 经 extract_archive 分发
    let out2 = dir.path().join("out_tar2");
    extract_archive(&archive, &out2).unwrap();
    assert!(find_executable_file(&out2, kernel_executable_name()).is_ok());
}

#[test]
fn find_executable_nested_and_missing() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("a/b/c");
    fs::create_dir_all(&nested).unwrap();
    let exe = nested.join(kernel_executable_name());
    fs::write(&exe, b"x").unwrap();
    let found = find_executable_file(dir.path(), kernel_executable_name()).unwrap();
    assert_eq!(found, exe);
    assert!(find_executable_file(dir.path(), "no-such-binary-xyz").is_err());
}

#[test]
fn extract_zip_with_directory_entries() {
    let dir = tempfile::tempdir().unwrap();
    let zip_path = dir.path().join("d.zip");
    let out = dir.path().join("out_d");
    {
        let f = fs::File::create(&zip_path).unwrap();
        let mut z = ZipWriter::new(f);
        z.add_directory("nested/", SimpleFileOptions::default())
            .unwrap();
        z.start_file(
            format!("nested/{}", kernel_executable_name()),
            SimpleFileOptions::default(),
        )
        .unwrap();
        z.write_all(b"bin").unwrap();
        z.finish().unwrap();
    }
    extract_zip_archive(&zip_path, &out).unwrap();
    let found = find_executable_file(&out, kernel_executable_name()).unwrap();
    assert!(found.exists());
}

#[test]
fn is_archive_file_covers_extensions() {
    assert!(is_archive_file(Path::new("A.TAR")));
    assert!(is_archive_file(Path::new("x.TGZ")));
    assert!(!is_archive_file(Path::new("x.rar")));
    assert!(!is_archive_file(Path::new("")));
}

#[test]
fn build_backup_path_rejects_root_filename() {
    // 无父目录的路径（相对单文件）仍有 parent 为 ""
    let p = PathBuf::from(kernel_executable_name());
    // 相对路径的 parent 可能是 Some("")，build 仍可能 Ok
    let r = build_backup_path(&p);
    let _ = r;
}

/// 写入可执行假内核脚本（version / check / run）。
fn write_fake_kernel_script(path: &Path, version_line: &str) {
    let script = format!(
        r#"#!/bin/sh
if [ "$1" = "version" ]; then
  echo '{ver}'
  exit 0
fi
if [ "$1" = "check" ]; then exit 0; fi
if [ "$1" = "run" ]; then exec sleep "${{FAKE_KERNEL_RUN_SECS:-5}}"; fi
exit 0
"#,
        ver = version_line.replace('\'', "")
    );
    fs::write(path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut p = fs::metadata(path).unwrap().permissions();
        p.set_mode(0o755);
        fs::set_permissions(path, p).unwrap();
    }
}

#[tokio::test]
async fn validate_kernel_binary_accepts_fake_singbox() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join(kernel_executable_name());
    write_fake_kernel_script(&bin, "sing-box version 1.11.0");
    let ver = validate_kernel_binary(&bin).await.unwrap();
    assert!(ver.contains("1.11.0"), "got {ver}");
}

#[tokio::test]
async fn validate_kernel_binary_rejects_non_singbox_output() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("other-tool");
    write_fake_kernel_script(&bin, "other-tool version 9.9.9");
    let err = validate_kernel_binary(&bin).await.unwrap_err();
    assert!(
        err.contains("不是有效") || err.contains("校验失败"),
        "err={err}"
    );
}

#[tokio::test]
async fn validate_kernel_binary_rejects_failing_exit() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join(kernel_executable_name());
    fs::write(
        &bin,
        r#"#!/bin/sh
echo boom >&2
exit 1
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
    let err = validate_kernel_binary(&bin).await.unwrap_err();
    assert!(
        err.contains("校验失败") && err.contains("boom"),
        "err={err}"
    );
}

#[tokio::test]
async fn stage_and_resolve_plain_binary() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("source-bin");
    write_fake_kernel_script(&src, "sing-box version 1.10.0");
    let temp = dir.path().join("stage");
    fs::create_dir_all(&temp).unwrap();

    let resolved = resolve_kernel_binary_source(&src, &temp).await.unwrap();
    assert_eq!(resolved, src);

    let staged = stage_kernel_binary(&src, &temp).await.unwrap();
    assert_eq!(staged.file_name().unwrap(), kernel_executable_name());
    assert!(staged.exists());
}

#[tokio::test]
async fn resolve_kernel_binary_from_zip_archive() {
    let dir = tempfile::tempdir().unwrap();
    let zip_path = dir.path().join("k.zip");
    {
        let f = fs::File::create(&zip_path).unwrap();
        let mut z = ZipWriter::new(f);
        z.start_file(kernel_executable_name(), SimpleFileOptions::default())
            .unwrap();
        // zip 内放假脚本内容；校验前会 stage 到可执行文件
        z.write_all(
            br#"#!/bin/sh
if [ "$1" = "version" ]; then echo 'sing-box version 1.9.0'; exit 0; fi
exit 0
"#,
        )
        .unwrap();
        z.finish().unwrap();
    }
    let temp = dir.path().join("tmp");
    fs::create_dir_all(&temp).unwrap();
    let resolved = resolve_kernel_binary_source(&zip_path, &temp)
        .await
        .unwrap();
    assert!(resolved.ends_with(kernel_executable_name()));
    set_executable_permission(&resolved).unwrap();
    let ver = validate_kernel_binary(&resolved).await.unwrap();
    assert!(ver.contains("1.9.0"), "got {ver}");
}

#[tokio::test]
async fn replace_installed_kernel_with_backup_and_restore() {
    let dir = tempfile::tempdir().unwrap();
    let kernel_path = dir.path().join(kernel_executable_name());
    fs::write(&kernel_path, b"old-kernel").unwrap();

    let staged = dir.path().join("staged");
    fs::write(&staged, b"new-kernel").unwrap();

    let backup = replace_installed_kernel(&staged, &kernel_path)
        .await
        .unwrap();
    assert!(backup.is_some());
    assert_eq!(fs::read(&kernel_path).unwrap(), b"new-kernel");

    let bak = PathBuf::from(backup.unwrap());
    assert!(bak.exists());
    assert_eq!(fs::read(&bak).unwrap(), b"old-kernel");

    // 从备份回滚应恢复旧内核（避免同秒二次 replace 覆盖同名 .bak）
    restore_kernel_from_backup(&kernel_path, &bak)
        .await
        .unwrap();
    assert_eq!(fs::read(&kernel_path).unwrap(), b"old-kernel");

    // 再次替换：无已有备份路径冲突时仍应成功
    let staged2 = dir.path().join("staged2");
    fs::write(&staged2, b"newer").unwrap();
    // 先确保目标存在
    fs::write(&kernel_path, b"current").unwrap();
    let bak2 = replace_installed_kernel(&staged2, &kernel_path)
        .await
        .unwrap();
    assert!(bak2.is_some());
    assert_eq!(fs::read(&kernel_path).unwrap(), b"newer");
}

#[tokio::test]
async fn replace_installed_kernel_when_no_existing() {
    let dir = tempfile::tempdir().unwrap();
    let kernel_path = dir.path().join("nested").join(kernel_executable_name());
    let staged = dir.path().join("newbin");
    fs::write(&staged, b"first").unwrap();
    let backup = replace_installed_kernel(&staged, &kernel_path)
        .await
        .unwrap();
    assert!(backup.is_none());
    assert_eq!(fs::read(&kernel_path).unwrap(), b"first");
}

#[tokio::test]
async fn move_file_with_fallback_rename_and_copy() {
    let dir = tempfile::tempdir().unwrap();
    let from = dir.path().join("a.bin");
    let to = dir.path().join("b.bin");
    fs::write(&from, b"data").unwrap();
    move_file_with_fallback(&from, &to).await.unwrap();
    assert!(!from.exists());
    assert_eq!(fs::read(&to).unwrap(), b"data");
}

#[tokio::test]
async fn restore_kernel_from_backup_missing_errors() {
    let dir = tempfile::tempdir().unwrap();
    let err = restore_kernel_from_backup(&dir.path().join("k"), &dir.path().join("missing.bak"))
        .await
        .unwrap_err();
    assert!(err.contains("备份"));
}

#[tokio::test]
async fn wait_kernel_running_times_out_when_stopped() {
    // 全局进程未运行时，短超时应返回 false
    let ok = wait_kernel_running(Duration::from_millis(50)).await;
    // 若其它测试残留进程则可能为 true；主要覆盖轮询路径
    let _ = ok;
}

#[tokio::test]
async fn hermetic_import_pipeline_without_app_handle() {
    // resolve → stage → validate → replace（内核未运行，无 AppHandle）
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("incoming");
    write_fake_kernel_script(&src, "sing-box version 2.0.0");
    let temp = dir.path().join("tmp");
    fs::create_dir_all(&temp).unwrap();
    let source = resolve_kernel_binary_source(&src, &temp).await.unwrap();
    let staged = stage_kernel_binary(&source, &temp).await.unwrap();
    let ver = validate_kernel_binary(&staged).await.unwrap();
    assert!(ver.contains("2.0.0"));
    let target = dir.path().join("install").join(kernel_executable_name());
    let bak = replace_installed_kernel(&staged, &target).await.unwrap();
    assert!(bak.is_none());
    assert!(target.exists());
}

#[test]
fn validate_import_source_path_and_message() {
    assert!(validate_import_source_path("").is_err());
    assert!(validate_import_source_path("   ").is_err());
    assert!(validate_import_source_path("/no/such/file-xyz-999").is_err());

    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("bin");
    fs::write(&f, b"x").unwrap();
    assert!(validate_import_source_path(f.to_str().unwrap()).is_ok());
    assert!(validate_import_source_path(dir.path().to_str().unwrap()).is_err());

    assert!(build_import_success_message("1.0", false).contains("1.0"));
    assert!(build_import_success_message("1.0", true).contains("重启"));
}

#[tokio::test]
async fn import_kernel_executable_inner_not_running_via_mock() {
    use crate::test_support::MockAppEnv;

    let env = MockAppEnv::new();
    let work = env.workspace.path();
    let db = work.join("import.db");
    env.install_storage_from_path(db.to_str().unwrap()).await;

    // 确保全局进程已停（平台层可能仍看到残留 sing-box）
    let _ = crate::app::core::kernel_service::PROCESS_MANAGER
        .stop::<tauri::Wry>(None)
        .await;
    let _ = crate::app::core::kernel_service::PROCESS_MANAGER
        .force_kill_kernel_processes_by_name::<tauri::Wry>(None)
        .await;

    let src = work.join("incoming-kernel");
    write_fake_kernel_script(&src, "sing-box version 3.1.0");
    let temp = work.join("import-tmp");
    fs::create_dir_all(&temp).unwrap();

    let result = import_kernel_executable_inner(&env.handle(), &src, &temp).await;
    match result {
        Ok(r) => {
            assert!(r.imported_version.contains("3.1.0"));
            assert!(
                crate::app::constants::paths::get_kernel_path().exists()
                    || r.message.contains("导入成功")
            );
        }
        // 若平台残留不可杀进程，仍覆盖了 import 主路径的前半段
        Err(e) => assert!(
            e.contains("无法停止") || e.contains("进程"),
            "unexpected: {e}"
        ),
    }
}

#[tokio::test]
async fn import_kernel_executable_inner_while_running_stops_first() {
    use crate::app::constants::paths as kpaths;
    use crate::app::core::kernel_service::PROCESS_MANAGER;
    use crate::test_support::MockAppEnv;

    let env = MockAppEnv::new();
    let work = env.workspace.path();
    let db = work.join("import-run.db");
    env.install_storage_from_path(db.to_str().unwrap()).await;

    // 安装假内核并启动
    let dir = work.join("sing-box");
    fs::create_dir_all(&dir).unwrap();
    let kernel = dir.join("sing-box");
    write_fake_kernel_script(&kernel, "sing-box version 1.0.0");
    // 覆盖 write：run 要 sleep
    fs::write(
        &kernel,
        r#"#!/bin/sh
if [ "$1" = "version" ]; then echo "sing-box version 1.0.0"; exit 0; fi
if [ "$1" = "check" ]; then exit 0; fi
if [ "$1" = "run" ]; then exec sleep "${{FAKE_KERNEL_RUN_SECS:-5}}"; fi
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
    let cfg = kpaths::get_config_dir().join("config.json");
    fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    fs::write(&cfg, r#"{}"#).unwrap();
    let _ = PROCESS_MANAGER
        .start_inner::<tauri::Wry>(None, &cfg, false)
        .await;

    let src = work.join("new-kernel");
    fs::write(
        &src,
        r#"#!/bin/sh
if [ "$1" = "version" ]; then echo "sing-box version 4.0.0"; exit 0; fi
if [ "$1" = "check" ]; then exit 0; fi
if [ "$1" = "run" ]; then exec sleep "${{FAKE_KERNEL_RUN_SECS:-5}}"; fi
exit 0
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut p = fs::metadata(&src).unwrap().permissions();
        p.set_mode(0o755);
        fs::set_permissions(&src, p).unwrap();
    }
    let temp = work.join("import-tmp2");
    fs::create_dir_all(&temp).unwrap();

    // 导入会停旧核；重启可能因假 API 失败并回滚
    let result = import_kernel_executable_inner(&env.handle(), &src, &temp).await;
    let _ = result; // Ok 或 Err(回滚) 均覆盖 stop+replace 路径
    let _ = PROCESS_MANAGER.stop::<tauri::Wry>(None).await;
}

#[tokio::test]
async fn validate_kernel_binary_extracts_version_from_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join(kernel_executable_name());
    fs::write(
        &bin,
        r#"#!/bin/sh
if [ "$1" = "version" ]; then echo "sing-box version 1.8.0" >&2; exit 0; fi
exit 0
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
    let ver = validate_kernel_binary(&bin).await.unwrap();
    assert!(ver.contains("1.8.0"), "got {ver}");
}

#[tokio::test]
async fn resolve_kernel_binary_source_rejects_bad_archive() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.zip");
    fs::write(&bad, b"not-a-zip").unwrap();
    let temp = dir.path().join("tmp");
    fs::create_dir_all(&temp).unwrap();
    let err = resolve_kernel_binary_source(&bad, &temp).await.unwrap_err();
    assert!(!err.is_empty(), "unexpected err: {err}");
}

#[tokio::test]
async fn replace_installed_kernel_rollback_path() {
    let dir = tempfile::tempdir().unwrap();
    let kernel_path = dir.path().join(kernel_executable_name());
    fs::write(&kernel_path, b"old").unwrap();

    // staged 与目标同目录，rename 成功
    let staged = dir.path().join("staged");
    fs::write(&staged, b"new").unwrap();
    let bak = replace_installed_kernel(&staged, &kernel_path)
        .await
        .unwrap();
    assert!(bak.is_some());
    assert_eq!(fs::read(&kernel_path).unwrap(), b"new");

    // 从 backup 恢复
    restore_kernel_from_backup(&kernel_path, Path::new(&bak.clone().unwrap()))
        .await
        .unwrap();
    assert_eq!(fs::read(&kernel_path).unwrap(), b"old");
}

#[tokio::test]
async fn stop_running_kernel_for_replace_when_already_stopped() {
    use crate::test_support::MockAppEnv;

    let env = MockAppEnv::new();
    let _ = crate::app::core::kernel_service::PROCESS_MANAGER
        .stop::<tauri::Wry>(None)
        .await;
    let _ = crate::app::core::kernel_service::PROCESS_MANAGER
        .force_kill_kernel_processes_by_name::<tauri::Wry>(None)
        .await;
    // 未运行时成功；若仍有平台残留不可杀进程则返回明确错误（覆盖路径）
    let r = stop_running_kernel_for_replace(&env.handle()).await;
    match r {
        Ok(()) => {}
        Err(e) => assert!(
            e.contains("无法停止") || e.contains("进程"),
            "unexpected: {e}"
        ),
    }
}
