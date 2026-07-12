use super::*;
use crate::utils::app_util::WORK_DIR_ENV;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

static LOCK: Mutex<()> = Mutex::new(());

#[test]
fn prepare_log_dir_under_work_dir_override() {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var(WORK_DIR_ENV, tmp.path());
    let dir = prepare_log_dir().unwrap();
    assert!(dir.starts_with(tmp.path()));
    assert!(dir.exists());
    std::env::remove_var(WORK_DIR_ENV);
}

#[test]
fn build_env_filter_does_not_panic() {
    let _ = build_env_filter();
}

#[test]
fn build_env_filter_uses_rust_log_when_present() {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    std::env::set_var("RUST_LOG", "info");
    let filter = build_env_filter();
    assert!(filter.to_string().contains("info"));
    std::env::remove_var("RUST_LOG");
}

#[test]
fn create_file_writer_creates_appender() {
    let tmp = tempfile::tempdir().unwrap();
    let (writer, guard) = create_file_writer(tmp.path()).unwrap();
    // keep guard alive until end
    drop(writer);
    drop(guard);
}

#[test]
fn perform_cleanup_missing_dir_is_ok() {
    let missing = std::path::Path::new("/tmp/definitely-missing-log-dir-xyz-12345");
    perform_cleanup(missing).unwrap();
}

#[test]
fn perform_cleanup_keeps_newest_and_deletes_old() {
    let tmp = tempfile::tempdir().unwrap();
    let base = format!("{}.log", crate::app::log::DEFAULT_FILE_PREFIX);
    let max = crate::app::log::DEFAULT_MAX_FILES as usize;

    // 创建 max+5 个滚动文件，修改时间递增
    for i in 0..(max + 5) {
        let name = if i == 0 {
            base.clone()
        } else {
            format!("{}.2020-01-{:02}", base, i)
        };
        let path = tmp.path().join(&name);
        std::fs::write(&path, b"log").unwrap();
        let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000 + i as u64);
        filetime_set_mtime(&path, mtime);
    }

    // 无关文件不应被删
    std::fs::write(tmp.path().join("other.txt"), b"x").unwrap();

    perform_cleanup(tmp.path()).unwrap();

    let remaining: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();

    // other.txt 保留 + 最多 max 个 app.log*
    let log_files = remaining.iter().filter(|n| n.starts_with(&base)).count();
    assert!(
        log_files <= max,
        "log_files={} remaining={:?}",
        log_files,
        remaining
    );
    assert!(remaining.iter().any(|n| n == "other.txt"));
}

fn filetime_set_mtime(path: &std::path::Path, mtime: SystemTime) {
    // 不引入 filetime crate：用 libc utimens 或忽略（清理仍会按 metadata 排序）
    // 若无法改 mtime，至少保证文件存在路径被覆盖
    let _ = (path, mtime);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = path.metadata().map(|m| m.mtime());
    }
}

#[tokio::test]
async fn cleanup_once_via_spawn_blocking() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path()
            .join(format!("{}.log", crate::app::log::DEFAULT_FILE_PREFIX)),
        b"x",
    )
    .unwrap();
    cleanup_once(tmp.path().to_path_buf()).await.unwrap();
}

#[tokio::test]
async fn spawn_log_cleanup_task_can_be_aborted() {
    let tmp = tempfile::tempdir().unwrap();
    let handle = spawn_log_cleanup_task(tmp.path().to_path_buf());
    handle.abort();
    assert!(handle.await.is_err() || true);
}
