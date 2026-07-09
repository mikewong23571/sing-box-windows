use crate::app::constants::messages;
use std::path::PathBuf;
use tracing::error;

/// 测试/CI 可覆盖工作目录的环境变量（见 docs/backend-test-coverage-plan.md D4）。
pub const WORK_DIR_ENV: &str = "SING_BOX_WINDOWS_WORK_DIR";

/// 解析工作目录路径（不创建目录）。优先 `SING_BOX_WINDOWS_WORK_DIR`。
pub fn resolve_work_dir_path() -> PathBuf {
    if let Ok(override_path) = std::env::var(WORK_DIR_ENV) {
        let trimmed = override_path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    if cfg!(target_os = "windows") {
        // Windows: %LOCALAPPDATA%\sing-box-windows
        std::env::var("LOCALAPPDATA")
            .map(|p| PathBuf::from(p).join("sing-box-windows"))
            .unwrap_or_else(|_| PathBuf::from(r"C:\ProgramData\sing-box-windows"))
    } else if cfg!(target_os = "linux") {
        // Linux: ~/.local/share/sing-box-windows
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("sing-box-windows")
    } else if cfg!(target_os = "macos") {
        // macOS: ~/Library/Application Support/sing-box-windows
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("sing-box-windows")
    } else {
        // 其他系统使用默认缓存目录
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("sing-box-windows")
    }
}

// 获取工作目录（同步版本）
pub fn get_work_dir_sync() -> String {
    let cache_dir = resolve_work_dir_path();

    // 确保目录存在
    if let Err(e) = std::fs::create_dir_all(&cache_dir) {
        error!("{}: {}", messages::ERR_CREATE_DIR_FAILED, e);
    }

    cache_dir.to_str().unwrap_or(".").to_string()
}

// 获取工作目录
pub async fn get_work_dir() -> String {
    let cache_dir = resolve_work_dir_path();

    // 确保目录存在
    if let Err(e) = tokio::fs::create_dir_all(&cache_dir).await {
        error!("{}: {}", messages::ERR_CREATE_DIR_FAILED, e);
    }

    cache_dir.to_str().unwrap_or(".").to_string()
}

/// 获取服务路径
pub fn get_service_path() -> PathBuf {
    // 获取可执行程序路径
    let exe_path = std::env::current_exe().expect("无法获取可执行程序路径");
    let work_dir = exe_path
        .parent()
        .expect("无法获取可执行程序父目录")
        .to_str()
        .expect("无法将父目录路径转换为字符串");

    // 根据平台确定可执行文件名
    let service_name = if cfg!(target_os = "windows") {
        "sing-box-service.exe"
    } else {
        "sing-box-service"
    };

    PathBuf::from(&work_dir)
        .join("src")
        .join("config")
        .join(service_name)
}

#[cfg(test)]
#[path = "app_util.tests.rs"]
mod tests;
