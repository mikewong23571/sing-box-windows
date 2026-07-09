use crate::app::constants::process as process_constants;
use serde::{Deserialize, Serialize};
use std::fmt;

pub mod manager;

// 进程状态枚举
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProcessStatus {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed(String),
}

// 进程错误类型
#[derive(Debug, Clone)]
pub enum ProcessError {
    AlreadyRunning,
    NotRunning,
    StartFailed(String),
    StopFailed(String),
    StatusCheckFailed(String),
    ConfigError(String),
    SystemError(String),
    PermissionError(String),
    NetworkError(String),
    Unknown(String),
    Other(String),
}

impl From<std::io::Error> for ProcessError {
    fn from(err: std::io::Error) -> Self {
        ProcessError::SystemError(err.to_string())
    }
}

impl fmt::Display for ProcessError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ProcessError::AlreadyRunning => write!(f, "进程已在运行"),
            ProcessError::NotRunning => write!(f, "进程未运行"),
            ProcessError::StartFailed(msg) => write!(f, "启动失败: {}", msg),
            ProcessError::StopFailed(msg) => write!(f, "停止失败: {}", msg),
            ProcessError::StatusCheckFailed(msg) => write!(f, "进程状态检查失败: {}", msg),
            ProcessError::ConfigError(msg) => write!(f, "配置错误: {}", msg),
            ProcessError::SystemError(msg) => write!(f, "系统错误: {}", msg),
            ProcessError::PermissionError(msg) => write!(f, "权限错误: {}", msg),
            ProcessError::NetworkError(msg) => write!(f, "网络错误: {}", msg),
            ProcessError::Unknown(msg) => write!(f, "未知错误: {}", msg),
            ProcessError::Other(msg) => write!(f, "其他错误: {}", msg),
        }
    }
}

impl std::error::Error for ProcessError {}

pub type Result<T> = std::result::Result<T, ProcessError>;

// 进程配置结构体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessConfig {
    pub graceful_timeout: u64,      // 优雅关闭超时时间(秒)
    pub health_check_interval: u64, // 健康检查间隔(秒)
    pub max_restart_attempts: u32,  // 最大重启尝试次数
    pub restart_delay: u64,         // 重启延迟时间(秒)
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self {
            graceful_timeout: process_constants::GRACEFUL_TIMEOUT,
            health_check_interval: process_constants::HEALTH_CHECK_INTERVAL,
            max_restart_attempts: process_constants::MAX_RESTART_ATTEMPTS,
            restart_delay: process_constants::RESTART_DELAY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_error_display_covers_all_variants() {
        let cases: Vec<(ProcessError, &str)> = vec![
            (ProcessError::AlreadyRunning, "已在运行"),
            (ProcessError::NotRunning, "未运行"),
            (ProcessError::StartFailed("s".into()), "启动失败"),
            (ProcessError::StopFailed("s".into()), "停止失败"),
            (ProcessError::StatusCheckFailed("s".into()), "状态检查"),
            (ProcessError::ConfigError("s".into()), "配置错误"),
            (ProcessError::SystemError("s".into()), "系统错误"),
            (ProcessError::PermissionError("s".into()), "权限错误"),
            (ProcessError::NetworkError("s".into()), "网络错误"),
            (ProcessError::Unknown("s".into()), "未知错误"),
            (ProcessError::Other("s".into()), "其他错误"),
        ];
        for (err, needle) in cases {
            let s = err.to_string();
            assert!(s.contains(needle), "display={s} needle={needle}");
        }
        // From io::Error
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "io-x");
        let pe: ProcessError = io_err.into();
        assert!(matches!(pe, ProcessError::SystemError(_)));
        assert!(pe.to_string().contains("io-x"));
    }

    #[test]
    fn process_status_and_config_defaults() {
        let _ = ProcessStatus::Starting;
        let _ = ProcessStatus::Running;
        let _ = ProcessStatus::Stopping;
        let _ = ProcessStatus::Stopped;
        let failed = ProcessStatus::Failed("x".into());
        assert_eq!(failed, ProcessStatus::Failed("x".into()));
        assert_ne!(failed, ProcessStatus::Running);

        let cfg = ProcessConfig::default();
        assert!(cfg.graceful_timeout > 0);
        assert!(cfg.health_check_interval > 0);
        assert!(cfg.max_restart_attempts > 0);
        assert!(cfg.restart_delay > 0);

        // Serialize/deserialize roundtrip
        let json = serde_json::to_string(&ProcessStatus::Running).unwrap();
        let back: ProcessStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ProcessStatus::Running);
        let cfg_json = serde_json::to_string(&cfg).unwrap();
        let cfg2: ProcessConfig = serde_json::from_str(&cfg_json).unwrap();
        assert_eq!(cfg2.graceful_timeout, cfg.graceful_timeout);
    }
}
