use crate::app::constants::paths;
use crate::app::core::kernel_service::event::start_websocket_relay;
use crate::app::core::kernel_service::orchestrator::execute_kernel_operation;
use crate::app::core::kernel_service::state::KERNEL_STATE;
use crate::app::core::kernel_service::status::is_kernel_running;
use crate::app::core::kernel_service::utils::{
    emit_kernel_error, emit_kernel_error_with_context, emit_kernel_started, emit_kernel_stopped,
    resolve_config_path_or_default,
};
use crate::app::core::kernel_service::{KernelProcessControl, PROCESS_MANAGER};
use crate::app::storage::enhanced_storage_service::db_get_app_config;
use futures::FutureExt;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tauri::{AppHandle, Runtime};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{info, warn};

static KEEP_ALIVE_ENABLED: AtomicBool = AtomicBool::new(false);
static GUARDED_API_PORT: AtomicU16 = AtomicU16::new(0);
static GUARDED_TUN_ENABLED: AtomicBool = AtomicBool::new(false);
/// 守护循环 tick 秒数（默认 8；测试可临时压低以覆盖重启分支）。
static GUARD_TICK_SECS: AtomicU64 = AtomicU64::new(8);
/// TUN 自愈 warmup 秒数（默认 20；测试可压到 0）。
static GUARD_WARMUP_SECS: AtomicU64 = AtomicU64::new(20);
/// 连通性探测覆盖：0=真实探测，1=强制成功，2=强制失败。
static GUARD_CONNECTIVITY_OVERRIDE: AtomicU16 = AtomicU16::new(0);

const TUN_CONNECTIVITY_FAIL_THRESHOLD: u8 = 3;
#[allow(dead_code)]
const TUN_SELF_HEAL_WARMUP_SECS: u64 = 20;
#[allow(dead_code)]
const GUARD_TICK_DEFAULT_SECS: u64 = 8;

/// 测试用：调整守护循环间隔（0 会按 1 秒下限处理，避免忙等）。
#[allow(dead_code)]
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn set_guard_tick_secs_for_tests(secs: u64) {
    GUARD_TICK_SECS.store(secs, Ordering::Relaxed);
}

#[allow(dead_code)]
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn reset_guard_tick_secs_for_tests() {
    GUARD_TICK_SECS.store(GUARD_TICK_DEFAULT_SECS, Ordering::Relaxed);
}

/// 测试用：压低 TUN 自愈 warmup（秒）。
#[allow(dead_code)]
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn set_guard_warmup_secs_for_tests(secs: u64) {
    GUARD_WARMUP_SECS.store(secs, Ordering::Relaxed);
}

#[allow(dead_code)]
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn reset_guard_warmup_secs_for_tests() {
    GUARD_WARMUP_SECS.store(TUN_SELF_HEAL_WARMUP_SECS, Ordering::Relaxed);
}

/// 测试用：覆盖连通性结果。`None`=真实；`Some(true/false)`=固定。
#[allow(dead_code)]
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn set_guard_connectivity_override_for_tests(result: Option<bool>) {
    let v = match result {
        None => 0,
        Some(true) => 1,
        Some(false) => 2,
    };
    GUARD_CONNECTIVITY_OVERRIDE.store(v, Ordering::Relaxed);
}

#[allow(dead_code)]
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn reset_guard_connectivity_override_for_tests() {
    GUARD_CONNECTIVITY_OVERRIDE.store(0, Ordering::Relaxed);
}

fn guard_tick_duration() -> Duration {
    let secs = GUARD_TICK_SECS.load(Ordering::Relaxed).max(1);
    Duration::from_secs(secs)
}

fn guard_warmup_duration() -> Duration {
    Duration::from_secs(GUARD_WARMUP_SECS.load(Ordering::Relaxed))
}

/// TUN 连通性探测（可测试覆盖）。
async fn probe_guard_tun_connectivity() -> Result<bool, ()> {
    match GUARD_CONNECTIVITY_OVERRIDE.load(Ordering::Relaxed) {
        1 => Ok(true),
        2 => Ok(false),
        _ => crate::app::system::system_service::check_network_connectivity(Some(false))
            .await
            .map_err(|_| ()),
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TunSelfHealPolicy {
    pub enabled: bool,
    pub cooldown_secs: u64,
}

impl TunSelfHealPolicy {
    pub(crate) fn default_policy() -> Self {
        Self {
            enabled: true,
            cooldown_secs: 90,
        }
    }

    /// 将配置中的冷却秒数钳制到安全区间（纯逻辑）。
    pub(crate) fn from_config(enabled: bool, cooldown_secs: u16) -> Self {
        Self {
            enabled,
            cooldown_secs: u64::from(cooldown_secs).clamp(15, 600),
        }
    }
}

/// 根据连通性结果更新失败计数，并判断是否达到自愈阈值（纯逻辑）。
pub(crate) fn update_tun_connectivity_failures(
    prev_failures: u8,
    connectivity_ok: Result<bool, ()>,
    threshold: u8,
) -> (u8, bool /* should_attempt_self_heal */) {
    match connectivity_ok {
        Ok(true) => (0, false),
        Ok(false) | Err(()) => {
            let next = prev_failures.saturating_add(1);
            (next, next >= threshold)
        }
    }
}

/// 自愈触发后下一次允许时间（纯逻辑）。
pub(crate) fn next_self_heal_deadline(now: Instant, cooldown_secs: u64) -> Instant {
    now + Duration::from_secs(cooldown_secs)
}

/// 是否已过自愈冷却（纯逻辑）。
pub(crate) fn can_self_heal_now(now: Instant, next_at: Instant) -> bool {
    now >= next_at
}

lazy_static::lazy_static! {
    pub(super) static ref KERNEL_GUARD_HANDLE: Mutex<Option<JoinHandle<()>>> =
        Mutex::new(None);
}

async fn load_tun_self_heal_policy<R: Runtime>(app_handle: &AppHandle<R>) -> TunSelfHealPolicy {
    match db_get_app_config(app_handle.clone()).await {
        Ok(config) => TunSelfHealPolicy::from_config(
            config.tun_self_heal_enabled,
            config.tun_self_heal_cooldown_secs,
        ),
        Err(err) => {
            warn!("读取 TUN 自愈策略失败，回退默认值: {}", err);
            TunSelfHealPolicy::default_policy()
        }
    }
}

/// 判断错误是否属于 sudo 密码类（纯逻辑）。
pub(crate) fn is_sudo_password_error(err: &str) -> bool {
    err.contains("SUDO_PASSWORD_REQUIRED") || err.contains("SUDO_PASSWORD_INVALID")
}

/// 守护重启前文件存在性检查结果（纯逻辑）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuardPreflightIssue {
    None,
    KernelBinaryMissing,
    ConfigMissing,
}

pub(crate) fn classify_guard_preflight(
    kernel_exists: bool,
    config_exists: bool,
) -> GuardPreflightIssue {
    if !kernel_exists {
        GuardPreflightIssue::KernelBinaryMissing
    } else if !config_exists {
        GuardPreflightIssue::ConfigMissing
    } else {
        GuardPreflightIssue::None
    }
}

/// 启用内核守护（process 可注入，测试入口）。
pub(super) async fn enable_kernel_guard_with_process<R: Runtime + 'static>(
    app_handle: AppHandle<R>,
    api_port: u16,
    tun_enabled: bool,
    process: Arc<dyn KernelProcessControl<R>>,
) {
    GUARDED_API_PORT.store(api_port, Ordering::Relaxed);
    GUARDED_TUN_ENABLED.store(tun_enabled, Ordering::Relaxed);
    if KEEP_ALIVE_ENABLED.swap(true, Ordering::Relaxed) {
        return;
    }

    let mut handle_slot = KERNEL_GUARD_HANDLE.lock().await;
    let guard_handle = tokio::spawn(async move {
        info!("内核守护已启动");
        let mut tun_connectivity_failures: u8 = 0;
        let mut next_tun_self_heal_at = Instant::now() + guard_warmup_duration();

        loop {
            if !KEEP_ALIVE_ENABLED.load(Ordering::Relaxed) {
                break;
            }

            tokio::time::sleep(guard_tick_duration()).await;

            if !KEEP_ALIVE_ENABLED.load(Ordering::Relaxed) {
                break;
            }

            match is_kernel_running().await {
                Ok(true) => {
                    if GUARDED_TUN_ENABLED.load(Ordering::Relaxed) {
                        let policy = load_tun_self_heal_policy(&app_handle).await;
                        if !policy.enabled {
                            tun_connectivity_failures = 0;
                            next_tun_self_heal_at = Instant::now() + guard_warmup_duration();
                            continue;
                        }

                        let connectivity = probe_guard_tun_connectivity().await;
                        let (next_failures, should_attempt_self_heal) =
                            update_tun_connectivity_failures(
                                tun_connectivity_failures,
                                connectivity,
                                TUN_CONNECTIVITY_FAIL_THRESHOLD,
                            );
                        if next_failures == 0 && tun_connectivity_failures > 0 {
                            info!("TUN 连通性已恢复，清空失败计数");
                        } else if next_failures > tun_connectivity_failures {
                            warn!(
                                "TUN 连通性检测失败/异常，计数: {}/{}",
                                next_failures, TUN_CONNECTIVITY_FAIL_THRESHOLD
                            );
                        }
                        tun_connectivity_failures = next_failures;

                        if should_attempt_self_heal
                            && can_self_heal_now(Instant::now(), next_tun_self_heal_at)
                        {
                            let port_value = GUARDED_API_PORT.load(Ordering::Relaxed);
                            info!(
                                "触发 TUN 自愈重启，准备重启内核进程: api_port={}, failures={}, cooldown_secs={}",
                                port_value, tun_connectivity_failures, policy.cooldown_secs
                            );

                            let operation_app = app_handle.clone();
                            let operation_process = process.clone();
                            let result = execute_kernel_operation(
                                app_handle.clone(),
                                "kernel.guard-tun-self-heal",
                                async move {
                                    if KERNEL_STATE.get_desired_state()
                                        != crate::app::core::kernel_service::KernelDesiredState::Running
                                    {
                                        return Ok(json!({ "success": true, "skipped": true }));
                                    }
                                    let config_path =
                                        resolve_config_path_or_default(&operation_app).await;
                                    let tun_enabled =
                                        GUARDED_TUN_ENABLED.load(Ordering::Relaxed);
                                    operation_process
                                        .restart(&operation_app, &config_path, tun_enabled)
                                        .await?;
                                    KERNEL_STATE.mark_running(port_value);
                                    if port_value > 0 {
                                        if let Err(e) = start_websocket_relay(
                                            operation_app.clone(),
                                            Some(port_value),
                                        )
                                        .await
                                        {
                                            warn!("TUN 自愈后启动事件中继失败: {}", e);
                                        }
                                    }
                                    emit_kernel_started(
                                        &operation_app,
                                        "auto",
                                        port_value,
                                        0,
                                        true,
                                    );
                                    Ok(json!({ "success": true, "skipped": false }))
                                }
                                .boxed(),
                            )
                            .await;

                            match result {
                                Ok(value) => {
                                    if value
                                        .get("skipped")
                                        .and_then(|value| value.as_bool())
                                        .unwrap_or(false)
                                    {
                                        KEEP_ALIVE_ENABLED.store(false, Ordering::Relaxed);
                                        break;
                                    }
                                    tun_connectivity_failures = 0;
                                    next_tun_self_heal_at = next_self_heal_deadline(
                                        Instant::now(),
                                        policy.cooldown_secs,
                                    );
                                    info!("TUN 自愈重启完成");
                                }
                                Err(err) => {
                                    warn!("TUN 自愈重启失败: {}", err);
                                    KERNEL_STATE.mark_failed();
                                    next_tun_self_heal_at = next_self_heal_deadline(
                                        Instant::now(),
                                        policy.cooldown_secs,
                                    );

                                    let err_str = err.to_string();
                                    if is_sudo_password_error(&err_str) {
                                        emit_kernel_error(
                                            &app_handle,
                                            "TUN 提权失败：sudo 密码无效，请重新输入系统密码后重启内核。",
                                        );
                                        KEEP_ALIVE_ENABLED.store(false, Ordering::Relaxed);
                                        GUARDED_API_PORT.store(0, Ordering::Relaxed);
                                        GUARDED_TUN_ENABLED.store(false, Ordering::Relaxed);
                                        break;
                                    }

                                    emit_kernel_error_with_context(
                                        &app_handle,
                                        "KERNEL_GUARD_SELF_HEAL_FAILED",
                                        "内核自愈重启失败",
                                        Some(&err_str),
                                        Some("kernel.guard.self_heal"),
                                        true,
                                    );
                                }
                            }
                        }
                    } else {
                        tun_connectivity_failures = 0;
                        next_tun_self_heal_at = Instant::now() + guard_warmup_duration();
                    }

                    continue;
                }
                _ => {
                    let port_value = GUARDED_API_PORT.load(Ordering::Relaxed);
                    let tun_enabled = GUARDED_TUN_ENABLED.load(Ordering::Relaxed);
                    info!(
                        "守护检测到内核停止，尝试自动重启: api_port={}, tun_enabled={}",
                        port_value, tun_enabled
                    );
                    KERNEL_STATE.mark_crashed();

                    emit_kernel_stopped(&app_handle);

                    if KERNEL_STATE.get_desired_state()
                        != crate::app::core::kernel_service::KernelDesiredState::Running
                    {
                        info!("守护检测到期望状态为停止，结束守护而不重启");
                        KEEP_ALIVE_ENABLED.store(false, Ordering::Relaxed);
                        break;
                    }

                    let config_path = resolve_config_path_or_default(&app_handle).await;

                    let kernel_path = paths::get_kernel_path();
                    match classify_guard_preflight(kernel_path.exists(), config_path.exists()) {
                        GuardPreflightIssue::KernelBinaryMissing => {
                            warn!("守护跳过重启：内核文件不存在 {:?}", kernel_path);
                            KERNEL_STATE.mark_failed();
                            emit_kernel_error_with_context(
                                &app_handle,
                                "KERNEL_BINARY_MISSING",
                                "自动重启失败：内核文件不存在",
                                Some(&format!("{:?}", kernel_path)),
                                Some("kernel.guard.restart"),
                                false,
                            );
                            KEEP_ALIVE_ENABLED.store(false, Ordering::Relaxed);
                            GUARDED_API_PORT.store(0, Ordering::Relaxed);
                            break;
                        }
                        GuardPreflightIssue::ConfigMissing => {
                            warn!("守护跳过重启：配置不存在 {:?}", config_path);
                            KERNEL_STATE.mark_failed();
                            emit_kernel_error_with_context(
                                &app_handle,
                                "KERNEL_CONFIG_MISSING",
                                "自动重启失败：配置文件不存在",
                                Some(&format!("{:?}", config_path)),
                                Some("kernel.guard.restart"),
                                false,
                            );
                            KEEP_ALIVE_ENABLED.store(false, Ordering::Relaxed);
                            GUARDED_API_PORT.store(0, Ordering::Relaxed);
                            break;
                        }
                        GuardPreflightIssue::None => {}
                    }

                    let operation_app = app_handle.clone();
                    let operation_process = process.clone();
                    let operation_config_path = config_path.clone();
                    let restart_result = execute_kernel_operation(
                        app_handle.clone(),
                        "kernel.guard-crash-recovery",
                        async move {
                            if KERNEL_STATE.get_desired_state()
                                != crate::app::core::kernel_service::KernelDesiredState::Running
                            {
                                return Ok(json!({ "success": true, "skipped": true }));
                            }
                            operation_process
                                .start(Some(&operation_app), &operation_config_path, tun_enabled)
                                .await?;
                            KERNEL_STATE.mark_running(port_value);
                            if port_value > 0 {
                                if let Err(e) =
                                    start_websocket_relay(operation_app.clone(), Some(port_value))
                                        .await
                                {
                                    warn!("守护启动事件中继失败: {}", e);
                                }
                            }
                            emit_kernel_started(&operation_app, "auto", port_value, 0, true);
                            Ok(json!({ "success": true, "skipped": false }))
                        }
                        .boxed(),
                    )
                    .await;

                    match restart_result {
                        Ok(value)
                            if value
                                .get("skipped")
                                .and_then(|value| value.as_bool())
                                .unwrap_or(false) =>
                        {
                            KEEP_ALIVE_ENABLED.store(false, Ordering::Relaxed);
                            break;
                        }
                        Ok(_) => {}
                        Err(err) => {
                            warn!("守护重启内核失败: {}", err);
                            KERNEL_STATE.mark_failed();

                            let err_str = err.to_string();
                            if is_sudo_password_error(&err_str) {
                                // 若因 sudo 密码失效而重启失败，停止守护并提示用户重新设置密码。
                                emit_kernel_error(
                                    &app_handle,
                                    "TUN 提权失败：sudo 密码无效，请重新输入系统密码后重启内核。",
                                );
                                KEEP_ALIVE_ENABLED.store(false, Ordering::Relaxed);
                                GUARDED_API_PORT.store(0, Ordering::Relaxed);
                                GUARDED_TUN_ENABLED.store(false, Ordering::Relaxed);
                                break;
                            }

                            emit_kernel_error_with_context(
                                &app_handle,
                                "KERNEL_GUARD_RESTART_FAILED",
                                "守护自动重启失败",
                                Some(&err_str),
                                Some("kernel.guard.restart"),
                                true,
                            );
                            continue;
                        }
                    }

                    tun_connectivity_failures = 0;
                    next_tun_self_heal_at = Instant::now() + guard_warmup_duration();
                }
            }
        }

        info!("内核守护任务结束");
    });

    *handle_slot = Some(guard_handle);
}

pub(super) async fn enable_kernel_guard<R: Runtime + 'static>(
    app_handle: AppHandle<R>,
    api_port: u16,
    tun_enabled: bool,
) {
    enable_kernel_guard_with_process(
        app_handle,
        api_port,
        tun_enabled,
        Arc::clone(&PROCESS_MANAGER) as Arc<dyn KernelProcessControl<R>>,
    )
    .await
}

pub(super) async fn disable_kernel_guard() {
    if !KEEP_ALIVE_ENABLED.swap(false, Ordering::Relaxed) {
        return;
    }

    GUARDED_API_PORT.store(0, Ordering::Relaxed);
    GUARDED_TUN_ENABLED.store(false, Ordering::Relaxed);
    let mut handle_slot = KERNEL_GUARD_HANDLE.lock().await;
    if let Some(handle) = handle_slot.take() {
        handle.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static GUARD_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[test]
    fn tun_self_heal_policy_default_and_clamp() {
        let d = TunSelfHealPolicy::default_policy();
        assert!(d.enabled);
        assert_eq!(d.cooldown_secs, 90);

        let low = TunSelfHealPolicy::from_config(true, 1);
        assert_eq!(low.cooldown_secs, 15);

        let high = TunSelfHealPolicy::from_config(false, 9999);
        assert!(!high.enabled);
        assert_eq!(high.cooldown_secs, 600);

        let mid = TunSelfHealPolicy::from_config(true, 120);
        assert_eq!(mid.cooldown_secs, 120);
    }

    #[test]
    fn tun_connectivity_failure_counter_and_heal_gate() {
        assert_eq!(update_tun_connectivity_failures(0, Ok(true), 3), (0, false));
        assert_eq!(update_tun_connectivity_failures(2, Ok(true), 3), (0, false));
        assert_eq!(
            update_tun_connectivity_failures(0, Ok(false), 3),
            (1, false)
        );
        assert_eq!(update_tun_connectivity_failures(2, Ok(false), 3), (3, true));
        assert_eq!(update_tun_connectivity_failures(2, Err(()), 3), (3, true));

        let now = Instant::now();
        let next = next_self_heal_deadline(now, 90);
        assert!(!can_self_heal_now(now, next));
        assert!(can_self_heal_now(next, next));
        assert!(can_self_heal_now(next + Duration::from_secs(1), next));
    }

    #[tokio::test]
    async fn disable_kernel_guard_idempotent() {
        let _test_guard = GUARD_TEST_LOCK.lock().await;
        // 未启用时调用应直接返回
        disable_kernel_guard().await;
        disable_kernel_guard().await;
    }

    #[test]
    fn tun_connectivity_threshold_edges_and_saturation() {
        // 阈值为 1：单次失败即触发
        assert_eq!(update_tun_connectivity_failures(0, Ok(false), 1), (1, true));
        // 阈值为 0：任意失败计数后 next>=0 恒真
        assert_eq!(update_tun_connectivity_failures(0, Err(()), 0), (1, true));
        // u8 饱和
        assert_eq!(
            update_tun_connectivity_failures(u8::MAX, Ok(false), 3),
            (u8::MAX, true)
        );
        // 成功重置
        assert_eq!(
            update_tun_connectivity_failures(u8::MAX, Ok(true), 3),
            (0, false)
        );
    }

    #[test]
    fn self_heal_deadline_zero_cooldown() {
        let now = Instant::now();
        let next = next_self_heal_deadline(now, 0);
        assert!(can_self_heal_now(now, next));
        let later = next_self_heal_deadline(now, 3600);
        assert!(!can_self_heal_now(now, later));
    }

    #[test]
    fn tun_self_heal_policy_boundary_clamp_values() {
        assert_eq!(TunSelfHealPolicy::from_config(true, 15).cooldown_secs, 15);
        assert_eq!(TunSelfHealPolicy::from_config(true, 600).cooldown_secs, 600);
        assert_eq!(TunSelfHealPolicy::from_config(true, 14).cooldown_secs, 15);
        assert_eq!(
            TunSelfHealPolicy::from_config(false, 601).cooldown_secs,
            600
        );
        // 常量存在性
        assert_eq!(TUN_CONNECTIVITY_FAIL_THRESHOLD, 3);
        assert_eq!(TUN_SELF_HEAL_WARMUP_SECS, 20);
    }

    #[test]
    fn is_sudo_password_error_and_preflight() {
        assert!(is_sudo_password_error("x SUDO_PASSWORD_REQUIRED y"));
        assert!(is_sudo_password_error("SUDO_PASSWORD_INVALID"));
        assert!(!is_sudo_password_error("other"));

        assert_eq!(
            classify_guard_preflight(false, true),
            GuardPreflightIssue::KernelBinaryMissing
        );
        assert_eq!(
            classify_guard_preflight(true, false),
            GuardPreflightIssue::ConfigMissing
        );
        assert_eq!(
            classify_guard_preflight(true, true),
            GuardPreflightIssue::None
        );
        assert_eq!(
            classify_guard_preflight(false, false),
            GuardPreflightIssue::KernelBinaryMissing
        );
    }

    #[tokio::test]
    async fn enable_then_disable_guard_with_mock_app() {
        let _test_guard = GUARD_TEST_LOCK.lock().await;
        use crate::test_support::MockAppEnv;

        let env = MockAppEnv::new();
        let h = env.handle();
        // 启用守护（会 spawn 循环）；立即 disable 覆盖启用/禁用路径
        enable_kernel_guard(h.clone(), 19090, false).await;
        // 再次 enable 应 early-return（KEEP_ALIVE 已 true）
        enable_kernel_guard(h, 19091, true).await;
        disable_kernel_guard().await;
        // 幂等
        disable_kernel_guard().await;
    }

    #[tokio::test]
    async fn guard_loop_restarts_missing_kernel_then_stops() {
        let _test_guard = GUARD_TEST_LOCK.lock().await;
        use crate::test_support::MockAppEnv;
        use std::fs;

        // 压低 tick，使循环尽快进入“内核未运行 → 重启失败（缺二进制）”分支
        #[cfg(any(test, feature = "test-util"))]
        set_guard_tick_secs_for_tests(1);
        let env = MockAppEnv::new();
        // 工作区下无 sing-box 可执行文件 → preflight KernelBinaryMissing
        let cfg_path = env.workspace.path().join("sing-box/config.json");
        fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
        fs::write(
            &cfg_path,
            r#"{"log":{"level":"info"},"inbounds":[],"outbounds":[{"type":"direct","tag":"direct"}]}"#,
        )
        .unwrap();
        let db = env.workspace.path().join("guard.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        let mut cfg = crate::app::storage::state_model::AppConfig::default();
        cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
        storage.save_app_config(&cfg).await.unwrap();

        let h = env.handle();
        enable_kernel_guard(h, 19100, false).await;
        // 等待至少 1 个 tick + 处理时间
        tokio::time::sleep(Duration::from_millis(1500)).await;
        disable_kernel_guard().await;
        #[cfg(any(test, feature = "test-util"))]
        reset_guard_tick_secs_for_tests();
    }

    #[tokio::test]
    async fn guard_loop_running_kernel_tun_disabled_path() {
        let _test_guard = GUARD_TEST_LOCK.lock().await;
        use crate::app::core::kernel_service::PROCESS_MANAGER;
        use crate::test_support::MockAppEnv;
        use std::fs;
        use std::path::Path;

        #[cfg(any(test, feature = "test-util"))]
        set_guard_tick_secs_for_tests(1);

        let env = MockAppEnv::new();
        // 安装假内核并 start_inner，使 is_kernel_running=true 进入 TUN 关闭分支 continue
        let work = env.workspace.path();
        let dir = work.join("sing-box");
        fs::create_dir_all(&dir).unwrap();
        let kernel = dir.join("sing-box");
        fs::write(
            &kernel,
            r#"#!/bin/sh
if [ "$1" = "check" ]; then exit 0; fi
if [ "$1" = "run" ]; then exec sleep "${FAKE_KERNEL_RUN_SECS:-5}"; fi
exit 0
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = fs::metadata(&kernel).unwrap().permissions();
            p.set_mode(0o755);
            fs::set_permissions(&kernel, p).unwrap();
        }
        let cfg_path = dir.join("config.json");
        fs::write(
            &cfg_path,
            r#"{"log":{"level":"info"},"inbounds":[],"outbounds":[{"type":"direct","tag":"direct"}]}"#,
        )
        .unwrap();
        let _ = PROCESS_MANAGER
            .start_inner::<tauri::Wry>(None, Path::new(&cfg_path), false)
            .await;

        let h = env.handle();
        enable_kernel_guard(h, 19101, false).await;
        tokio::time::sleep(Duration::from_millis(1500)).await;
        disable_kernel_guard().await;
        let _ = PROCESS_MANAGER.stop::<tauri::Wry>(None).await;
        #[cfg(any(test, feature = "test-util"))]
        reset_guard_tick_secs_for_tests();
    }

    #[tokio::test]
    async fn guard_loop_tun_self_heal_disabled_policy() {
        let _test_guard = GUARD_TEST_LOCK.lock().await;
        use crate::app::core::kernel_service::PROCESS_MANAGER;
        use crate::test_support::MockAppEnv;
        use std::fs;
        use std::path::Path;

        #[cfg(any(test, feature = "test-util"))]
        {
            set_guard_tick_secs_for_tests(1);
            set_guard_warmup_secs_for_tests(0);
            set_guard_connectivity_override_for_tests(Some(false));
        }

        let env = MockAppEnv::new();
        let work = env.workspace.path();
        let dir = work.join("sing-box");
        fs::create_dir_all(&dir).unwrap();
        let kernel = dir.join("sing-box");
        fs::write(
            &kernel,
            r#"#!/bin/sh
if [ "$1" = "check" ]; then exit 0; fi
if [ "$1" = "run" ]; then exec sleep "${FAKE_KERNEL_RUN_SECS:-5}"; fi
exit 0
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = fs::metadata(&kernel).unwrap().permissions();
            p.set_mode(0o755);
            fs::set_permissions(&kernel, p).unwrap();
        }
        let cfg_path = dir.join("config.json");
        fs::write(
            &cfg_path,
            r#"{"log":{"level":"info"},"inbounds":[],"outbounds":[{"type":"direct","tag":"direct"}]}"#,
        )
        .unwrap();
        let db = work.join("guard-tun-off.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        let mut cfg = crate::app::storage::state_model::AppConfig::default();
        cfg.tun_self_heal_enabled = false;
        cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
        storage.save_app_config(&cfg).await.unwrap();

        let _ = PROCESS_MANAGER
            .start_inner::<tauri::Wry>(None, Path::new(&cfg_path), false)
            .await;

        // tun_enabled=true 但 policy.enabled=false → 重置失败计数 continue
        enable_kernel_guard(env.handle(), 19110, true).await;
        tokio::time::sleep(Duration::from_millis(1800)).await;
        disable_kernel_guard().await;
        let _ = PROCESS_MANAGER.stop::<tauri::Wry>(None).await;
        #[cfg(any(test, feature = "test-util"))]
        {
            reset_guard_tick_secs_for_tests();
            reset_guard_warmup_secs_for_tests();
            reset_guard_connectivity_override_for_tests();
        }
    }

    #[tokio::test]
    async fn guard_loop_tun_self_heal_connectivity_fail_triggers_restart() {
        let _test_guard = GUARD_TEST_LOCK.lock().await;
        use crate::app::core::kernel_service::PROCESS_MANAGER;
        use crate::test_support::MockAppEnv;
        use std::fs;
        use std::path::Path;

        #[cfg(any(test, feature = "test-util"))]
        {
            set_guard_tick_secs_for_tests(1);
            set_guard_warmup_secs_for_tests(0);
            set_guard_connectivity_override_for_tests(Some(false));
        }

        let env = MockAppEnv::new();
        let work = env.workspace.path();
        let dir = work.join("sing-box");
        fs::create_dir_all(&dir).unwrap();
        let kernel = dir.join("sing-box");
        fs::write(
            &kernel,
            r#"#!/bin/sh
if [ "$1" = "check" ]; then exit 0; fi
if [ "$1" = "run" ]; then exec sleep "${FAKE_KERNEL_RUN_SECS:-5}"; fi
exit 0
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = fs::metadata(&kernel).unwrap().permissions();
            p.set_mode(0o755);
            fs::set_permissions(&kernel, p).unwrap();
        }
        let cfg_path = dir.join("config.json");
        fs::write(
            &cfg_path,
            r#"{"log":{"level":"info"},"inbounds":[],"outbounds":[{"type":"direct","tag":"direct"}]}"#,
        )
        .unwrap();
        let db = work.join("guard-heal.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        let mut cfg = crate::app::storage::state_model::AppConfig::default();
        cfg.tun_self_heal_enabled = true;
        cfg.tun_self_heal_cooldown_secs = 15;
        cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
        storage.save_app_config(&cfg).await.unwrap();

        let _ = PROCESS_MANAGER
            .start_inner::<tauri::Wry>(None, Path::new(&cfg_path), false)
            .await;

        // 连续失败 ≥3 次后触发自愈 restart（非 TUN 假内核可 restart_inner）
        enable_kernel_guard(env.handle(), 19111, true).await;
        // 至少 3 个 tick 累计失败 + 一次自愈尝试
        tokio::time::sleep(Duration::from_millis(4500)).await;
        disable_kernel_guard().await;
        let _ = PROCESS_MANAGER.stop::<tauri::Wry>(None).await;
        #[cfg(any(test, feature = "test-util"))]
        {
            reset_guard_tick_secs_for_tests();
            reset_guard_warmup_secs_for_tests();
            reset_guard_connectivity_override_for_tests();
        }
    }

    #[tokio::test]
    async fn guard_loop_config_missing_preflight() {
        let _test_guard = GUARD_TEST_LOCK.lock().await;
        use crate::test_support::MockAppEnv;
        use std::fs;

        #[cfg(any(test, feature = "test-util"))]
        set_guard_tick_secs_for_tests(1);

        let env = MockAppEnv::new();
        // 有假内核二进制但配置路径不存在 → ConfigMissing
        let dir = env.workspace.path().join("sing-box");
        fs::create_dir_all(&dir).unwrap();
        let kernel = dir.join("sing-box");
        fs::write(
            &kernel,
            r#"#!/bin/sh
exit 0
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = fs::metadata(&kernel).unwrap().permissions();
            p.set_mode(0o755);
            fs::set_permissions(&kernel, p).unwrap();
        }
        let db = env.workspace.path().join("guard-cfg-miss.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        let mut cfg = crate::app::storage::state_model::AppConfig::default();
        cfg.active_config_path = Some(
            env.workspace
                .path()
                .join("no-such-config.json")
                .to_string_lossy()
                .to_string(),
        );
        storage.save_app_config(&cfg).await.unwrap();

        enable_kernel_guard(env.handle(), 19112, false).await;
        tokio::time::sleep(Duration::from_millis(1800)).await;
        disable_kernel_guard().await;
        #[cfg(any(test, feature = "test-util"))]
        reset_guard_tick_secs_for_tests();
    }

    #[tokio::test]
    async fn guard_loop_successful_restart_with_fake_kernel() {
        let _test_guard = GUARD_TEST_LOCK.lock().await;
        use crate::test_support::MockAppEnv;
        use std::fs;

        #[cfg(any(test, feature = "test-util"))]
        set_guard_tick_secs_for_tests(1);

        let env = MockAppEnv::new();
        let work = env.workspace.path();
        let dir = work.join("sing-box");
        fs::create_dir_all(&dir).unwrap();
        let kernel = dir.join("sing-box");
        fs::write(
            &kernel,
            r#"#!/bin/sh
if [ "$1" = "check" ]; then exit 0; fi
if [ "$1" = "run" ]; then exec sleep "${FAKE_KERNEL_RUN_SECS:-5}"; fi
exit 0
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = fs::metadata(&kernel).unwrap().permissions();
            p.set_mode(0o755);
            fs::set_permissions(&kernel, p).unwrap();
        }
        let cfg_path = dir.join("config.json");
        fs::write(
            &cfg_path,
            r#"{"log":{"level":"info"},"inbounds":[],"outbounds":[{"type":"direct","tag":"direct"}]}"#,
        )
        .unwrap();
        let db = work.join("guard-restart-ok.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        let mut cfg = crate::app::storage::state_model::AppConfig::default();
        cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
        storage.save_app_config(&cfg).await.unwrap();

        // 不预先 start：守护检测到未运行后成功 start
        enable_kernel_guard(env.handle(), 19113, false).await;
        tokio::time::sleep(Duration::from_millis(2500)).await;
        disable_kernel_guard().await;
        let _ = crate::app::core::kernel_service::PROCESS_MANAGER
            .stop::<tauri::Wry>(None)
            .await;
        #[cfg(any(test, feature = "test-util"))]
        reset_guard_tick_secs_for_tests();
    }

    #[tokio::test]
    async fn enable_kernel_guard_with_process_triggers_start() {
        let _test_guard = GUARD_TEST_LOCK.lock().await;
        use crate::app::core::kernel_service::status::set_platform_kernel_detection_enabled_for_tests;
        use crate::app::core::kernel_service::{
            reset_process_controller_for_test, set_process_controller_for_test,
        };
        use crate::test_support::{FakeProcessController, MockAppEnv};
        use std::fs;
        use std::sync::Arc;

        #[cfg(any(test, feature = "test-util"))]
        set_guard_tick_secs_for_tests(1);
        set_platform_kernel_detection_enabled_for_tests(false);

        let env = MockAppEnv::new();
        let work = env.workspace.path();
        let dir = work.join("sing-box");
        fs::create_dir_all(&dir).unwrap();
        let kernel = dir.join("sing-box");
        fs::write(
            &kernel,
            r#"#!/bin/sh
if [ "$1" = "check" ]; then exit 0; fi
if [ "$1" = "run" ]; then exec sleep "${FAKE_KERNEL_RUN_SECS:-5}"; fi
exit 0
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = fs::metadata(&kernel).unwrap().permissions();
            p.set_mode(0o755);
            fs::set_permissions(&kernel, p).unwrap();
        }
        let cfg_path = dir.join("config.json");
        fs::write(
            &cfg_path,
            r#"{"log":{"level":"info"},"inbounds":[],"outbounds":[{"type":"direct","tag":"direct"}]}"#,
        )
        .unwrap();
        let db = work.join("guard-process.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        let mut cfg = crate::app::storage::state_model::AppConfig::default();
        cfg.active_config_path = Some(cfg_path.to_string_lossy().to_string());
        storage.save_app_config(&cfg).await.unwrap();

        let fake = Arc::new(FakeProcessController::default());
        fake.set_start_result(Ok(()));
        set_process_controller_for_test(fake.clone());

        // 确保守护状态干净
        disable_kernel_guard().await;
        KERNEL_STATE
            .set_desired_state(crate::app::core::kernel_service::KernelDesiredState::Running);

        enable_kernel_guard_with_process(env.handle(), 19120, false, fake.clone()).await;
        tokio::time::sleep(Duration::from_millis(2500)).await;
        disable_kernel_guard().await;

        let calls = fake.calls.lock().unwrap();
        assert!(
            calls.iter().any(|c| c.method == "start"),
            "guard should call process.start when kernel not running, calls={:?}",
            calls
        );

        reset_process_controller_for_test();
        KERNEL_STATE
            .set_desired_state(crate::app::core::kernel_service::KernelDesiredState::Stopped);
        set_platform_kernel_detection_enabled_for_tests(true);
        #[cfg(any(test, feature = "test-util"))]
        reset_guard_tick_secs_for_tests();
    }

    #[tokio::test]
    async fn load_tun_self_heal_policy_from_storage_and_fallback() {
        let _test_guard = GUARD_TEST_LOCK.lock().await;
        use crate::test_support::MockAppEnv;

        // 无 storage → fallback default（作用域结束释放 ENV_LOCK）
        {
            let env_no = MockAppEnv::new();
            let p1 = load_tun_self_heal_policy(&env_no.handle()).await;
            assert!(p1.enabled);
            assert_eq!(p1.cooldown_secs, 90);
        }

        // 独立 env + storage：读取自定义策略
        let env = MockAppEnv::new();
        let db = env.workspace.path().join("policy.db");
        let storage = env.install_storage_from_path(db.to_str().unwrap()).await;
        let mut cfg = crate::app::storage::state_model::AppConfig::default();
        cfg.tun_self_heal_enabled = false;
        cfg.tun_self_heal_cooldown_secs = 30;
        storage.save_app_config(&cfg).await.unwrap();
        // 校验 storage 已写入
        let saved = storage.get_app_config().await.unwrap();
        assert!(!saved.tun_self_heal_enabled);
        assert_eq!(saved.tun_self_heal_cooldown_secs, 30);
        let p2 = load_tun_self_heal_policy(&env.handle()).await;
        assert!(!p2.enabled);
        assert_eq!(p2.cooldown_secs, 30);
    }
}
