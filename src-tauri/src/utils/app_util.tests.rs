use super::*;
use std::sync::Mutex;

// env 覆盖测试串行化，避免并行污染
static WORK_DIR_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn get_work_dir_sync_should_return_existing_sing_box_windows_dir() {
    let _guard = WORK_DIR_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let tmp_home = tempfile::tempdir().expect("tempdir");
    // Redirect platform default data dir into a temp home so the test stays hermetic.
    std::env::set_var("HOME", tmp_home.path());
    #[cfg(target_os = "windows")]
    std::env::set_var("USERPROFILE", tmp_home.path());
    std::env::remove_var(WORK_DIR_ENV);

    let work_dir = get_work_dir_sync();
    let work_dir_path = PathBuf::from(&work_dir);

    assert!(work_dir_path.exists());
    assert!(work_dir_path.ends_with("sing-box-windows"));
}

#[tokio::test]
async fn get_work_dir_should_return_existing_sing_box_windows_dir() {
    let _guard = WORK_DIR_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let tmp_home = tempfile::tempdir().expect("tempdir");
    std::env::set_var("HOME", tmp_home.path());
    #[cfg(target_os = "windows")]
    std::env::set_var("USERPROFILE", tmp_home.path());
    std::env::remove_var(WORK_DIR_ENV);

    let work_dir = get_work_dir().await;
    let work_dir_path = PathBuf::from(&work_dir);

    assert!(work_dir_path.exists());
    assert!(work_dir_path.ends_with("sing-box-windows"));
}

#[test]
fn work_dir_env_override_takes_precedence_sync_and_async() {
    let _guard = WORK_DIR_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let tmp = tempfile::tempdir().expect("tempdir");
    let override_path = tmp.path().join("custom-work");
    std::env::set_var(WORK_DIR_ENV, &override_path);

    let sync_path = PathBuf::from(get_work_dir_sync());
    assert_eq!(sync_path, override_path);
    assert!(sync_path.exists());

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let async_path = rt.block_on(async { PathBuf::from(get_work_dir().await) });
    assert_eq!(async_path, override_path);

    std::env::remove_var(WORK_DIR_ENV);
}

#[test]
fn get_service_path_should_point_to_expected_binary_name() {
    let service_path = get_service_path();

    #[cfg(target_os = "windows")]
    assert!(service_path.ends_with(r"src\config\sing-box-service.exe"));

    #[cfg(not(target_os = "windows"))]
    assert!(service_path.ends_with("src/config/sing-box-service"));
}

#[test]
fn resolve_work_dir_empty_env_uses_platform_default() {
    let _guard = WORK_DIR_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let tmp_home = tempfile::tempdir().expect("tempdir");
    std::env::set_var("HOME", tmp_home.path());
    #[cfg(target_os = "windows")]
    std::env::set_var("USERPROFILE", tmp_home.path());
    std::env::set_var(WORK_DIR_ENV, "   ");

    let path = resolve_work_dir_path();
    assert!(!path.as_os_str().is_empty());
    assert!(path.ends_with("sing-box-windows"));

    std::env::remove_var(WORK_DIR_ENV);
}
