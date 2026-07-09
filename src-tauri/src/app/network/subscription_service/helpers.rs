use crate::utils::app_util::get_work_dir_sync;
use std::path::{Path, PathBuf};
use tracing::warn;

fn sanitize_file_name(raw: &str) -> String {
    let mut sanitized: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();

    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        sanitized = "config.json".to_string();
    }

    sanitized
}

fn managed_config_dir() -> PathBuf {
    let work_dir = get_work_dir_sync();
    Path::new(&work_dir).join("sing-box/configs")
}

fn managed_target_path_from_input(input: &str, default_name: Option<&str>) -> PathBuf {
    let candidate = PathBuf::from(input);
    let fallback = default_name.unwrap_or("config.json");
    let file_name = candidate
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| fallback.to_string());

    managed_config_dir().join(sanitize_file_name(&file_name))
}

pub fn resolve_target_config_path(
    file_name: Option<String>,
    config_path: Option<String>,
) -> Result<PathBuf, String> {
    let config_dir = managed_config_dir();
    if let Err(e) = std::fs::create_dir_all(&config_dir) {
        return Err(format!("创建配置目录失败: {}", e));
    }

    if let Some(path) = config_path {
        // 安全收敛：订阅配置只允许落在工作目录下的托管 configs 目录，
        // 避免导入跨机器备份后继续写入旧机器绝对路径导致权限错误。
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Ok(managed_target_path_from_input(trimmed, None));
        }
    }

    let file = file_name
        .and_then(|name| {
            Path::new(&name)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            format!("config-{}.json", ts)
        });
    let safe_file = sanitize_file_name(&file);

    Ok(config_dir.join(safe_file))
}

pub fn backup_existing_config(target: &Path) -> Option<PathBuf> {
    if target.exists() {
        let backup = target.with_extension("bak");
        if let Err(e) = std::fs::copy(target, &backup) {
            warn!("备份配置失败: {}", e);
            None
        } else {
            Some(backup)
        }
    } else {
        None
    }
}

#[cfg(test)]
#[path = "helpers.tests.rs"]
mod tests;
