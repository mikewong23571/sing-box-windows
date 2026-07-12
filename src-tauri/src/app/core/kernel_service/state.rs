//! 内核运行时配置与状态类型定义
//!
//! 提供期望状态、观测状态、转换规划与启动诊断类型。

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU16, AtomicU8, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// 内核运行状态枚举
///
/// 使用状态机模式管理内核生命周期，确保状态转换的一致性。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum KernelState {
    /// 内核已停止
    #[default]
    Stopped = 0,
    /// 内核正在启动
    Starting = 1,
    /// 内核运行中
    Running = 2,
    /// 内核正在停止
    Stopping = 3,
    /// 内核启动失败
    Failed = 4,
    /// 内核意外崩溃（由守护进程检测）
    Crashed = 5,
}

/// 本次应用会话中用户期望的内核状态。
///
/// 该状态不持久化：应用启动时由 `auto_start_kernel` 初始化，之后只允许显式
/// start/stop 和 shutdown 修改。配置、订阅与内核文件更新必须保持该状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum KernelDesiredState {
    Running = 1,
    #[default]
    Stopped = 0,
}

impl From<u8> for KernelDesiredState {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::Running,
            _ => Self::Stopped,
        }
    }
}

impl KernelDesiredState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
        }
    }
}

/// 由进程状态与 readiness 合成的对外观测状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum KernelObservedState {
    #[default]
    Stopped,
    Starting,
    Running,
    Degraded,
    Stopping,
    Failed,
    Crashed,
}

impl KernelObservedState {
    pub fn from_runtime(state: KernelState, readiness: &KernelReadinessSnapshot) -> Self {
        if readiness.process_alive
            && !matches!(state, KernelState::Starting | KernelState::Stopping)
        {
            return if readiness.api_ready && readiness.relay_ready {
                Self::Running
            } else {
                Self::Degraded
            };
        }

        match state {
            KernelState::Starting => Self::Starting,
            KernelState::Stopping => Self::Stopping,
            KernelState::Failed => Self::Failed,
            KernelState::Crashed => Self::Crashed,
            KernelState::Running => Self::Stopped,
            KernelState::Stopped => Self::Stopped,
        }
    }

    pub fn is_process_active(self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Running | Self::Degraded | Self::Stopping
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum KernelChangeImpact {
    #[default]
    PersistOnly,
    HotApply,
    RestartIfRunning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelRequestKind {
    UserStart,
    UserStop,
    UserRestart,
    ApplyRuntimeChange(KernelChangeImpact),
    StartupReconcile { auto_start: bool },
    ProcessCrashed,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelAction {
    Noop,
    Start,
    Stop,
    Restart,
    HotApply,
    ApplyConfigOnly,
    Reject,
}

/// 与一次串行生命周期操作关联的元数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelOperationMeta {
    pub op_id: String,
    pub operation: String,
    pub state_version: u64,
}

/// 前后端共享的内核生命周期快照。
///
/// `process_running`、`api_ready` 与 `websocket_ready` 为迁移期间保留的兼容字段；
/// 新调用方应优先使用 desired/observed state 和 readiness。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelLifecycleSnapshot {
    pub desired_state: KernelDesiredState,
    pub observed_state: KernelObservedState,
    pub process_running: bool,
    pub api_ready: bool,
    pub websocket_ready: bool,
    pub readiness: KernelReadinessSnapshot,
    pub startup_diagnosis: Option<StartupDiagnosis>,
    pub kernel_state: KernelState,
    pub state_version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_meta: Option<KernelOperationMeta>,
}

/// 无 IO 的生命周期规划器。所有生产入口最终都应遵循该转换语义。
pub fn plan_kernel_transition(
    desired: KernelDesiredState,
    observed: KernelObservedState,
    request: KernelRequestKind,
) -> KernelAction {
    use KernelAction::*;

    let running = matches!(
        observed,
        KernelObservedState::Running | KernelObservedState::Degraded
    );
    let active = observed.is_process_active();

    match request {
        KernelRequestKind::UserStart => {
            if running || matches!(observed, KernelObservedState::Starting) {
                Noop
            } else {
                Start
            }
        }
        KernelRequestKind::UserStop | KernelRequestKind::Shutdown => {
            if active {
                Stop
            } else {
                Noop
            }
        }
        KernelRequestKind::UserRestart => {
            if running {
                Restart
            } else {
                Reject
            }
        }
        KernelRequestKind::ApplyRuntimeChange(KernelChangeImpact::PersistOnly) => ApplyConfigOnly,
        KernelRequestKind::ApplyRuntimeChange(KernelChangeImpact::HotApply) => {
            if running {
                HotApply
            } else {
                ApplyConfigOnly
            }
        }
        KernelRequestKind::ApplyRuntimeChange(KernelChangeImpact::RestartIfRunning) => {
            if desired == KernelDesiredState::Running && running {
                Restart
            } else {
                ApplyConfigOnly
            }
        }
        KernelRequestKind::StartupReconcile { auto_start } => {
            if auto_start {
                if running {
                    Noop
                } else {
                    Start
                }
            } else if active {
                Stop
            } else {
                Noop
            }
        }
        KernelRequestKind::ProcessCrashed => {
            if desired == KernelDesiredState::Running {
                Start
            } else {
                Noop
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StartupStage {
    #[default]
    Preflight,
    Spawn,
    Readiness,
    Guard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupDiagnosisKind {
    ConfigInvalid,
    ConfigMissing,
    BinaryMissing,
    PermissionDenied,
    SudoRequired,
    SudoInvalid,
    PortConflict,
    ProcessExitedEarly,
    ApiHttpError,
    ApiTimeout,
    ConflictCleanupFailed,
    GuardRestartFailed,
    Unknown,
}

impl StartupDiagnosisKind {
    pub fn priority(&self) -> u8 {
        match self {
            StartupDiagnosisKind::ConfigMissing => 100,
            StartupDiagnosisKind::ConfigInvalid => 90,
            StartupDiagnosisKind::BinaryMissing => 80,
            StartupDiagnosisKind::SudoRequired
            | StartupDiagnosisKind::SudoInvalid
            | StartupDiagnosisKind::PermissionDenied => 70,
            StartupDiagnosisKind::PortConflict | StartupDiagnosisKind::ConflictCleanupFailed => 60,
            StartupDiagnosisKind::ProcessExitedEarly => 50,
            StartupDiagnosisKind::ApiHttpError | StartupDiagnosisKind::ApiTimeout => 40,
            StartupDiagnosisKind::GuardRestartFailed => 30,
            StartupDiagnosisKind::Unknown => 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupDiagnosis {
    pub attempt_id: String,
    pub stage: StartupStage,
    pub code: String,
    pub kind: StartupDiagnosisKind,
    pub message: String,
    pub detail: String,
    pub source: String,
    pub recoverable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_actions: Option<Vec<String>>,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct KernelReadinessSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_validated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_spawned: Option<bool>,
    pub process_alive: bool,
    pub api_ready: bool,
    pub relay_ready: bool,
}

impl From<u8> for KernelState {
    fn from(v: u8) -> Self {
        match v {
            0 => KernelState::Stopped,
            1 => KernelState::Starting,
            2 => KernelState::Running,
            3 => KernelState::Stopping,
            4 => KernelState::Failed,
            5 => KernelState::Crashed,
            _ => KernelState::Stopped,
        }
    }
}

impl KernelState {
    /// 是否处于可启动状态
    pub fn can_start(&self) -> bool {
        matches!(
            self,
            KernelState::Stopped | KernelState::Failed | KernelState::Crashed
        )
    }

    /// 是否处于可停止状态
    pub fn can_stop(&self) -> bool {
        matches!(self, KernelState::Running | KernelState::Starting)
    }

    /// 是否正在运行
    pub fn is_running(&self) -> bool {
        matches!(self, KernelState::Running)
    }

    /// 是否处于过渡状态
    pub fn is_transitioning(&self) -> bool {
        matches!(self, KernelState::Starting | KernelState::Stopping)
    }

    /// 转字符串用于日志
    pub fn as_str(&self) -> &'static str {
        match self {
            KernelState::Stopped => "stopped",
            KernelState::Starting => "starting",
            KernelState::Running => "running",
            KernelState::Stopping => "stopping",
            KernelState::Failed => "failed",
            KernelState::Crashed => "crashed",
        }
    }
}

/// 全局内核状态管理器
///
/// 线程安全的状态追踪，供所有模块共享访问。
/// 使用无锁原子类型确保高性能和无死锁风险。
pub struct KernelStateManager {
    state: AtomicU8,
    desired_state: AtomicU8,
    api_port: AtomicU16,
    startup_diagnosis: RwLock<Option<StartupDiagnosis>>,
    readiness: RwLock<KernelReadinessSnapshot>,
    current_attempt_id: RwLock<Option<String>>,
}

impl KernelStateManager {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(KernelState::Stopped as u8),
            desired_state: AtomicU8::new(KernelDesiredState::Stopped as u8),
            api_port: AtomicU16::new(0),
            startup_diagnosis: RwLock::new(None),
            readiness: RwLock::new(KernelReadinessSnapshot::default()),
            current_attempt_id: RwLock::new(None),
        }
    }

    fn now_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    pub fn begin_attempt(&self, prefix: &str) -> String {
        let attempt_id = format!("{}-{}", prefix, Self::now_millis());
        if let Ok(mut guard) = self.current_attempt_id.write() {
            *guard = Some(attempt_id.clone());
        }
        if let Ok(mut diagnosis) = self.startup_diagnosis.write() {
            *diagnosis = None;
        }
        if let Ok(mut readiness) = self.readiness.write() {
            *readiness = KernelReadinessSnapshot::default();
        }
        attempt_id
    }

    pub fn ensure_attempt(&self, prefix: &str) -> String {
        if let Ok(guard) = self.current_attempt_id.read() {
            if let Some(existing) = guard.clone() {
                return existing;
            }
        }
        self.begin_attempt(prefix)
    }

    /// 获取当前状态
    pub fn get_state(&self) -> KernelState {
        KernelState::from(self.state.load(Ordering::SeqCst))
    }

    /// 设置状态
    pub fn set_state(&self, state: KernelState) {
        self.state.store(state as u8, Ordering::SeqCst);
    }

    pub fn get_desired_state(&self) -> KernelDesiredState {
        KernelDesiredState::from(self.desired_state.load(Ordering::SeqCst))
    }

    pub fn set_desired_state(&self, state: KernelDesiredState) {
        self.desired_state.store(state as u8, Ordering::SeqCst);
    }

    pub fn get_observed_state(&self) -> KernelObservedState {
        KernelObservedState::from_runtime(self.get_state(), &self.get_readiness())
    }

    /// 尝试过渡到启动状态，仅在可启动时返回 true
    pub fn try_transition_to_starting(&self) -> bool {
        let current = self.get_state();
        if current.can_start() {
            self.set_state(KernelState::Starting);
            true
        } else {
            false
        }
    }

    /// 尝试过渡到停止状态，仅在可停止时返回 true
    pub fn try_transition_to_stopping(&self) -> bool {
        let current = self.get_state();
        if current.can_stop() {
            self.set_state(KernelState::Stopping);
            true
        } else {
            false
        }
    }

    /// 标记为运行中
    pub fn mark_running(&self, api_port: u16) {
        self.api_port.store(api_port, Ordering::SeqCst);
        self.set_state(KernelState::Running);
        self.clear_startup_diagnosis();
        self.update_readiness(|readiness| {
            readiness.config_validated = Some(true);
            readiness.process_spawned = Some(true);
            readiness.process_alive = true;
            readiness.api_ready = true;
        });
    }

    /// 标记为已停止
    pub fn mark_stopped(&self) {
        self.api_port.store(0, Ordering::SeqCst);
        self.set_state(KernelState::Stopped);
        self.clear_startup_diagnosis();
        self.update_readiness(|readiness| {
            readiness.process_alive = false;
            readiness.api_ready = false;
            readiness.relay_ready = false;
        });
    }

    /// 标记为失败
    pub fn mark_failed(&self) {
        self.set_state(KernelState::Failed);
        self.update_readiness(|readiness| {
            readiness.process_alive = false;
            readiness.api_ready = false;
            readiness.relay_ready = false;
        });
    }

    /// 标记为崩溃（守护进程检测）
    pub fn mark_crashed(&self) {
        self.set_state(KernelState::Crashed);
    }

    /// 获取 API 端口
    pub fn get_api_port(&self) -> u16 {
        self.api_port.load(Ordering::SeqCst)
    }

    pub fn get_readiness(&self) -> KernelReadinessSnapshot {
        self.readiness.read().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn set_readiness(&self, readiness: KernelReadinessSnapshot) {
        if let Ok(mut guard) = self.readiness.write() {
            *guard = readiness;
        }
    }

    pub fn update_readiness<F>(&self, updater: F)
    where
        F: FnOnce(&mut KernelReadinessSnapshot),
    {
        if let Ok(mut guard) = self.readiness.write() {
            updater(&mut guard);
        }
    }

    pub fn get_startup_diagnosis(&self) -> Option<StartupDiagnosis> {
        self.startup_diagnosis.read().ok().and_then(|g| g.clone())
    }

    pub fn clear_startup_diagnosis(&self) {
        if let Ok(mut guard) = self.startup_diagnosis.write() {
            *guard = None;
        }
        if let Ok(mut guard) = self.current_attempt_id.write() {
            *guard = None;
        }
    }

    pub fn record_startup_diagnosis(&self, diagnosis: StartupDiagnosis) {
        if let Ok(mut guard) = self.startup_diagnosis.write() {
            match guard.as_ref() {
                None => *guard = Some(diagnosis),
                Some(existing) if existing.attempt_id != diagnosis.attempt_id => {
                    *guard = Some(diagnosis)
                }
                Some(existing) => {
                    let should_replace = diagnosis.kind.priority() > existing.kind.priority()
                        || (diagnosis.kind == existing.kind
                            && diagnosis.detail.len() >= existing.detail.len());
                    if should_replace {
                        *guard = Some(diagnosis);
                    }
                }
            }
        }
    }
}

impl Default for KernelStateManager {
    fn default() -> Self {
        Self::new()
    }
}

// 全局状态管理器实例
lazy_static::lazy_static! {
    pub static ref KERNEL_STATE: Arc<KernelStateManager> = Arc::new(KernelStateManager::new());
}

#[cfg(test)]
#[path = "state.tests.rs"]
mod tests;
