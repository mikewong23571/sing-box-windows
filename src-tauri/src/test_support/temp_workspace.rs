//! 隔离工作目录 + 可选 SQLite，串行化 env 覆盖。

use crate::utils::app_util::WORK_DIR_ENV;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

pub struct TempWorkspace {
    _dir: tempfile::TempDir,
    _guard: MutexGuard<'static, ()>,
    work_dir: PathBuf,
}

impl Default for TempWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

impl TempWorkspace {
    pub fn new() -> Self {
        // 覆盖率并行下若其它测试 panic 持锁，poison 后仍取回锁，避免连锁失败
        let guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        let work_dir = dir.path().to_path_buf();
        std::env::set_var(WORK_DIR_ENV, &work_dir);
        // 确保 get_work_dir 可见
        let _ = std::fs::create_dir_all(&work_dir);
        Self {
            _dir: dir,
            _guard: guard,
            work_dir,
        }
    }

    pub fn path(&self) -> &Path {
        &self.work_dir
    }

    pub fn join(&self, rel: &str) -> PathBuf {
        self.work_dir.join(rel)
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        // 仅清除仍指向本 workspace 的覆盖，避免并行测试互相踩 env
        if let Ok(current) = std::env::var(WORK_DIR_ENV) {
            if std::path::Path::new(&current) == self.work_dir.as_path() {
                std::env::remove_var(WORK_DIR_ENV);
            }
        }
    }
}
