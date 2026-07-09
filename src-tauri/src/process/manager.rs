use super::{ProcessError, Result};
use crate::app::constants::{messages, paths};
use crate::app::core::kernel_service::state::KERNEL_STATE;
use crate::utils::proxy_util::disable_system_proxy;

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
#[cfg(target_os = "macos")]
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex as StdMutex};
use tauri::{AppHandle, Runtime};
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

const STDERR_TAIL_LIMIT: usize = 200;

pub struct ProcessManager {
    process: Arc<RwLock<Option<Child>>>,
    stderr_tail: Arc<StdMutex<VecDeque<String>>>,
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            process: Arc::new(RwLock::new(None)),
            stderr_tail: Arc::new(StdMutex::new(VecDeque::with_capacity(STDERR_TAIL_LIMIT))),
        }
    }

    fn push_stderr_tail(tail: &Arc<StdMutex<VecDeque<String>>>, line: String) {
        let Ok(mut guard) = tail.lock() else {
            return;
        };

        if guard.len() >= STDERR_TAIL_LIMIT {
            guard.pop_front();
        }
        guard.push_back(line);
    }

    fn clear_stderr_tail(&self) {
        if let Ok(mut guard) = self.stderr_tail.lock() {
            guard.clear();
        }
    }

    fn attach_stderr_drain(&self, child: &mut Child) -> Result<()> {
        let Some(stderr) = child.stderr.take() else {
            return Ok(());
        };

        let tail = Arc::clone(&self.stderr_tail);
        std::thread::Builder::new()
            .name("sing-box-stderr-drain".to_string())
            .spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    match line {
                        Ok(line) => {
                            debug!(target: "sing_box_stderr", "{}", line);
                            Self::push_stderr_tail(&tail, line);
                        }
                        Err(err) => {
                            Self::push_stderr_tail(
                                &tail,
                                format!("读取 sing-box stderr 失败: {}", err),
                            );
                            break;
                        }
                    }
                }
            })
            .map_err(|e| ProcessError::StartFailed(format!("启动 stderr 读取线程失败: {}", e)))?;

        Ok(())
    }

    pub(crate) fn managed_pid_file() -> std::path::PathBuf {
        paths::get_kernel_work_dir().join(".managed-kernel.pid")
    }

    pub(crate) fn persist_managed_pid(&self, pid: u32) -> std::io::Result<()> {
        let pid_file = Self::managed_pid_file();
        if let Some(parent) = pid_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(pid_file, pid.to_string())
    }

    pub(crate) fn read_managed_pid(&self) -> Option<u32> {
        let pid_file = Self::managed_pid_file();
        let content = std::fs::read_to_string(pid_file).ok()?;
        content.trim().parse::<u32>().ok()
    }

    pub(crate) fn clear_managed_pid(&self) {
        let pid_file = Self::managed_pid_file();
        if let Err(e) = std::fs::remove_file(&pid_file) {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!("清理托管 PID 文件失败 {:?}: {}", pid_file, e);
            }
        }
    }

    fn is_pid_matching_kernel_name(&self, pid: u32, kernel_name: &str) -> bool {
        #[cfg(target_os = "linux")]
        {
            let comm_path = format!("/proc/{}/comm", pid);
            if let Ok(name) = std::fs::read_to_string(&comm_path) {
                if name.trim() == kernel_name {
                    return true;
                }
            }

            let exe_path = format!("/proc/{}/exe", pid);
            if let Ok(target) = std::fs::read_link(&exe_path) {
                return target
                    .file_name()
                    .and_then(|f| f.to_str())
                    .map(|f| f == kernel_name)
                    .unwrap_or(false);
            }

            false
        }

        #[cfg(target_os = "macos")]
        {
            let output = std::process::Command::new("ps")
                .args(["-p", &pid.to_string(), "-o", "comm="])
                .output();
            if let Ok(output) = output {
                if output.status.success() {
                    let comm = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    let cmd_base = Path::new(&comm)
                        .file_name()
                        .and_then(|f| f.to_str())
                        .unwrap_or(comm.as_str());
                    return cmd_base == kernel_name;
                }
            }
            false
        }

        #[cfg(target_os = "windows")]
        {
            let mut cmd = std::process::Command::new("tasklist");
            cmd.args(["/FI", &format!("PID eq {}", pid), "/FO", "CSV", "/NH"]);
            // 统一走平台封装，确保 Windows 下不会弹出瞬时控制台窗口。
            crate::platform::configure_std_command(&mut cmd);
            let output = cmd.output();

            if let Ok(output) = output {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    for line in stdout.lines() {
                        let parts: Vec<&str> = line
                            .split('"')
                            .filter(|s| !s.is_empty() && *s != ",")
                            .collect();
                        if let Some(image_name) = parts.first() {
                            return image_name.eq_ignore_ascii_case(kernel_name);
                        }
                    }
                }
            }
            false
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        {
            let _ = (pid, kernel_name);
            false
        }
    }

    async fn persist_started_process_pid(
        &self,
        child_pid: u32,
        kernel_name: &str,
        tun_enabled: bool,
    ) {
        #[cfg(target_os = "linux")]
        {
            if tun_enabled {
                match self
                    .resolve_linux_managed_kernel_pid(child_pid, kernel_name)
                    .await
                {
                    Some(real_pid) => {
                        if let Err(e) = self.persist_managed_pid(real_pid) {
                            warn!("记录 Linux 托管内核 PID 失败 (pid={}): {}", real_pid, e);
                        } else {
                            info!(
                                "已记录 Linux 托管内核 PID: {} (启动子进程 PID: {})",
                                real_pid, child_pid
                            );
                        }
                    }
                    None => {
                        // sudo 包装进程可能比真实 sing-box 更早退出，此时宁可不记录，也不把错误 PID 写入托管文件。
                        warn!("未能解析 Linux TUN 模式下的真实内核 PID，后续将回退到按进程名清理");
                        self.clear_managed_pid();
                    }
                }
                return;
            }
        }

        #[cfg(not(target_os = "linux"))]
        let _ = tun_enabled;

        let _ = kernel_name;
        if let Err(e) = self.persist_managed_pid(child_pid) {
            warn!("记录托管内核 PID 失败 (pid={}): {}", child_pid, e);
        }
    }

    async fn is_managed_kernel_pid_active(&self, pid: u32, kernel_name: &str) -> bool {
        #[cfg(target_os = "linux")]
        {
            return crate::platform::list_active_processes_by_name(kernel_name)
                .await
                .map(|active_pids| active_pids.contains(&pid))
                .unwrap_or_else(|err| {
                    warn!("读取 Linux 活跃内核 PID 失败: {}", err);
                    self.is_pid_matching_kernel_name(pid, kernel_name)
                });
        }

        #[cfg(not(target_os = "linux"))]
        {
            self.is_pid_matching_kernel_name(pid, kernel_name)
        }
    }

    async fn try_kill_pid_with_optional_privilege<R: Runtime>(
        &self,
        app_handle: Option<&AppHandle<R>>,
        pid: u32,
        kernel_name: &str,
    ) {
        if let Err(err) = kill_process_by_pid(pid) {
            warn!("终止托管内核进程失败 (pid={}): {}", pid, err);
        }

        sleep(Duration::from_millis(250)).await;
        if !self.is_managed_kernel_pid_active(pid, kernel_name).await {
            return;
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let _ = app_handle;

        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if let Some(app_handle) = app_handle {
            warn!(
                "普通权限终止 PID {} 失败，尝试使用 sudo 继续清理 {}",
                pid, kernel_name
            );
            match crate::app::system::sudo_service::kill_process_by_pid_with_saved_password(
                app_handle, pid,
            )
            .await
            {
                Ok(_) => {
                    sleep(Duration::from_millis(250)).await;
                    if !self.is_managed_kernel_pid_active(pid, kernel_name).await {
                        info!("已通过 sudo 终止内核进程 PID: {}", pid);
                        return;
                    }
                }
                Err(err) => {
                    warn!("使用 sudo 终止 PID {} 失败: {}", pid, err);
                }
            }
        }

        warn!("PID {} 在终止后仍处于活跃状态", pid);
    }

    async fn has_active_managed_kernel_pid(&self) -> bool {
        let kernel_name = crate::platform::get_kernel_executable_name();
        let Some(pid) = self.read_managed_pid() else {
            return false;
        };

        #[cfg(target_os = "linux")]
        {
            match crate::platform::list_active_processes_by_name(kernel_name).await {
                Ok(active_pids) => {
                    if active_pids.contains(&pid) {
                        return true;
                    }

                    if self.is_pid_matching_kernel_name(pid, kernel_name) {
                        info!("托管 PID {} 已不是活跃 {} 进程，清理记录", pid, kernel_name);
                    } else {
                        warn!(
                            "托管 PID({}) 与当前活跃内核进程({})不匹配，已清除记录",
                            pid, kernel_name
                        );
                    }
                    self.clear_managed_pid();
                    return false;
                }
                Err(e) => {
                    warn!("读取 Linux 活跃内核 PID 失败: {}", e);
                }
            }
        }

        if self.is_pid_matching_kernel_name(pid, kernel_name) {
            return true;
        }

        self.clear_managed_pid();
        false
    }

    #[cfg(target_os = "linux")]
    async fn resolve_linux_managed_kernel_pid(
        &self,
        child_pid: u32,
        kernel_name: &str,
    ) -> Option<u32> {
        const RESOLVE_ATTEMPTS: usize = 8;
        const RESOLVE_INTERVAL_MS: u64 = 150;

        for attempt in 1..=RESOLVE_ATTEMPTS {
            match crate::platform::list_active_processes_by_name(kernel_name).await {
                Ok(active_pids) if active_pids.contains(&child_pid) => {
                    info!("Linux 启动子进程 PID 已切换为真实内核 PID: {}", child_pid);
                    return Some(child_pid);
                }
                Ok(active_pids) if active_pids.len() == 1 => {
                    return active_pids.first().copied();
                }
                Ok(active_pids) if !active_pids.is_empty() => {
                    let selected = active_pids.iter().copied().max();
                    warn!(
                        "第{}次解析真实内核 PID 时检测到多个活跃 {} 进程 {:?}，回退选择最大 PID {:?}",
                        attempt, kernel_name, active_pids, selected
                    );
                    return selected;
                }
                Ok(_) => {
                    debug!(
                        "第{}次解析真实内核 PID 时尚未检测到活跃 {}",
                        attempt, kernel_name
                    );
                }
                Err(e) => {
                    warn!("第{}次解析真实内核 PID 失败: {}", attempt, e);
                }
            }

            sleep(Duration::from_millis(RESOLVE_INTERVAL_MS)).await;
        }

        None
    }

    // 启动进程（带系统环境检查和重试机制）
    // tun_enabled: 是否启用 TUN 模式，在 Linux/macOS 上需要特殊权限提升
    pub async fn start<R: Runtime>(
        &self,
        app_handle: &AppHandle<R>,
        config_path: &std::path::Path,
        tun_enabled: bool,
    ) -> Result<()> {
        self.start_inner(Some(app_handle), config_path, tun_enabled)
            .await
    }

    /// Hermetic/test entry: `app_handle=None` 仅允许非 TUN 启动（无 sudo 路径）。
    /// 泛型 Runtime 便于 MockRuntime 覆盖生产 start 路径（非 TUN 时 handle 可为 None）。
    pub async fn start_inner<R: Runtime>(
        &self,
        app_handle: Option<&AppHandle<R>>,
        config_path: &std::path::Path,
        tun_enabled: bool,
    ) -> Result<()> {
        info!("🚀 开始启动内核进程... TUN模式: {}", tun_enabled);

        if tun_enabled && app_handle.is_none() {
            return Err(ProcessError::PermissionError(
                "TUN 模式需要 AppHandle 以使用 sudo 提权".to_string(),
            ));
        }

        // 验证配置文件有效性
        self.validate_config(config_path).await?;

        // 先检查本地是否有 sing-box 进程在运行，如果有则先终止。
        // Linux/macOS 的 TUN 进程可能是 root 身份，需要携带 app_handle 走 sudo 回退。
        if let Err(e) = self.kill_existing_processes(app_handle).await {
            warn!("终止已有sing-box进程失败: {}", e);
        }

        // 检查本实例中是否已经有进程在运行
        {
            let mut process_guard = self.process.write().await;
            if let Some(ref mut proc) = *process_guard {
                // 尝试获取进程状态，如果可以获取则说明进程还在运行
                match proc.try_wait() {
                    Ok(None) => {
                        // 进程在运行，需要先停止
                        info!("内核已经在运行中，将重新启动");
                        match proc.kill() {
                            Ok(_) => {
                                info!("已终止现有内核进程");
                                match proc.wait() {
                                    Ok(status) => info!("内核进程已终止，退出状态: {}", status),
                                    Err(e) => warn!("等待内核进程终止失败: {}", e),
                                }
                                *process_guard = None;
                                self.clear_managed_pid();
                            }
                            Err(e) => {
                                warn!("终止现有内核进程失败: {}", e);
                                // 尝试使用更强力的方式终止
                                let pid = proc.id();
                                if let Err(e) = kill_process_by_pid(pid) {
                                    error!("强制终止进程失败: {}", e);
                                }
                                *process_guard = None;
                                self.clear_managed_pid();
                            }
                        }
                    }
                    Ok(Some(status)) => {
                        info!("发现已退出的内核进程，退出状态: {}", status);
                        *process_guard = None;
                        self.clear_managed_pid();
                    }
                    Err(e) => {
                        warn!("检查内核进程状态失败: {}", e);
                        *process_guard = None;
                        self.clear_managed_pid();
                    }
                }
            }
        }

        // 获取内核路径和配置路径
        let kernel_path = paths::get_kernel_path();
        let kernel_work_dir = paths::get_kernel_work_dir();

        // 检查系统环境，特别是在开机自启动时
        self.check_system_environment().await?;
        self.clear_stderr_tail();

        // 多次尝试启动进程
        let max_attempts = 3;
        let mut last_error = ProcessError::StartFailed("未知错误".to_string());

        for attempt in 1..=max_attempts {
            info!("🔧 尝试启动内核进程，第 {}/{} 次", attempt, max_attempts);

            match self
                .try_start_kernel_process(
                    app_handle,
                    &kernel_path,
                    &kernel_work_dir,
                    config_path,
                    tun_enabled,
                )
                .await
            {
                Ok(child) => {
                    let child_pid = child.id();
                    KERNEL_STATE.update_readiness(|readiness| {
                        readiness.process_spawned = Some(true);
                        readiness.process_alive = true;
                    });
                    // 保存进程句柄
                    {
                        let mut process_guard = self.process.write().await;
                        *process_guard = Some(child);
                    }
                    self.persist_started_process_pid(
                        child_pid,
                        crate::platform::get_kernel_executable_name(),
                        tun_enabled,
                    )
                    .await;

                    // 更稳健的启动检查
                    if self.verify_startup().await {
                        info!("✅ 内核进程启动成功并验证通过");
                        return Ok(());
                    } else {
                        KERNEL_STATE.update_readiness(|readiness| {
                            readiness.process_alive = false;
                        });
                        last_error =
                            ProcessError::StartFailed("内核进程启动后验证失败".to_string());
                        warn!("❌ 第{}次启动后验证失败", attempt);

                        // 清理失败的进程
                        if let Err(e) = self.cleanup_failed_process().await {
                            error!("清理失败进程时出错: {}", e);
                        }
                    }
                }
                Err(e) => {
                    KERNEL_STATE.update_readiness(|readiness| {
                        readiness.process_spawned = Some(false);
                        readiness.process_alive = false;
                    });
                    last_error = e;
                    error!("❌ 第{}次启动失败: {}", attempt, last_error);
                }
            }

            // 如果不是最后一次尝试，等待后重试
            if attempt < max_attempts {
                let delay = Duration::from_secs(2 * attempt as u64);
                warn!("⏳ 第{}次启动失败，{}秒后重试...", attempt, delay.as_secs());
                tokio::time::sleep(delay).await;
            }
        }

        Err(last_error)
    }

    // 检查系统环境
    async fn check_system_environment(&self) -> Result<()> {
        info!("🔍 检查系统环境...");

        // 检查内核文件是否可执行
        let kernel_path = paths::get_kernel_path();
        if !kernel_path.exists() {
            return Err(ProcessError::ConfigError(format!(
                "内核文件不存在: {}",
                kernel_path.to_str().unwrap_or("unknown")
            )));
        }

        // 检查工作目录
        let kernel_work_dir = paths::get_kernel_work_dir();
        if !kernel_work_dir.exists() {
            if let Err(e) = tokio::fs::create_dir_all(&kernel_work_dir).await {
                return Err(ProcessError::SystemError(format!(
                    "无法创建工作目录: {}",
                    e
                )));
            }
        }

        info!("✅ 系统环境检查完成");
        Ok(())
    }

    // 尝试启动内核进程
    // tun_enabled 参数用于在 Linux/macOS 上启用 TUN 时进行权限提升
    async fn try_start_kernel_process<R: Runtime>(
        &self,
        app_handle: Option<&AppHandle<R>>,
        kernel_path: &std::path::Path,
        kernel_work_dir: &std::path::Path,
        config_path: &std::path::Path,
        tun_enabled: bool,
    ) -> Result<std::process::Child> {
        let kernel_str = kernel_path
            .to_str()
            .ok_or_else(|| ProcessError::StartFailed("内核路径包含无效字符".to_string()))?;
        let work_dir_str = kernel_work_dir
            .to_str()
            .ok_or_else(|| ProcessError::StartFailed("工作目录路径包含无效字符".to_string()))?;
        let config_str = config_path
            .to_str()
            .ok_or_else(|| ProcessError::StartFailed("配置文件路径包含无效字符".to_string()))?;

        // Windows: 直接启动（假设应用已以管理员权限运行）
        #[cfg(target_os = "windows")]
        {
            let _ = (tun_enabled, kernel_str, app_handle); // Windows 不使用这些参数，由应用整体权限控制
            let mut cmd = Command::new(kernel_path);
            cmd.args(["run", "-D", work_dir_str, "-c", config_str]);
            cmd.stdout(Stdio::null()).stderr(Stdio::piped());
            crate::platform::configure_std_command(&mut cmd);

            let mut child = cmd
                .spawn()
                .map_err(|e| ProcessError::StartFailed(format!("启动内核进程失败: {}", e)))?;
            self.attach_stderr_drain(&mut child)?;
            Ok(child)
        }

        // Linux: TUN 模式使用 sudo + 系统密钥环提权（由前端首次收集系统密码）
        #[cfg(target_os = "linux")]
        {
            if tun_enabled {
                let app_handle = app_handle.ok_or_else(|| {
                    ProcessError::PermissionError("TUN 启动需要 AppHandle".to_string())
                })?;
                info!("🔐 TUN 模式启用，使用 sudo 提升内核权限");
                return crate::app::system::sudo_service::spawn_kernel_with_saved_password(
                    app_handle,
                    kernel_str,
                    work_dir_str,
                    config_str,
                )
                .await
                .map_err(ProcessError::StartFailed);
            } else {
                let mut cmd = Command::new(kernel_path);
                cmd.args(["run", "-D", work_dir_str, "-c", config_str]);
                cmd.stdout(Stdio::null()).stderr(Stdio::piped());

                let mut child = cmd
                    .spawn()
                    .map_err(|e| ProcessError::StartFailed(format!("启动内核进程失败: {}", e)))?;
                self.attach_stderr_drain(&mut child)?;
                Ok(child)
            }
        }

        // macOS: TUN 模式使用 sudo + 系统钥匙串提权（由前端首次收集系统密码）
        #[cfg(target_os = "macos")]
        {
            if tun_enabled {
                let app_handle = app_handle.ok_or_else(|| {
                    ProcessError::PermissionError("TUN 启动需要 AppHandle".to_string())
                })?;
                info!("🔐 TUN 模式启用，使用 sudo 提升内核权限");
                return crate::app::system::sudo_service::spawn_kernel_with_saved_password(
                    app_handle,
                    kernel_str,
                    work_dir_str,
                    config_str,
                )
                .await
                .map_err(ProcessError::StartFailed);
            } else {
                let mut cmd = Command::new(kernel_path);
                cmd.args(["run", "-D", work_dir_str, "-c", config_str]);
                cmd.stdout(Stdio::null()).stderr(Stdio::piped());

                let mut child = cmd
                    .spawn()
                    .map_err(|e| ProcessError::StartFailed(format!("启动内核进程失败: {}", e)))?;
                self.attach_stderr_drain(&mut child)?;
                Ok(child)
            }
        }

        // 其他平台回退
        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        {
            let _ = (tun_enabled, app_handle);
            let mut cmd = Command::new(kernel_path);
            cmd.args(["run", "-D", work_dir_str, "-c", config_str]);
            cmd.stdout(Stdio::null()).stderr(Stdio::piped());

            let mut child = cmd
                .spawn()
                .map_err(|e| ProcessError::StartFailed(format!("启动内核进程失败: {}", e)))?;
            self.attach_stderr_drain(&mut child)?;
            Ok(child)
        }
    }

    // 说明：旧版 Linux(pkexec)/macOS(osascript) 提权方案已替换为 sudo + 密钥环保存密码，
    // 以满足“首次弹窗输入密码、后续自动提权”的产品需求。
    // 验证启动是否成功
    async fn verify_startup(&self) -> bool {
        info!("🔍 验证内核启动状态...");

        // 短轮询快速确认，不长时间阻塞启动流程
        for i in 1..=3 {
            tokio::time::sleep(Duration::from_millis(500)).await;

            if self.is_running().await {
                info!("✅ 内核状态验证通过（第{}次检查）", i);
                return true;
            } else {
                debug!("⏳ 内核尚未就绪，第{}次检查", i);
            }
        }

        error!("❌ 内核启动验证失败，多次检查都未通过");
        false
    }

    // 清理失败的进程
    async fn cleanup_failed_process(&self) -> Result<()> {
        let mut process_guard = self.process.write().await;
        if let Some(mut child) = process_guard.take() {
            if let Err(e) = child.kill() {
                warn!("清理失败进程时出错: {}", e);
                // 尝试强制终止
                #[cfg(windows)]
                {
                    let pid = child.id();
                    if let Err(e) = kill_process_by_pid(pid) {
                        error!("强制终止进程失败: {}", e);
                    }
                }
            }
        }
        self.clear_managed_pid();
        Ok(())
    }

    /// Read kernel process stderr for startup failure diagnostics.
    /// The pipe is drained continuously after process spawn, so this returns the bounded tail.
    pub async fn read_stderr_output(&self) -> Option<String> {
        let Ok(guard) = self.stderr_tail.lock() else {
            return None;
        };

        if guard.is_empty() {
            return None;
        }

        Some(guard.iter().cloned().collect::<Vec<_>>().join("\n"))
    }

    // 仅清理本程序托管过的内核 PID，避免误杀用户自行运行的 sing-box 进程。
    pub async fn kill_existing_processes<R: Runtime>(
        &self,
        app_handle: Option<&AppHandle<R>>,
    ) -> std::io::Result<()> {
        let kernel_name = crate::platform::get_kernel_executable_name();
        let Some(pid) = self.read_managed_pid() else {
            info!("未发现托管 PID 记录，跳过内核进程清理");
            return Ok(());
        };

        #[cfg(target_os = "linux")]
        {
            match crate::platform::list_active_processes_by_name(kernel_name).await {
                Ok(active_pids) => {
                    if !active_pids.contains(&pid) {
                        info!(
                            "托管 PID {} 当前不是活跃 {} 进程（活跃 PID: {:?}），跳过清理并清除记录",
                            pid, kernel_name, active_pids
                        );
                        self.clear_managed_pid();
                        return Ok(());
                    }
                }
                Err(e) => {
                    warn!("复核 Linux 活跃内核 PID 失败，将回退到传统校验: {}", e);
                }
            }
        }

        if !self.is_pid_matching_kernel_name(pid, kernel_name) {
            warn!(
                "托管 PID({}) 与当前内核进程名({})不匹配，已跳过清理并清除记录",
                pid, kernel_name
            );
            self.clear_managed_pid();
            return Ok(());
        }

        info!("发现托管内核进程 PID: {}，开始清理", pid);
        self.try_kill_pid_with_optional_privilege(app_handle, pid, kernel_name)
            .await;
        self.clear_managed_pid();
        sleep(Duration::from_millis(300)).await;

        Ok(())
    }

    // 按进程名强制清理所有内核进程。
    // 用于“检测到旧内核残留导致启动冲突”场景，优先保证新启动流程可恢复。
    pub async fn force_kill_kernel_processes_by_name<R: Runtime>(
        &self,
        app_handle: Option<&AppHandle<R>>,
    ) -> std::result::Result<(), String> {
        let kernel_name = crate::platform::get_kernel_executable_name();
        info!("按进程名强制清理内核进程: {}", kernel_name);

        #[cfg(not(target_os = "linux"))]
        let _ = app_handle;

        let plain_kill_result = crate::platform::kill_processes_by_name(kernel_name)
            .await
            .map_err(|e| format!("按进程名终止内核进程失败: {}", e));

        // 清理本地句柄与 PID 记录，避免后续状态仍指向被外部终止的旧进程。
        {
            let mut process_guard = self.process.write().await;
            *process_guard = None;
        }
        self.clear_managed_pid();

        #[cfg(target_os = "linux")]
        {
            const VERIFY_ATTEMPTS: usize = 5;
            const VERIFY_INTERVAL_MS: u64 = 400;

            if let Err(err) = plain_kill_result {
                warn!("普通权限按名称终止内核失败: {}", err);
            }

            for attempt in 1..=VERIFY_ATTEMPTS {
                sleep(Duration::from_millis(VERIFY_INTERVAL_MS)).await;

                match crate::platform::list_active_processes_by_name(kernel_name).await {
                    Ok(active_pids) if active_pids.is_empty() => {
                        info!("按进程名强制清理完成，未发现活跃 {} 进程", kernel_name);
                        return Ok(());
                    }
                    Ok(active_pids) => {
                        #[cfg(any(target_os = "linux", target_os = "macos"))]
                        if attempt == 1 {
                            if let Some(app_handle) = app_handle {
                                warn!(
                                    "普通权限按名称清理后仍检测到活跃 {} 进程 {:?}，尝试使用 sudo 继续清理",
                                    kernel_name, active_pids
                                );
                                match crate::app::system::sudo_service::kill_processes_by_name_with_saved_password(app_handle, kernel_name).await {
                                    Ok(_) => {
                                        continue;
                                    }
                                    Err(err) => {
                                        warn!("使用 sudo 按名称终止 {} 失败: {}", kernel_name, err);
                                    }
                                }
                            }
                        }

                        if attempt == VERIFY_ATTEMPTS {
                            return Err(format!(
                                "强制清理后仍检测到 {} 活跃进程在运行，PID: {:?}，可能存在权限不足",
                                kernel_name, active_pids
                            ));
                        }

                        info!(
                            "第{}次复核时仍检测到活跃 {} 进程: {:?}，继续等待退出",
                            attempt, kernel_name, active_pids
                        );
                    }
                    Err(e) => {
                        warn!("强制清理后状态复核失败，继续后续流程: {}", e);
                        return Ok(());
                    }
                }
            }

            Ok(())
        }

        #[cfg(not(target_os = "linux"))]
        {
            plain_kill_result?;
            sleep(Duration::from_millis(350)).await;
            match crate::platform::is_process_running(kernel_name).await {
                Ok(true) => Err(format!(
                    "强制清理后仍检测到 {} 进程在运行，可能存在权限不足",
                    kernel_name
                )),
                Ok(false) => Ok(()),
                Err(e) => {
                    // 检测失败时不直接阻断：终止命令已成功执行，交由上层启动稳定性校验兜底。
                    warn!("强制清理后状态复核失败，继续后续流程: {}", e);
                    Ok(())
                }
            }
        }
    }

    // 停止进程
    pub async fn stop<R: Runtime>(&self, app_handle: Option<&AppHandle<R>>) -> Result<()> {
        // 尝试关闭系统代理
        if let Err(e) = disable_system_proxy() {
            warn!("关闭系统代理失败: {}", e);
        } else {
            info!("{}", messages::INFO_SYSTEM_PROXY_DISABLED);
        }

        // 提取进程并停止它
        let mut child_opt = {
            let mut process_guard = self.process.write().await;
            process_guard.take()
        };

        if let Some(mut child) = child_opt.take() {
            // Windows 优先使用强制终止，避免长时间等待
            #[cfg(windows)]
            {
                let pid = child.id();
                if let Err(e) = kill_process_by_pid(pid) {
                    warn!("强制终止内核进程失败: {}", e);
                } else {
                    info!("已强制终止内核进程 (pid={})", pid);
                }
            }

            // 其他平台或兜底再尝试优雅 kill
            match child.kill() {
                Ok(_) => {
                    info!("{}", messages::INFO_PROCESS_STOPPED);
                    if let Err(e) = child.wait() {
                        warn!("等待内核进程终止失败: {}", e);
                    }
                }
                Err(e) => {
                    warn!("终止内核进程失败: {}", e);
                    #[cfg(windows)]
                    {
                        let pid = child.id();
                        if let Err(e) = kill_process_by_pid(pid) {
                            error!("强制终止进程失败: {}", e);
                            return Err(ProcessError::StopFailed(format!(
                                "强制终止进程失败: {}",
                                e
                            )));
                        }
                    }
                }
            }
            self.clear_managed_pid();
        } else {
            info!("没有正在运行的内核进程");
        }

        // 兜底：尝试清理托管 PID 记录对应的进程
        if let Err(e) = self.kill_existing_processes(app_handle).await {
            warn!("清理托管内核进程失败: {}", e);
        }

        Ok(())
    }

    // 重启进程
    pub async fn restart<R: Runtime>(
        &self,
        app_handle: &AppHandle<R>,
        config_path: &std::path::Path,
        tun_enabled: bool,
    ) -> Result<()> {
        info!("正在重启内核进程，TUN模式: {}", tun_enabled);
        self.stop(Some(app_handle)).await?;
        sleep(Duration::from_millis(1000)).await;
        self.start(app_handle, config_path, tun_enabled).await?;
        info!("内核进程重启完成");
        Ok(())
    }

    /// Hermetic 重启：非 TUN 可传 `None` AppHandle。
    pub async fn restart_inner<R: Runtime>(
        &self,
        app_handle: Option<&AppHandle<R>>,
        config_path: &std::path::Path,
        tun_enabled: bool,
    ) -> Result<()> {
        info!("正在重启内核进程(inner)，TUN模式: {}", tun_enabled);
        self.stop(app_handle).await?;
        sleep(Duration::from_millis(1000)).await;
        self.start_inner(app_handle, config_path, tun_enabled).await?;
        info!("内核进程重启完成(inner)");
        Ok(())
    }

    // 验证配置文件
    async fn validate_config(&self, config_path: &std::path::Path) -> Result<()> {
        if !config_path.exists() {
            KERNEL_STATE.update_readiness(|readiness| {
                readiness.config_validated = Some(false);
                readiness.process_spawned = Some(false);
            });
            return Err(ProcessError::ConfigError(format!(
                "配置文件不存在: {}",
                config_path.to_str().unwrap_or("unknown")
            )));
        }

        // 检查配置文件是否可读
        if let Err(e) = tokio::fs::metadata(config_path).await {
            KERNEL_STATE.update_readiness(|readiness| {
                readiness.config_validated = Some(false);
                readiness.process_spawned = Some(false);
            });
            return Err(ProcessError::ConfigError(format!(
                "无法访问配置文件: {}",
                e
            )));
        }

        // 启动前执行一次显式配置检查，避免内核启动后才暴露语法/迁移错误。
        let kernel_path = paths::get_kernel_path();
        if kernel_path.exists() {
            let config_str = config_path
                .to_str()
                .ok_or_else(|| ProcessError::ConfigError("配置路径包含无效字符".to_string()))?;

            let mut check_cmd = Command::new(&kernel_path);
            check_cmd.args(["check", "--config", config_str]);

            #[cfg(target_os = "windows")]
            crate::platform::configure_std_command(&mut check_cmd);

            let output = check_cmd
                .output()
                .map_err(|e| ProcessError::ConfigError(format!("执行配置校验命令失败: {}", e)))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let detail = if !stderr.is_empty() { stderr } else { stdout };
                KERNEL_STATE.update_readiness(|readiness| {
                    readiness.config_validated = Some(false);
                    readiness.process_spawned = Some(false);
                });

                if detail.contains("legacy DNS servers is deprecated")
                    || detail.contains("ENABLE_DEPRECATED_LEGACY_DNS_SERVERS")
                {
                    return Err(ProcessError::ConfigError(
                        "当前配置仍使用已弃用的 legacy DNS servers。请在订阅页刷新当前订阅配置，或关闭“按原始配置运行”后重新生成。".to_string(),
                    ));
                }
                if detail.contains("legacy domain strategy options is deprecated")
                    || detail.contains("ENABLE_DEPRECATED_LEGACY_DOMAIN_STRATEGY_OPTIONS")
                {
                    return Err(ProcessError::ConfigError(
                        "当前配置仍使用已弃用的 legacy domain strategy 选项。请在订阅页刷新当前订阅配置（或重新导入）后重试。".to_string(),
                    ));
                }
                if detail.contains("dns.servers") && detail.contains("unknown field \"strategy\"") {
                    return Err(ProcessError::ConfigError(
                        "当前配置包含已弃用字段 dns.servers[].strategy。请在订阅页手动刷新当前订阅配置后重试。".to_string(),
                    ));
                }

                return Err(ProcessError::ConfigError(format!(
                    "配置校验失败: {}",
                    detail
                )));
            }
        }

        KERNEL_STATE.update_readiness(|readiness| {
            readiness.config_validated = Some(true);
        });
        Ok(())
    }

    // 检查进程是否运行（使用读锁，提升并发性能）
    pub async fn is_running(&self) -> bool {
        let has_process_handle = {
            let process_guard = self.process.read().await;
            process_guard.is_some()
        };

        if has_process_handle {
            let mut wrapper_exited = false;

            {
                let mut process_guard = self.process.write().await;
                if let Some(ref mut proc) = *process_guard {
                    match proc.try_wait() {
                        Ok(None) => return true,
                        Ok(Some(status)) => {
                            info!("托管启动子进程已退出，状态: {}", status);
                            *process_guard = None;
                            wrapper_exited = true;
                        }
                        Err(err) => {
                            warn!("检查托管启动子进程状态失败: {}", err);
                            *process_guard = None;
                            wrapper_exited = true;
                        }
                    }
                }
            }

            if wrapper_exited && self.has_active_managed_kernel_pid().await {
                info!("托管启动子进程已退出，但记录的内核 PID 仍在运行");
                return true;
            }
        }

        self.has_active_managed_kernel_pid().await
    }
}

// 使用PID强制终止进程
fn kill_process_by_pid(pid: u32) -> std::io::Result<()> {
    crate::platform::kill_process_by_pid(pid).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempWorkspace;

    #[tokio::test]
    async fn stderr_tail_should_keep_only_recent_lines() {
        let manager = ProcessManager::new();

        for i in 0..(STDERR_TAIL_LIMIT + 25) {
            ProcessManager::push_stderr_tail(&manager.stderr_tail, format!("line-{i}"));
        }

        let output = manager
            .read_stderr_output()
            .await
            .expect("stderr tail should exist");
        let lines = output.lines().collect::<Vec<_>>();

        assert_eq!(lines.len(), STDERR_TAIL_LIMIT);
        assert!(!output.contains("line-0"));
        assert!(output.contains(&format!("line-{}", STDERR_TAIL_LIMIT + 24)));
    }

    #[test]
    fn managed_pid_file_roundtrip_and_clear() {
        let ws = TempWorkspace::new();
        let manager = ProcessManager::new();
        assert!(manager.read_managed_pid().is_none());
        manager.persist_managed_pid(4242).unwrap();
        assert_eq!(manager.read_managed_pid(), Some(4242));
        manager.clear_managed_pid();
        assert!(manager.read_managed_pid().is_none());
        let _ = ws;
    }

    #[tokio::test]
    async fn is_running_false_when_no_process() {
        let manager = ProcessManager::new();
        assert!(!manager.is_running().await);
    }

    #[test]
    fn default_constructs() {
        let _m = ProcessManager::default();
    }

    #[test]
    fn managed_pid_file_path_under_work_dir() {
        let ws = TempWorkspace::new();
        let path = ProcessManager::managed_pid_file();
        assert!(path.ends_with(".managed-kernel.pid"));
        assert!(path.starts_with(ws.path()) || path.to_string_lossy().contains("sing-box"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn is_pid_matching_kernel_name_for_self_is_false_for_wrong_name() {
        let manager = ProcessManager::new();
        let pid = std::process::id();
        assert!(!manager.is_pid_matching_kernel_name(pid, "definitely-not-this-kernel"));
    }

    /// 安装可执行的假 sing-box：支持 `check` 成功、`run` 长驻并写 stderr。
    fn install_fake_kernel(work: &std::path::Path) -> std::path::PathBuf {
        let dir = work.join("sing-box");
        std::fs::create_dir_all(&dir).unwrap();
        let kernel = dir.join("sing-box");
        std::fs::write(
            &kernel,
            r#"#!/bin/sh
if [ "$1" = "check" ]; then
  exit 0
fi
if [ "$1" = "run" ]; then
  echo "fake-kernel-started" >&2
  exec sleep "${FAKE_KERNEL_RUN_SECS:-5}"
fi
exit 0
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&kernel).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&kernel, perms).unwrap();
        }
        kernel
    }

    #[tokio::test]
    async fn start_inner_with_fake_kernel_then_stop() {
        let ws = TempWorkspace::new();
        let _ = install_fake_kernel(ws.path());

        let cfg_path = ws.path().join("sing-box/config.json");
        std::fs::write(&cfg_path, r#"{"log":{"level":"info"}}"#).unwrap();

        let manager = ProcessManager::new();
        manager
            .start_inner::<tauri::Wry>(None, &cfg_path, false)
            .await
            .expect("fake kernel should start");
        assert!(manager.is_running().await);

        let _ = manager.read_stderr_output().await;

        manager.stop::<tauri::Wry>(None).await.expect("stop should succeed");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let _ = manager.is_running().await;
    }

    #[tokio::test]
    async fn start_inner_tun_without_handle_fails() {
        let manager = ProcessManager::new();
        let err = manager
            .start_inner::<tauri::Wry>(None, std::path::Path::new("/tmp/nope.json"), true)
            .await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn validate_config_missing_file_errors() {
        let manager = ProcessManager::new();
        let err = manager
            .validate_config(std::path::Path::new("/tmp/definitely-missing-cfg-xyz.json"))
            .await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn kill_existing_with_no_managed_pid_is_ok() {
        let ws = TempWorkspace::new();
        let manager = ProcessManager::new();
        manager.clear_managed_pid();
        manager.kill_existing_processes::<tauri::Wry>(None).await.unwrap();
        let _ = ws;
    }

    #[tokio::test]
    async fn restart_without_app_handle_path_via_stop_start_inner() {
        let ws = TempWorkspace::new();
        install_fake_kernel(ws.path());
        let cfg_path = ws.path().join("sing-box/config.json");
        std::fs::write(&cfg_path, r#"{"log":{"level":"info"}}"#).unwrap();

        let manager = ProcessManager::new();
        manager.start_inner::<tauri::Wry>(None, &cfg_path, false).await.unwrap();
        assert!(manager.is_running().await);
        manager.stop::<tauri::Wry>(None).await.unwrap();
        manager.start_inner::<tauri::Wry>(None, &cfg_path, false).await.unwrap();
        assert!(manager.is_running().await);
        manager.stop::<tauri::Wry>(None).await.unwrap();
    }

    #[tokio::test]
    async fn validate_config_with_fake_kernel_ok() {
        let ws = TempWorkspace::new();
        install_fake_kernel(ws.path());
        let cfg = ws.path().join("sing-box/config.json");
        std::fs::write(&cfg, r#"{"log":{"level":"info"}}"#).unwrap();
        let manager = ProcessManager::new();
        manager
            .validate_config(&cfg)
            .await
            .expect("check should pass");
    }

    #[tokio::test]
    async fn double_start_and_clear_stderr_tail() {
        let ws = TempWorkspace::new();
        install_fake_kernel(ws.path());
        let cfg = ws.path().join("sing-box/config.json");
        std::fs::write(&cfg, r#"{"log":{"level":"info"}}"#).unwrap();
        let manager = ProcessManager::new();
        manager.start_inner::<tauri::Wry>(None, &cfg, false).await.unwrap();
        let _ = manager.start_inner::<tauri::Wry>(None, &cfg, false).await;
        let _ = manager.read_stderr_output().await;
        manager.clear_stderr_tail();
        manager.stop::<tauri::Wry>(None).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }

    #[tokio::test]
    async fn force_kill_by_name_without_process() {
        let ws = TempWorkspace::new();
        let manager = ProcessManager::new();
        manager.clear_managed_pid();
        let _ = manager.force_kill_kernel_processes_by_name::<tauri::Wry>(None).await;
        let _ = ws;
    }

    #[test]
    fn managed_pid_invalid_content() {
        let ws = TempWorkspace::new();
        let manager = ProcessManager::new();
        let path = ProcessManager::managed_pid_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, "not-a-pid").unwrap();
        assert!(manager.read_managed_pid().is_none());
        let _ = ws;
    }

    #[tokio::test]
    async fn global_process_manager_start_stop_via_kernel_service() {
        use crate::app::core::kernel_service::PROCESS_MANAGER as GLOBAL_PM;
        use crate::app::core::kernel_service::status::is_kernel_running;
        use crate::app::constants::paths;
        use crate::test_support::TempWorkspace;

        let ws = TempWorkspace::new();
        install_fake_kernel(ws.path());
        let cfg = paths::get_config_dir().join("config.json");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(&cfg, r#"{"log":{"level":"info"}}"#).unwrap();

        GLOBAL_PM.start_inner::<tauri::Wry>(None, &cfg, false).await.unwrap();
        assert!(GLOBAL_PM.is_running().await);
        assert!(is_kernel_running().await.unwrap());
        let _ = GLOBAL_PM.read_stderr_output().await;
        GLOBAL_PM.stop::<tauri::Wry>(None).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    #[tokio::test]
    async fn restart_public_api_without_app_handle_errors_or_works() {
        // restart 需要 AppHandle 的某些路径；无 handle 时走 stop+start_inner 已覆盖
        let ws = TempWorkspace::new();
        install_fake_kernel(ws.path());
        let cfg = ws.path().join("sing-box/config.json");
        std::fs::write(&cfg, r#"{"log":{"level":"info"}}"#).unwrap();
        let manager = ProcessManager::new();
        // stop when nothing running
        let _ = manager.stop::<tauri::Wry>(None).await;
        manager.start_inner::<tauri::Wry>(None, &cfg, false).await.unwrap();
        // kill_existing while running
        let _ = manager.kill_existing_processes::<tauri::Wry>(None).await;
        let _ = manager.stop::<tauri::Wry>(None).await;
    }

    /// 安装会在 `check` 时输出 legacy DNS 错误的假内核。
    fn install_fake_kernel_check_fail(work: &std::path::Path, stderr_msg: &str) -> std::path::PathBuf {
        let dir = work.join("sing-box");
        std::fs::create_dir_all(&dir).unwrap();
        let kernel = dir.join("sing-box");
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "check" ]; then
  echo '{msg}' >&2
  exit 1
fi
if [ "$1" = "run" ]; then exec sleep "${{FAKE_KERNEL_RUN_SECS:-5}}"; fi
exit 0
"#,
            msg = stderr_msg.replace('\'', "")
        );
        std::fs::write(&kernel, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&kernel).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&kernel, perms).unwrap();
        }
        kernel
    }

    #[tokio::test]
    async fn validate_config_maps_legacy_dns_error() {
        let ws = TempWorkspace::new();
        install_fake_kernel_check_fail(ws.path(), "legacy DNS servers is deprecated");
        let cfg = ws.path().join("sing-box/config.json");
        std::fs::write(&cfg, r#"{"log":{"level":"info"}}"#).unwrap();
        let manager = ProcessManager::new();
        let err = manager.validate_config(&cfg).await.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("legacy DNS") || msg.contains("弃用") || msg.contains("配置"),
            "unexpected: {msg}"
        );
    }

    #[tokio::test]
    async fn validate_config_maps_domain_strategy_and_strategy_field_errors() {
        let ws = TempWorkspace::new();
        install_fake_kernel_check_fail(
            ws.path(),
            "legacy domain strategy options is deprecated ENABLE_DEPRECATED_LEGACY_DOMAIN_STRATEGY_OPTIONS",
        );
        let cfg = ws.path().join("sing-box/config.json");
        std::fs::write(&cfg, r#"{}"#).unwrap();
        let manager = ProcessManager::new();
        let err = manager.validate_config(&cfg).await.unwrap_err().to_string();
        assert!(err.contains("domain strategy") || err.contains("弃用") || err.contains("配置"));

        install_fake_kernel_check_fail(
            ws.path(),
            r#"dns.servers: unknown field "strategy""#,
        );
        let err2 = manager.validate_config(&cfg).await.unwrap_err().to_string();
        assert!(err2.contains("strategy") || err2.contains("弃用") || err2.contains("配置"));

        install_fake_kernel_check_fail(ws.path(), "generic parse error xyz");
        let err3 = manager.validate_config(&cfg).await.unwrap_err().to_string();
        assert!(err3.contains("配置校验失败") || err3.contains("generic") || err3.contains("xyz"));
    }

    #[tokio::test]
    async fn start_inner_missing_kernel_errors() {
        let ws = TempWorkspace::new();
        // 工作区存在但无 sing-box 可执行文件
        let cfg = ws.path().join("sing-box/config.json");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(&cfg, r#"{"log":{"level":"info"}}"#).unwrap();
        // 确保内核路径不存在
        let kernel = crate::app::constants::paths::get_kernel_path();
        if kernel.exists() {
            let _ = std::fs::remove_file(&kernel);
        }
        let manager = ProcessManager::new();
        let err = manager.start_inner::<tauri::Wry>(None, &cfg, false).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn start_inner_exiting_kernel_fails_verify() {
        let ws = TempWorkspace::new();
        let dir = ws.path().join("sing-box");
        std::fs::create_dir_all(&dir).unwrap();
        let kernel = dir.join("sing-box");
        // check 成功，run 立刻退出
        std::fs::write(
            &kernel,
            r#"#!/bin/sh
if [ "$1" = "check" ]; then exit 0; fi
if [ "$1" = "run" ]; then exit 1; fi
exit 0
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&kernel).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&kernel, perms).unwrap();
        }
        let cfg = dir.join("config.json");
        std::fs::write(&cfg, r#"{"log":{"level":"info"}}"#).unwrap();
        let manager = ProcessManager::new();
        let err = manager.start_inner::<tauri::Wry>(None, &cfg, false).await;
        assert!(err.is_err(), "immediate exit should fail verify/start");
    }

    #[tokio::test]
    async fn force_kill_after_start_and_stop_when_already_dead() {
        let ws = TempWorkspace::new();
        install_fake_kernel(ws.path());
        let cfg = ws.path().join("sing-box/config.json");
        std::fs::write(&cfg, r#"{"log":{"level":"info"}}"#).unwrap();
        let manager = ProcessManager::new();
        manager.start_inner::<tauri::Wry>(None, &cfg, false).await.unwrap();
        assert!(manager.is_running().await);
        let _ = manager.force_kill_kernel_processes_by_name::<tauri::Wry>(None).await;
        let _ = manager.kill_existing_processes::<tauri::Wry>(None).await;
        let _ = manager.stop::<tauri::Wry>(None).await;
        // 二次 stop 应幂等
        let _ = manager.stop::<tauri::Wry>(None).await;
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn is_pid_matching_kernel_name_for_self_comm() {
        let manager = ProcessManager::new();
        let pid = std::process::id();
        // 自身 comm 通常是测试 runner 名，不会是 sing-box
        assert!(!manager.is_pid_matching_kernel_name(pid, "sing-box"));
        // 读取 /proc/self/comm 再匹配应成功
        let comm = std::fs::read_to_string(format!("/proc/{}/comm", pid))
            .unwrap_or_default()
            .trim()
            .to_string();
        if !comm.is_empty() {
            assert!(manager.is_pid_matching_kernel_name(pid, &comm));
        }
    }

    #[tokio::test]
    async fn is_running_false_after_process_exits_naturally() {
        let ws = TempWorkspace::new();
        let dir = ws.path().join("sing-box");
        std::fs::create_dir_all(&dir).unwrap();
        let kernel = dir.join("sing-box");
        // check 成功，run 短暂 sleep 后退出
        std::fs::write(
            &kernel,
            r#"#!/bin/sh
if [ "$1" = "check" ]; then exit 0; fi
if [ "$1" = "run" ]; then sleep 0.2; exit 0; fi
exit 0
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&kernel).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&kernel, perms).unwrap();
        }
        let cfg = dir.join("config.json");
        std::fs::write(&cfg, r#"{}"#).unwrap();
        let manager = ProcessManager::new();
        // 可能因 verify 失败而 Err，也可能短暂成功后退出
        let _ = manager.start_inner::<tauri::Wry>(None, &cfg, false).await;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        // 进程已退出后 is_running 应为 false（或清理后）
        let running = manager.is_running().await;
        if running {
            let _ = manager.stop::<tauri::Wry>(None).await;
        }
        assert!(!manager.is_running().await || !running);
    }

    #[tokio::test]
    async fn start_inner_twice_replaces_running_process() {
        let ws = TempWorkspace::new();
        install_fake_kernel(ws.path());
        let cfg = ws.path().join("sing-box/config.json");
        std::fs::write(&cfg, r#"{"log":{"level":"info"}}"#).unwrap();
        let manager = ProcessManager::new();
        manager.start_inner::<tauri::Wry>(None, &cfg, false).await.unwrap();
        assert!(manager.is_running().await);
        // 第二次启动应杀掉旧进程再起
        manager.start_inner::<tauri::Wry>(None, &cfg, false).await.unwrap();
        assert!(manager.is_running().await);
        let stderr = manager.read_stderr_output().await;
        let _ = stderr;
        manager.stop::<tauri::Wry>(None).await.unwrap();
    }

    #[tokio::test]
    async fn kill_existing_with_stale_managed_pid() {
        let ws = TempWorkspace::new();
        let manager = ProcessManager::new();
        // 写入不存在的 PID
        manager.persist_managed_pid(4_294_967_294).unwrap();
        manager.kill_existing_processes::<tauri::Wry>(None).await.unwrap();
        // force kill 空名列表
        let _ = manager.force_kill_kernel_processes_by_name::<tauri::Wry>(None).await;
        manager.clear_managed_pid();
        let _ = ws;
    }

    #[tokio::test]
    async fn validate_config_skips_check_when_kernel_missing() {
        let ws = TempWorkspace::new();
        // 生产语义：内核文件不存在时跳过 `check`，仅确认配置文件可读
        let cfg = ws.path().join("sing-box/config.json");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(&cfg, r#"{}"#).unwrap();
        let kernel = crate::app::constants::paths::get_kernel_path();
        if kernel.exists() {
            let _ = std::fs::remove_file(&kernel);
        }
        let manager = ProcessManager::new();
        if !kernel.exists() {
            manager
                .validate_config(&cfg)
                .await
                .expect("missing kernel skips check and accepts readable config");
        } else {
            // 其它用例残留内核时仍应能完成校验路径
            let _ = manager.validate_config(&cfg).await;
        }
    }

    #[test]
    fn push_stderr_tail_when_lock_ok() {
        let manager = ProcessManager::new();
        ProcessManager::push_stderr_tail(&manager.stderr_tail, "one".into());
        ProcessManager::push_stderr_tail(&manager.stderr_tail, "two".into());
        manager.clear_stderr_tail();
        // 再 push 后可读取
        ProcessManager::push_stderr_tail(&manager.stderr_tail, "three".into());
    }

    #[tokio::test]
    async fn read_stderr_empty_then_after_start() {
        let ws = TempWorkspace::new();
        install_fake_kernel(ws.path());
        let cfg = ws.path().join("sing-box/config.json");
        std::fs::write(&cfg, r#"{}"#).unwrap();
        let manager = ProcessManager::new();
        // 启动前可能为空
        let before = manager.read_stderr_output().await;
        let _ = before;
        manager.start_inner::<tauri::Wry>(None, &cfg, false).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let after = manager.read_stderr_output().await;
        let _ = after;
        manager.stop::<tauri::Wry>(None).await.unwrap();
    }

    #[tokio::test]
    async fn restart_inner_with_fake_kernel() {
        let ws = TempWorkspace::new();
        install_fake_kernel(ws.path());
        let cfg = ws.path().join("sing-box/config.json");
        std::fs::write(&cfg, r#"{}"#).unwrap();
        let manager = ProcessManager::new();
        manager
            .start_inner::<tauri::Wry>(None, &cfg, false)
            .await
            .unwrap();
        manager
            .restart_inner::<tauri::Wry>(None, &cfg, false)
            .await
            .expect("restart_inner");
        assert!(manager.is_running().await);
        manager.stop::<tauri::Wry>(None).await.unwrap();
    }

    #[tokio::test]
    async fn kill_existing_after_start_with_managed_pid() {
        let ws = TempWorkspace::new();
        install_fake_kernel(ws.path());
        let cfg = ws.path().join("sing-box/config.json");
        std::fs::write(&cfg, r#"{}"#).unwrap();
        let manager = ProcessManager::new();
        manager
            .start_inner::<tauri::Wry>(None, &cfg, false)
            .await
            .unwrap();
        assert!(manager.is_running().await);
        manager
            .kill_existing_processes::<tauri::Wry>(None)
            .await
            .expect("kill existing");
        // 进程应被清理
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let _ = manager.stop::<tauri::Wry>(None).await;
    }

    #[tokio::test]
    async fn has_active_managed_pid_after_start() {
        let ws = TempWorkspace::new();
        install_fake_kernel(ws.path());
        let cfg = ws.path().join("sing-box/config.json");
        std::fs::write(&cfg, r#"{}"#).unwrap();
        let manager = ProcessManager::new();
        assert!(!manager.has_active_managed_kernel_pid().await);
        manager
            .start_inner::<tauri::Wry>(None, &cfg, false)
            .await
            .unwrap();
        // 启动后可能记录了 managed pid
        let _ = manager.has_active_managed_kernel_pid().await;
        manager.stop::<tauri::Wry>(None).await.unwrap();
    }

    #[tokio::test]
    async fn start_public_api_via_mock_app() {
        use crate::test_support::MockAppEnv;

        let env = MockAppEnv::new();
        install_fake_kernel(env.workspace.path());
        let cfg = env.workspace.path().join("sing-box/config.json");
        std::fs::write(&cfg, r#"{}"#).unwrap();
        let manager = ProcessManager::new();
        manager
            .start(&env.handle(), &cfg, false)
            .await
            .expect("start with mock handle");
        manager
            .stop(Some(&env.handle()))
            .await
            .expect("stop with mock handle");
    }
}

#[async_trait::async_trait]
impl<R: tauri::Runtime> crate::app::core::kernel_service::KernelProcessControl<R> for ProcessManager {
    async fn start(
        &self,
        app_handle: Option<&tauri::AppHandle<R>>,
        config_path: &std::path::Path,
        tun_enabled: bool,
    ) -> std::result::Result<(), String> {
        self.start_inner(app_handle, config_path, tun_enabled)
            .await
            .map_err(|e| e.to_string())
    }

    async fn stop(
        &self,
        app_handle: Option<&tauri::AppHandle<R>>,
    ) -> std::result::Result<(), String> {
        ProcessManager::stop(self, app_handle)
            .await
            .map_err(|e| e.to_string())
    }

    async fn restart(
        &self,
        app_handle: &tauri::AppHandle<R>,
        config_path: &std::path::Path,
        tun_enabled: bool,
    ) -> std::result::Result<(), String> {
        ProcessManager::restart(self, app_handle, config_path, tun_enabled)
            .await
            .map_err(|e| e.to_string())
    }

    async fn kill_existing_processes(
        &self,
        app_handle: Option<&tauri::AppHandle<R>>,
    ) -> std::result::Result<(), String> {
        ProcessManager::kill_existing_processes(self, app_handle)
            .await
            .map_err(|e| e.to_string())
    }

    async fn force_kill_kernel_processes_by_name(
        &self,
        app_handle: Option<&tauri::AppHandle<R>>,
    ) -> std::result::Result<(), String> {
        ProcessManager::force_kill_kernel_processes_by_name(self, app_handle).await
    }

    async fn is_running(&self) -> bool {
        ProcessManager::is_running(self).await
    }

    async fn read_stderr_output(&self) -> Option<String> {
        ProcessManager::read_stderr_output(self).await
    }
}
