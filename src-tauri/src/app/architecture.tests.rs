use std::fs;
use std::path::{Path, PathBuf};

fn rust_files_under(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let entries = fs::read_dir(dir).expect("architecture test directory should exist");

    for entry in entries {
        let path = entry.expect("directory entry should be readable").path();
        if path.is_dir() {
            files.extend(rust_files_under(&path));
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            files.push(path);
        }
    }

    files
}

fn all_app_rust_files() -> Vec<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    rust_files_under(&manifest_dir.join("src/app"))
}

#[test]
fn storage_must_not_depend_on_runtime_or_network_services() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let storage_dir = manifest_dir.join("src/app/storage");
    let forbidden_patterns = [
        "crate::app::core::kernel",
        "crate::app::runtime",
        "crate::app::network",
        "kernel_auto_manage",
        "kernel_service",
        "subscription_service",
    ];

    let mut violations = Vec::new();
    for file in rust_files_under(&storage_dir) {
        let content = fs::read_to_string(&file).expect("source file should be readable");
        for pattern in forbidden_patterns {
            if content.contains(pattern) {
                violations.push(format!("{} contains `{}`", file.display(), pattern));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "storage layer must stay persistence-only:\n{}",
        violations.join("\n")
    );
}

#[test]
fn runtime_changes_must_use_runtime_orchestrator() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let allowed_files = [
        manifest_dir.join("src/app/architecture.tests.rs"),
        manifest_dir.join("src/app/core/kernel_auto_manage.rs"),
        manifest_dir.join("src/app/runtime/orchestrator.rs"),
    ];

    let mut violations = Vec::new();
    for file in all_app_rust_files() {
        if allowed_files.iter().any(|allowed| allowed == &file) {
            continue;
        }

        let content = fs::read_to_string(&file).expect("source file should be readable");
        if content.contains("auto_manage_with_saved_config(")
            || content.contains("run_auto_manage_with_saved_config(")
        {
            violations.push(file.display().to_string());
        }
    }

    assert!(
        violations.is_empty(),
        "runtime changes must go through runtime::orchestrator:\n{}",
        violations.join("\n")
    );
}
