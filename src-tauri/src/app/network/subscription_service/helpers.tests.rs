use super::*;

#[test]
fn resolve_target_config_path_should_rebase_absolute_path() {
    let path = if cfg!(target_os = "windows") {
        r"C:\Users\legacy-user\AppData\Local\sing-box-windows\sing-box\configs\legacy.json"
            .to_string()
    } else {
        "/tmp/legacy-user/sing-box-windows/sing-box/configs/legacy.json".to_string()
    };

    let resolved = resolve_target_config_path(None, Some(path)).expect("should resolve path");
    assert!(resolved.starts_with(managed_config_dir()));
    assert_eq!(
        resolved.file_name().and_then(|v| v.to_str()),
        Some("legacy.json")
    );
}

#[test]
fn resolve_target_config_path_should_rebase_relative_path() {
    let resolved = resolve_target_config_path(None, Some("configs/original.json".to_string()))
        .expect("should resolve path");

    assert!(resolved.starts_with(managed_config_dir()));
    assert_eq!(
        resolved.file_name().and_then(|v| v.to_str()),
        Some("original.json")
    );
}

#[test]
fn resolve_target_and_backup_under_workspace() {
    use crate::test_support::TempWorkspace;
    let ws = TempWorkspace::new();
    let p = resolve_target_config_path(Some("my sub.json".into()), None).unwrap();
    assert!(p.to_string_lossy().contains("configs"));
    assert!(p.file_name().unwrap().to_string_lossy().contains("my"));

    let abs = resolve_target_config_path(None, Some("/tmp/outside/foo.json".into())).unwrap();
    assert!(abs.to_string_lossy().contains("configs"));
    assert!(abs.ends_with("foo.json") || abs.to_string_lossy().contains("foo"));

    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, b"{}").unwrap();
    let bak = backup_existing_config(&p);
    assert!(bak.is_some());
    assert!(bak.unwrap().exists());
    let _ = ws;
}
