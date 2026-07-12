use super::icon;
use super::model::{
    events, menu_ids, TrayCloseBehavior, TrayNavigatePayload, TrayRuntimeStateInput,
    TrayToggleProxyFeaturePayload, TRAY_ICON_ID,
};
use super::state::TrayRuntimeState;
use crate::app::core::kernel_service::runtime::kernel_restart_fast;
use crate::app::core::kernel_service::status::is_kernel_running;
use crate::app::runtime::change::{RuntimeApplyOptions, RuntimeChange};
use crate::app::runtime::orchestrator::apply_runtime_change;
use crate::app::storage::enhanced_storage_service::db_save_app_config_internal;
use crate::app::storage::state_model::AppConfig;
use lazy_static::lazy_static;
use std::sync::RwLock;
use std::time::Duration;
use tauri::menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow, WebviewWindowBuilder};
use tracing::{debug, info, warn};

lazy_static! {
    static ref TRAY_RUNTIME_STATE: RwLock<TrayRuntimeState> =
        RwLock::new(TrayRuntimeState::default());
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TrayText {
    show_window: &'static str,
    rebuild_window: &'static str,
    kernel_menu: &'static str,
    restart_kernel: &'static str,
    status_running: &'static str,
    status_stopped: &'static str,
    proxy_controls: &'static str,
    current_status: &'static str,
    mode_system: &'static str,
    mode_tun: &'static str,
    mode_manual: &'static str,
    quit: &'static str,
    tooltip_kernel: &'static str,
    tooltip_mode: &'static str,
    tooltip_subscription: &'static str,
}

const TRAY_TEXT_ZH_CN: TrayText = TrayText {
    show_window: "显示主界面",
    rebuild_window: "重建窗口",
    kernel_menu: "内核",
    restart_kernel: "重启内核",
    status_running: "运行中",
    status_stopped: "已停止",
    proxy_controls: "代理开关",
    current_status: "当前状态：",
    mode_system: "系统代理",
    mode_tun: "TUN 模式",
    mode_manual: "手动模式",
    quit: "退出",
    tooltip_kernel: "内核: ",
    tooltip_mode: "模式: ",
    tooltip_subscription: "订阅: ",
};

const TRAY_TEXT_EN_US: TrayText = TrayText {
    show_window: "Show Main Window",
    rebuild_window: "Rebuild Window",
    kernel_menu: "Kernel",
    restart_kernel: "Restart Kernel",
    status_running: "Running",
    status_stopped: "Stopped",
    proxy_controls: "Proxy Controls",
    current_status: "Current Status:",
    mode_system: "System",
    mode_tun: "TUN",
    mode_manual: "Manual",
    quit: "Quit",
    tooltip_kernel: "Kernel: ",
    tooltip_mode: "Mode: ",
    tooltip_subscription: "Subscription: ",
};

const TRAY_TEXT_JA_JP: TrayText = TrayText {
    show_window: "メイン画面を表示",
    rebuild_window: "ウィンドウを再構築",
    kernel_menu: "カーネル",
    restart_kernel: "カーネルを再起動",
    status_running: "稼働中",
    status_stopped: "停止中",
    proxy_controls: "プロキシ切替",
    current_status: "現在の状態：",
    mode_system: "システム",
    mode_tun: "TUN",
    mode_manual: "手動",
    quit: "終了",
    tooltip_kernel: "カーネル: ",
    tooltip_mode: "モード: ",
    tooltip_subscription: "サブスクリプション: ",
};

const TRAY_TEXT_RU_RU: TrayText = TrayText {
    show_window: "Показать окно",
    rebuild_window: "Пересоздать окно",
    kernel_menu: "Ядро",
    restart_kernel: "Перезапустить ядро",
    status_running: "Запущено",
    status_stopped: "Остановлено",
    proxy_controls: "Прокси-переключатели",
    current_status: "Текущее состояние:",
    mode_system: "Системный",
    mode_tun: "TUN",
    mode_manual: "Ручной",
    quit: "Выход",
    tooltip_kernel: "Ядро: ",
    tooltip_mode: "Режим: ",
    tooltip_subscription: "Подписка: ",
};

fn with_state_read<T>(f: impl FnOnce(&TrayRuntimeState) -> T) -> T {
    let guard = TRAY_RUNTIME_STATE
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&guard)
}

fn with_state_write<T>(f: impl FnOnce(&mut TrayRuntimeState) -> T) -> T {
    let mut guard = TRAY_RUNTIME_STATE
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard)
}

pub(crate) fn tray_text_for_locale(locale: &str) -> TrayText {
    let normalized = locale.trim().to_lowercase();
    if normalized.starts_with("zh") {
        TRAY_TEXT_ZH_CN
    } else if normalized.starts_with("ja") {
        TRAY_TEXT_JA_JP
    } else if normalized.starts_with("ru") {
        TRAY_TEXT_RU_RU
    } else {
        TRAY_TEXT_EN_US
    }
}

pub(crate) fn mode_summary_text(state: &TrayRuntimeState, text: &TrayText) -> String {
    match (state.system_proxy_enabled, state.tun_enabled) {
        (true, true) => format!("{} + {}", text.mode_system, text.mode_tun),
        (true, false) => text.mode_system.to_string(),
        (false, true) => text.mode_tun.to_string(),
        (false, false) => text.mode_manual.to_string(),
    }
}

pub(crate) fn compose_tooltip(state: &TrayRuntimeState, text: &TrayText) -> String {
    let kernel_status = if state.kernel_running {
        text.status_running
    } else {
        text.status_stopped
    };
    let mode = mode_summary_text(state, text);

    let mut tooltip = format!(
        "sing-box-window - {}{}, {}{}",
        text.tooltip_kernel, kernel_status, text.tooltip_mode, mode
    );

    if let Some(subscription_name) = state.active_subscription_name.as_ref() {
        tooltip.push_str(&format!(
            ", {}{}",
            text.tooltip_subscription, subscription_name
        ));
    }

    tooltip
}

fn resolve_tray_icon(
    app: &AppHandle,
    state: &TrayRuntimeState,
) -> Option<tauri::image::Image<'static>> {
    if let Some(icon) = app.default_window_icon() {
        if let Some(recolored) = icon::recolor_icon_for_mode(icon, state.display_mode()) {
            return Some(recolored);
        }

        return Some(icon.clone().to_owned());
    }

    None
}

/// 托盘菜单文案（纯逻辑，不构建真实 Menu）。
pub(crate) struct TrayMenuLabels {
    pub primary_window_action: &'static str,
    pub kernel_status: &'static str,
    pub kernel_restart_enabled: bool,
    pub current_mode: String,
    pub system_checked: bool,
    pub tun_checked: bool,
}

pub(crate) fn build_tray_menu_labels(state: &TrayRuntimeState, text: &TrayText) -> TrayMenuLabels {
    TrayMenuLabels {
        primary_window_action: if state.window_visible {
            text.show_window
        } else {
            text.rebuild_window
        },
        kernel_status: if state.kernel_running {
            text.status_running
        } else {
            text.status_stopped
        },
        kernel_restart_enabled: state.kernel_running,
        current_mode: format!("{} {}", text.current_status, mode_summary_text(state, text)),
        system_checked: state.system_proxy_enabled,
        tun_checked: state.tun_enabled,
    }
}

/// 解析托盘菜单 id 对应的动作名（纯逻辑）。
pub(crate) fn classify_tray_menu_id(menu_id: &str) -> &'static str {
    match menu_id {
        id if id == menu_ids::SHOW_WINDOW => "show_window",
        id if id == menu_ids::KERNEL_RESTART => "kernel_restart",
        id if id == menu_ids::PROXY_SYSTEM => "proxy_system",
        id if id == menu_ids::PROXY_TUN => "proxy_tun",
        id if id == menu_ids::QUIT => "quit",
        _ => "unknown",
    }
}

#[allow(dead_code)]
pub(crate) fn next_proxy_toggle_enabled(
    menu_id: &str,
    system_proxy_enabled: bool,
    tun_enabled: bool,
) -> Option<(/* feature */ &'static str, /* enabled */ bool)> {
    match classify_tray_menu_id(menu_id) {
        "proxy_system" => Some(("systemProxy", !system_proxy_enabled)),
        "proxy_tun" => Some(("tun", !tun_enabled)),
        _ => None,
    }
}

#[allow(dead_code)]
pub(crate) fn should_destroy_window_for_startup_background(lightweight: bool) -> bool {
    lightweight
}

#[allow(dead_code)]
pub(crate) fn keep_alive_for_close_behavior(behavior: TrayCloseBehavior) -> bool {
    matches!(
        behavior,
        TrayCloseBehavior::Lightweight | TrayCloseBehavior::Hide
    )
}

/// 隐藏窗口后的状态突变（纯逻辑）。
pub(crate) fn apply_hide_window_state(state: &mut TrayRuntimeState) {
    state.set_window_visible(false);
    state.allow_app_exit = false;
}

/// 轻量驻留销毁窗口前的状态突变（纯逻辑）。
pub(crate) fn apply_destroy_window_state(state: &mut TrayRuntimeState) {
    let route = state.last_visible_route.clone();
    state.set_window_visible(false);
    state.keep_alive_without_windows = true;
    state.allow_app_exit = false;
    state.set_pending_restore_route(&route);
}

/// 请求退出时的状态突变（纯逻辑）。
pub(crate) fn apply_exit_request_state(state: &mut TrayRuntimeState) {
    state.allow_app_exit = true;
    state.keep_alive_without_windows = false;
}

#[allow(dead_code)]
pub(crate) fn close_window_action(behavior: TrayCloseBehavior) -> &'static str {
    match behavior {
        TrayCloseBehavior::Hide => "hide",
        TrayCloseBehavior::Lightweight => "destroy",
    }
}

/// 解析内核命令 JSON 是否 success（托盘重启/代理切换共用）。
pub(crate) fn kernel_json_command_ok(
    result: &serde_json::Value,
    default_err: &str,
) -> Result<(), String> {
    if result
        .get("success")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        Ok(())
    } else {
        Err(result
            .get("message")
            .and_then(|value| value.as_str())
            .unwrap_or(default_err)
            .to_string())
    }
}

/// TUN 开启前的提权门控结果（纯逻辑，平台参数由调用方注入）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TunEnablePrivilegeGate {
    /// 已具备提权条件，继续 apply+restart
    Proceed,
    /// 需要前端弹窗补密码/管理员
    QueueForFrontend,
    /// 平台不支持
    Unsupported,
}

#[allow(dead_code)]
pub(crate) fn classify_tun_enable_privilege_windows(is_admin: bool) -> TunEnablePrivilegeGate {
    if is_admin {
        TunEnablePrivilegeGate::Proceed
    } else {
        TunEnablePrivilegeGate::QueueForFrontend
    }
}

/// Linux/macOS：sudo 不支持 → Unsupported；无已存密码 → Queue；否则 Proceed。
pub(crate) fn classify_tun_enable_privilege_unix(
    sudo_supported: bool,
    has_saved_password: bool,
) -> TunEnablePrivilegeGate {
    if !sudo_supported {
        TunEnablePrivilegeGate::Unsupported
    } else if !has_saved_password {
        TunEnablePrivilegeGate::QueueForFrontend
    } else {
        TunEnablePrivilegeGate::Proceed
    }
}

/// TUN 重启失败后是否因 sudo 密码问题应转前端弹窗（纯逻辑）。
pub(crate) fn should_queue_tun_after_sudo_restart_error(message: &str) -> bool {
    message.contains(crate::app::system::sudo_service::SUDO_PASSWORD_REQUIRED)
        || message.contains(crate::app::system::sudo_service::SUDO_PASSWORD_INVALID)
}

/// 是否应立即刷新托盘（状态变更或强制）（纯逻辑）。
pub(crate) fn should_refresh_tray_after_backend_sync(changed: bool, force_refresh: bool) -> bool {
    changed || force_refresh
}

/// 托盘菜单事件 → 派发动作（纯逻辑，不含 OS 调用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TrayMenuDispatch {
    ShowWindow,
    KernelRestart,
    ProxyToggle {
        feature: &'static str,
        enabled: bool,
    },
    Quit,
    Unknown,
}

pub(crate) fn dispatch_tray_menu(
    menu_id: &str,
    system_proxy_enabled: bool,
    tun_enabled: bool,
) -> TrayMenuDispatch {
    match classify_tray_menu_id(menu_id) {
        "show_window" => TrayMenuDispatch::ShowWindow,
        "kernel_restart" => TrayMenuDispatch::KernelRestart,
        "proxy_system" => TrayMenuDispatch::ProxyToggle {
            feature: "systemProxy",
            enabled: !system_proxy_enabled,
        },
        "proxy_tun" => TrayMenuDispatch::ProxyToggle {
            feature: "tun",
            enabled: !tun_enabled,
        },
        "quit" => TrayMenuDispatch::Quit,
        _ => TrayMenuDispatch::Unknown,
    }
}

fn build_tray_menu(
    app: &AppHandle,
    state: &TrayRuntimeState,
    text: &TrayText,
) -> Result<tauri::menu::Menu<tauri::Wry>, String> {
    let labels = build_tray_menu_labels(state, text);
    let primary_window_action = labels.primary_window_action;

    let show_window_item = MenuItemBuilder::with_id(menu_ids::SHOW_WINDOW, primary_window_action)
        .build(app)
        .map_err(|e| format!("创建托盘菜单项失败: {}", e))?;

    let kernel_status_item =
        MenuItemBuilder::with_id(menu_ids::KERNEL_STATUS, labels.kernel_status)
            .enabled(false)
            .build(app)
            .map_err(|e| format!("创建内核状态菜单项失败: {}", e))?;

    let kernel_restart_item =
        MenuItemBuilder::with_id(menu_ids::KERNEL_RESTART, text.restart_kernel)
            .enabled(labels.kernel_restart_enabled)
            .build(app)
            .map_err(|e| format!("创建重启菜单项失败: {}", e))?;

    let kernel_submenu = SubmenuBuilder::with_id(app, menu_ids::KERNEL_SUBMENU, text.kernel_menu)
        .item(&kernel_status_item)
        .item(&kernel_restart_item)
        .build()
        .map_err(|e| format!("创建内核子菜单失败: {}", e))?;

    let current_mode_item = MenuItemBuilder::with_id(menu_ids::PROXY_CURRENT, labels.current_mode)
        .enabled(false)
        .build(app)
        .map_err(|e| format!("创建当前模式菜单项失败: {}", e))?;

    let proxy_system_item = CheckMenuItemBuilder::with_id(menu_ids::PROXY_SYSTEM, text.mode_system)
        .checked(labels.system_checked)
        .enabled(true)
        .build(app)
        .map_err(|e| format!("创建系统代理菜单项失败: {}", e))?;

    let proxy_tun_item = CheckMenuItemBuilder::with_id(menu_ids::PROXY_TUN, text.mode_tun)
        .checked(labels.tun_checked)
        .enabled(true)
        .build(app)
        .map_err(|e| format!("创建TUN菜单项失败: {}", e))?;

    let proxy_submenu = SubmenuBuilder::with_id(app, menu_ids::PROXY_SUBMENU, text.proxy_controls)
        .item(&current_mode_item)
        .separator()
        .item(&proxy_system_item)
        .item(&proxy_tun_item)
        .build()
        .map_err(|e| format!("创建代理模式子菜单失败: {}", e))?;

    let quit_item = MenuItemBuilder::with_id(menu_ids::QUIT, text.quit)
        .build(app)
        .map_err(|e| format!("创建退出菜单项失败: {}", e))?;

    MenuBuilder::new(app)
        .items(&[
            &show_window_item,
            &kernel_submenu,
            &proxy_submenu,
            &quit_item,
        ])
        .build()
        .map_err(|e| format!("创建托盘菜单失败: {}", e))
}

fn handle_proxy_toggle_menu_event(app: &AppHandle, feature: &str, enabled: bool) {
    let app_handle = app.clone();
    let feature = feature.to_string();
    tauri::async_runtime::spawn(async move {
        let result = if feature == "systemProxy" {
            apply_system_proxy_toggle_from_tray(&app_handle, enabled).await
        } else {
            apply_tun_toggle_from_tray(&app_handle, enabled).await
        };

        if let Err(err) = result {
            warn!("托盘代理切换失败: {}", err);
        }
    });
}

fn handle_menu_event(app: &AppHandle, menu_id: &str) {
    let (system_proxy_enabled, tun_enabled) =
        with_state_read(|state| (state.system_proxy_enabled, state.tun_enabled));
    match dispatch_tray_menu(menu_id, system_proxy_enabled, tun_enabled) {
        TrayMenuDispatch::ShowWindow => {
            if let Err(err) = show_main_window(app, true) {
                warn!("托盘显示窗口失败: {}", err);
            }
        }
        TrayMenuDispatch::KernelRestart => {
            let app_handle = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) = restart_kernel_from_tray(&app_handle).await {
                    warn!("托盘重启内核失败: {}", err);
                }
            });
        }
        TrayMenuDispatch::ProxyToggle { feature, enabled } => {
            handle_proxy_toggle_menu_event(app, feature, enabled)
        }
        TrayMenuDispatch::Quit => {
            if let Err(err) = request_app_exit(app) {
                warn!("托盘退出流程失败: {}", err);
            }
        }
        TrayMenuDispatch::Unknown => {
            debug!("忽略未处理的托盘菜单事件: {}", menu_id);
        }
    }
}

fn handle_tray_icon_event(tray: &tauri::tray::TrayIcon, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        if let Err(err) = show_main_window(tray.app_handle(), true) {
            warn!("托盘左键恢复窗口失败: {}", err);
        }
    }
}

fn create_or_replace_tray_icon(app: &AppHandle, state: &TrayRuntimeState) -> Result<(), String> {
    if app.remove_tray_by_id(TRAY_ICON_ID).is_some() {
        info!("已移除旧托盘实例，准备重建");
    }

    let text = tray_text_for_locale(&state.locale);
    let menu = build_tray_menu(app, state, &text)?;
    let tooltip = compose_tooltip(state, &text);
    let icon = resolve_tray_icon(app, state);

    let mut builder = TrayIconBuilder::with_id(TRAY_ICON_ID)
        .menu(&menu)
        .tooltip(&tooltip)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let menu_id = event.id().as_ref().to_string();
            handle_menu_event(app, &menu_id);
        })
        .on_tray_icon_event(|tray, event| {
            handle_tray_icon_event(tray, event);
        });

    if let Some(icon) = icon {
        builder = builder.icon(icon);
    }

    builder
        .build(app)
        .map(|_| ())
        .map_err(|e| format!("创建托盘图标失败: {}", e))
}

pub fn init_tray(app: &AppHandle) -> Result<(), String> {
    let state = with_state_read(|state| state.clone());
    create_or_replace_tray_icon(app, &state)
}

pub fn refresh_tray(app: &AppHandle) -> Result<(), String> {
    let state = with_state_read(|state| state.clone());
    let text = tray_text_for_locale(&state.locale);
    let menu = build_tray_menu(app, &state, &text)?;
    let tooltip = compose_tooltip(&state, &text);
    let icon = resolve_tray_icon(app, &state);

    if let Some(tray) = app.tray_by_id(TRAY_ICON_ID) {
        if let Err(err) = tray.set_menu(Some(menu)) {
            warn!("更新托盘菜单失败，尝试重建托盘: {}", err);
            return create_or_replace_tray_icon(app, &state);
        }
        if let Err(err) = tray.set_tooltip(Some(tooltip.as_str())) {
            debug!("更新托盘提示失败（可忽略的平台差异）: {}", err);
        }
        if let Err(err) = tray.set_icon(icon) {
            warn!("更新托盘图标失败，尝试重建托盘: {}", err);
            return create_or_replace_tray_icon(app, &state);
        }
        return Ok(());
    }

    info!("未找到托盘实例，尝试重新创建");
    create_or_replace_tray_icon(app, &state)
}

pub fn sync_tray_state(app: &AppHandle, payload: TrayRuntimeStateInput) -> Result<(), String> {
    let changed = with_state_write(|state| state.apply_sync_payload(payload));
    if !changed {
        return Ok(());
    }
    refresh_tray(app)
}

pub fn set_last_visible_route(path: &str) {
    with_state_write(|state| {
        state.set_last_visible_route(path);
    });
}

pub fn consume_pending_proxy_toggle() -> Option<TrayToggleProxyFeaturePayload> {
    with_state_write(|state| state.take_pending_proxy_toggle())
}

pub fn apply_startup_preferences(close_behavior: TrayCloseBehavior, window_visible: bool) {
    with_state_write(|state| {
        state.close_behavior = close_behavior;
        state.window_visible = window_visible;
        state.keep_alive_without_windows = false;
        state.allow_app_exit = false;
    });
}

fn create_main_window(app: &AppHandle) -> Result<(), String> {
    let window_config = app
        .config()
        .app
        .windows
        .iter()
        .find(|config| config.label == "main")
        .cloned()
        .ok_or_else(|| "未找到主窗口配置".to_string())?;
    let app_handle = app.clone();

    std::thread::spawn(move || {
        WebviewWindowBuilder::from_config(&app_handle, &window_config)
            .map_err(|e| format!("创建主窗口构建器失败: {}", e))?
            .build()
            .map(|_| ())
            .map_err(|e| format!("重建主窗口失败: {}", e))
    })
    .join()
    .map_err(|_| "重建主窗口线程异常退出".to_string())?
}

fn ensure_main_window(app: &AppHandle) -> Result<(WebviewWindow, bool), String> {
    if let Some(window) = app.get_webview_window("main") {
        return Ok((window, false));
    }

    create_main_window(app)?;
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "重建主窗口后未找到实例".to_string())?;
    Ok((window, true))
}

pub fn show_main_window(app: &AppHandle, emit_events: bool) -> Result<(), String> {
    let (main_window, recreated) = ensure_main_window(app)?;

    let _ = main_window.unminimize();
    main_window
        .show()
        .map_err(|e| format!("显示主窗口失败: {}", e))?;
    main_window
        .set_focus()
        .map_err(|e| format!("聚焦主窗口失败: {}", e))?;

    with_state_write(|state| {
        state.set_window_visible(true);
        state.keep_alive_without_windows = false;
        state.allow_app_exit = false;
        if !recreated {
            state.pending_restore_route = None;
        }
    });

    if emit_events {
        let route = with_state_read(|state| state.last_visible_route.clone());
        let route = resolve_navigate_route(&route);

        let _ = app.emit(events::ACTION_SHOW_WINDOW, ());
        if should_queue_restore_route_after_show(recreated) {
            with_state_write(|state| {
                state.set_pending_restore_route(&route);
            });
        } else {
            let _ = app.emit(
                events::ACTION_NAVIGATE_LAST_ROUTE,
                TrayNavigatePayload { path: route },
            );
        }
    }

    Ok(())
}

pub fn hide_main_window(app: &AppHandle, emit_events: bool) -> Result<(), String> {
    let main_window = app
        .get_webview_window("main")
        .ok_or_else(|| "未找到主窗口".to_string())?;

    main_window
        .hide()
        .map_err(|e| format!("隐藏主窗口失败: {}", e))?;

    with_state_write(apply_hide_window_state);

    if emit_events {
        let _ = app.emit(events::ACTION_HIDE_WINDOW, ());
    }

    Ok(())
}

pub fn close_main_window(app: &AppHandle) -> Result<(), String> {
    match with_state_read(|state| state.close_behavior) {
        TrayCloseBehavior::Hide => hide_main_window(app, true),
        TrayCloseBehavior::Lightweight => destroy_main_window_for_tray(app),
    }
}

pub fn enter_startup_background_mode(app: &AppHandle, lightweight: bool) -> Result<(), String> {
    match startup_background_action(lightweight) {
        "hide" => hide_main_window(app, false),
        _ => destroy_main_window_for_tray(app),
    }
}

pub fn consume_pending_restore_route() -> Option<TrayNavigatePayload> {
    with_state_write(|state| {
        state
            .take_pending_restore_route()
            .map(|path| TrayNavigatePayload { path })
    })
}

pub fn should_prevent_exit() -> bool {
    with_state_read(|state| state.keep_alive_without_windows && !state.allow_app_exit)
}

fn destroy_main_window_for_tray(app: &AppHandle) -> Result<(), String> {
    let main_window = app
        .get_webview_window("main")
        .ok_or_else(|| "未找到主窗口".to_string())?;

    with_state_write(apply_destroy_window_state);

    if let Err(err) = main_window.destroy() {
        with_state_write(|state| {
            state.keep_alive_without_windows = false;
        });
        return Err(format!("销毁主窗口失败: {}", err));
    }

    Ok(())
}

pub fn request_app_exit(app: &AppHandle) -> Result<(), String> {
    let _ = app.emit(events::ACTION_EXIT_REQUESTED, ());
    with_state_write(apply_exit_request_state);

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        match tokio::time::timeout(
            Duration::from_secs(4),
            crate::app::core::kernel_service::runtime::orchestrated_stop_kernel(app_handle.clone()),
        )
        .await
        {
            Ok(Ok(result)) => info!("退出前停止内核完成: {}", result),
            Ok(Err(err)) => warn!("退出前停止内核失败，继续退出: {}", err),
            Err(_) => warn!("退出前停止内核超时，继续退出"),
        }

        app_handle.exit(0);
    });

    Ok(())
}

async fn restart_kernel_from_tray(app: &AppHandle) -> Result<(), String> {
    let result =
        kernel_restart_fast(app.clone(), None, None, None, None, None, None, None, None).await?;

    kernel_json_command_ok(&result, "重启内核失败")?;
    refresh_runtime_state_from_backend(app, false).await
}

async fn apply_system_proxy_toggle_from_tray(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let mut app_config =
        crate::app::storage::enhanced_storage_service::db_get_app_config_internal(app).await?;
    app_config.system_proxy_enabled = enabled;
    app_config.proxy_mode =
        derive_proxy_mode(app_config.system_proxy_enabled, app_config.tun_enabled);
    db_save_app_config_internal(app_config, app).await?;
    apply_runtime_change(
        app,
        RuntimeChange::ProxySettingsChanged,
        RuntimeApplyOptions::new("tray-system-proxy-toggle"),
    )
    .await?;
    refresh_runtime_state_from_backend(app, false).await
}

async fn apply_tun_toggle_from_tray(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let runtime_active = is_kernel_running().await.unwrap_or(false);

    #[cfg(target_os = "windows")]
    if enabled && runtime_active {
        match classify_tun_enable_privilege_windows(
            crate::app::system::system_service::check_admin(),
        ) {
            TunEnablePrivilegeGate::QueueForFrontend => {
                return queue_proxy_toggle_for_frontend(app, "tun", true);
            }
            TunEnablePrivilegeGate::Unsupported => {
                return Err("当前平台暂不支持该操作".to_string());
            }
            TunEnablePrivilegeGate::Proceed => {}
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if enabled && runtime_active {
        let sudo_status =
            crate::app::system::sudo_service::sudo_password_status(app.clone()).await?;
        match classify_tun_enable_privilege_unix(sudo_status.supported, sudo_status.has_saved) {
            TunEnablePrivilegeGate::Unsupported => {
                return Err("当前平台暂不支持该操作".to_string());
            }
            TunEnablePrivilegeGate::QueueForFrontend => {
                return queue_proxy_toggle_for_frontend(app, "tun", true);
            }
            TunEnablePrivilegeGate::Proceed => {}
        }
    }

    let mut app_config =
        crate::app::storage::enhanced_storage_service::db_get_app_config_internal(app).await?;
    app_config.tun_enabled = enabled;
    app_config.proxy_mode =
        derive_proxy_mode(app_config.system_proxy_enabled, app_config.tun_enabled);
    db_save_app_config_internal(app_config, app).await?;

    if let Err(message) = apply_runtime_change(
        app,
        RuntimeChange::AppConfigUpdated,
        RuntimeApplyOptions::new("tray-tun-toggle")
            .patch_active_config(true)
            .restart_if_running(true),
    )
    .await
    {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if enabled && should_queue_tun_after_sudo_restart_error(&message) {
            return queue_proxy_toggle_for_frontend(app, "tun", true);
        }
        return Err(message);
    }

    refresh_runtime_state_from_backend(app, false).await
}

pub(crate) fn derive_proxy_mode(system_proxy_enabled: bool, tun_enabled: bool) -> String {
    if tun_enabled {
        "tun".to_string()
    } else if system_proxy_enabled {
        "system".to_string()
    } else {
        "manual".to_string()
    }
}

/// show_main_window 使用的导航路径（空则回退 `/`）。
pub(crate) fn resolve_navigate_route(route: &str) -> String {
    if route.trim().is_empty() {
        "/".to_string()
    } else {
        route.to_string()
    }
}

/// 重建窗口后是否应写入 pending_restore_route（纯逻辑）。
pub(crate) fn should_queue_restore_route_after_show(recreated: bool) -> bool {
    recreated
}

/// 无主窗口时排队代理切换；有主窗口时直接 emit（纯逻辑决策）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProxyToggleDelivery {
    EmitToWindow,
    QueuePending,
}

pub(crate) fn proxy_toggle_delivery(has_main_window: bool) -> ProxyToggleDelivery {
    if has_main_window {
        ProxyToggleDelivery::EmitToWindow
    } else {
        ProxyToggleDelivery::QueuePending
    }
}

/// enter_startup_background_mode 动作（纯逻辑）。
pub(crate) fn startup_background_action(lightweight: bool) -> &'static str {
    if lightweight {
        "destroy"
    } else {
        "hide"
    }
}

fn apply_backend_runtime_snapshot(
    state: &mut TrayRuntimeState,
    app_config: &AppConfig,
    kernel_running: bool,
) -> bool {
    let mut changed = false;
    let close_behavior = TrayCloseBehavior::from_raw(&app_config.tray_close_behavior);

    if state.system_proxy_enabled != app_config.system_proxy_enabled {
        state.system_proxy_enabled = app_config.system_proxy_enabled;
        changed = true;
    }
    if state.tun_enabled != app_config.tun_enabled {
        state.tun_enabled = app_config.tun_enabled;
        changed = true;
    }
    if state.kernel_running != kernel_running {
        state.kernel_running = kernel_running;
        changed = true;
    }
    if state.close_behavior != close_behavior {
        state.close_behavior = close_behavior;
        changed = true;
    }

    changed
}

pub async fn refresh_runtime_state_from_backend(
    app: &AppHandle,
    force_refresh: bool,
) -> Result<(), String> {
    let app_config =
        crate::app::storage::enhanced_storage_service::db_get_app_config_internal(app).await?;
    let kernel_running = is_kernel_running().await.unwrap_or(false);
    let changed = with_state_write(|state| {
        apply_backend_runtime_snapshot(state, &app_config, kernel_running)
    });

    if should_refresh_tray_after_backend_sync(changed, force_refresh) {
        refresh_tray(app)?;
    }

    let _ = app.emit(events::RUNTIME_STATE_UPDATED, ());
    Ok(())
}

fn queue_proxy_toggle_for_frontend(
    app: &AppHandle,
    feature: &str,
    enabled: bool,
) -> Result<(), String> {
    let payload = TrayToggleProxyFeaturePayload {
        feature: feature.to_string(),
        enabled,
    };

    match proxy_toggle_delivery(app.get_webview_window("main").is_some()) {
        ProxyToggleDelivery::EmitToWindow => {
            show_main_window(app, true)?;
            app.emit(events::ACTION_SWITCH_PROXY_MODE, payload)
                .map_err(|e| format!("发送托盘代理切换事件失败: {}", e))?;
            Ok(())
        }
        ProxyToggleDelivery::QueuePending => {
            with_state_write(|state| {
                state.set_pending_proxy_toggle(payload);
            });
            show_main_window(app, true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::storage::state_model::AppConfig;

    fn sample_app_config() -> AppConfig {
        AppConfig {
            system_proxy_enabled: true,
            tun_enabled: false,
            tray_close_behavior: "lightweight".to_string(),
            ..AppConfig::default()
        }
    }

    #[test]
    fn backend_runtime_sync_updates_proxy_related_fields() {
        let mut state = TrayRuntimeState::default();
        let config = sample_app_config();

        let changed = apply_backend_runtime_snapshot(&mut state, &config, true);

        assert!(changed);
        assert!(state.system_proxy_enabled);
        assert!(!state.tun_enabled);
        assert!(state.kernel_running);
        assert_eq!(state.close_behavior, TrayCloseBehavior::Lightweight);
    }

    #[test]
    fn backend_runtime_sync_preserves_window_lifecycle_fields() {
        let mut state = TrayRuntimeState {
            window_visible: false,
            last_visible_route: "/settings".to_string(),
            pending_restore_route: Some("/settings".to_string()),
            keep_alive_without_windows: true,
            allow_app_exit: true,
            ..TrayRuntimeState::default()
        };
        let config = AppConfig {
            system_proxy_enabled: false,
            tun_enabled: true,
            tray_close_behavior: "hide".to_string(),
            ..AppConfig::default()
        };

        let changed = apply_backend_runtime_snapshot(&mut state, &config, false);

        assert!(changed);
        assert!(!state.window_visible);
        assert_eq!(state.last_visible_route, "/settings");
        assert_eq!(state.pending_restore_route.as_deref(), Some("/settings"));
        assert!(state.keep_alive_without_windows);
        assert!(state.allow_app_exit);
        assert!(state.tun_enabled);
        assert_eq!(state.close_behavior, TrayCloseBehavior::Hide);
    }

    #[test]
    fn backend_runtime_sync_no_change_when_identical() {
        let mut state = TrayRuntimeState {
            system_proxy_enabled: true,
            tun_enabled: false,
            kernel_running: true,
            close_behavior: TrayCloseBehavior::Lightweight,
            ..TrayRuntimeState::default()
        };
        let config = sample_app_config();
        assert!(!apply_backend_runtime_snapshot(&mut state, &config, true));
    }

    #[test]
    fn tray_locale_and_mode_summary_and_tooltip() {
        let zh = tray_text_for_locale("zh-CN");
        let en = tray_text_for_locale("en-US");
        let ja = tray_text_for_locale("ja-JP");
        let ru = tray_text_for_locale("ru-RU");
        let fallback = tray_text_for_locale("fr-FR");
        assert_eq!(zh.show_window, TRAY_TEXT_ZH_CN.show_window);
        assert_eq!(en.show_window, TRAY_TEXT_EN_US.show_window);
        assert_eq!(ja.show_window, TRAY_TEXT_JA_JP.show_window);
        assert_eq!(ru.show_window, TRAY_TEXT_RU_RU.show_window);
        assert_eq!(fallback.show_window, TRAY_TEXT_EN_US.show_window);

        let mut state = TrayRuntimeState::default();
        state.system_proxy_enabled = true;
        assert_eq!(mode_summary_text(&state, &en), en.mode_system);
        state.tun_enabled = true;
        assert!(mode_summary_text(&state, &en).contains(en.mode_tun));
        state.system_proxy_enabled = false;
        assert_eq!(mode_summary_text(&state, &en), en.mode_tun);
        state.tun_enabled = false;
        assert_eq!(mode_summary_text(&state, &en), en.mode_manual);

        state.kernel_running = true;
        state.active_subscription_name = Some("My Sub".into());
        let tip = compose_tooltip(&state, &en);
        assert!(tip.contains("sing-box-window"));
        assert!(tip.contains(en.status_running));
        assert!(tip.contains("My Sub"));
    }

    #[test]
    fn derive_proxy_mode_priority() {
        assert_eq!(derive_proxy_mode(true, true), "tun");
        assert_eq!(derive_proxy_mode(true, false), "system");
        assert_eq!(derive_proxy_mode(false, false), "manual");
    }

    #[test]
    fn tray_state_mutations_without_app_handle() {
        set_last_visible_route("/proxies");
        assert_eq!(
            with_state_read(|s| s.last_visible_route.clone()),
            "/proxies"
        );

        apply_startup_preferences(TrayCloseBehavior::Lightweight, false);
        assert!(with_state_read(|s| {
            s.close_behavior == TrayCloseBehavior::Lightweight && !s.window_visible
        }));

        with_state_write(|s| {
            s.set_pending_proxy_toggle(TrayToggleProxyFeaturePayload {
                feature: "system".into(),
                enabled: true,
            });
            s.set_pending_restore_route("/home");
        });

        let toggle = consume_pending_proxy_toggle();
        assert_eq!(toggle.as_ref().map(|t| t.feature.as_str()), Some("system"));
        assert!(consume_pending_proxy_toggle().is_none());

        let route = consume_pending_restore_route();
        assert_eq!(route.as_ref().map(|r| r.path.as_str()), Some("/home"));
        assert!(consume_pending_restore_route().is_none());

        // keep_alive && !allow_app_exit → 阻止退出
        with_state_write(|s| {
            s.keep_alive_without_windows = true;
            s.allow_app_exit = false;
        });
        assert!(should_prevent_exit());

        with_state_write(|s| s.allow_app_exit = true);
        assert!(!should_prevent_exit());
    }

    #[test]
    fn tray_menu_labels_and_menu_id_classification() {
        let text = tray_text_for_locale("en-US");
        let mut state = TrayRuntimeState::default();
        state.window_visible = false;
        state.kernel_running = true;
        state.system_proxy_enabled = true;
        state.tun_enabled = false;
        let labels = build_tray_menu_labels(&state, &text);
        assert_eq!(labels.primary_window_action, text.rebuild_window);
        assert_eq!(labels.kernel_status, text.status_running);
        assert!(labels.kernel_restart_enabled);
        assert!(labels.system_checked);
        assert!(!labels.tun_checked);
        assert!(labels.current_mode.contains(text.current_status));

        assert_eq!(classify_tray_menu_id(menu_ids::SHOW_WINDOW), "show_window");
        assert_eq!(
            classify_tray_menu_id(menu_ids::KERNEL_RESTART),
            "kernel_restart"
        );
        assert_eq!(
            classify_tray_menu_id(menu_ids::PROXY_SYSTEM),
            "proxy_system"
        );
        assert_eq!(classify_tray_menu_id(menu_ids::PROXY_TUN), "proxy_tun");
        assert_eq!(classify_tray_menu_id(menu_ids::QUIT), "quit");
        assert_eq!(classify_tray_menu_id("nope"), "unknown");
    }

    #[test]
    fn tray_pure_toggle_and_close_helpers() {
        assert_eq!(
            next_proxy_toggle_enabled(menu_ids::PROXY_SYSTEM, true, false),
            Some(("systemProxy", false))
        );
        assert_eq!(
            next_proxy_toggle_enabled(menu_ids::PROXY_TUN, false, false),
            Some(("tun", true))
        );
        assert!(next_proxy_toggle_enabled(menu_ids::QUIT, false, false).is_none());
        assert!(should_destroy_window_for_startup_background(true));
        assert!(!should_destroy_window_for_startup_background(false));
        assert!(keep_alive_for_close_behavior(TrayCloseBehavior::Hide));
        assert!(keep_alive_for_close_behavior(
            TrayCloseBehavior::Lightweight
        ));
    }

    #[test]
    fn tray_state_mutation_helpers() {
        let mut s = TrayRuntimeState::default();
        s.window_visible = true;
        s.allow_app_exit = true;
        apply_hide_window_state(&mut s);
        assert!(!s.window_visible);
        assert!(!s.allow_app_exit);

        s.last_visible_route = "/home".into();
        apply_destroy_window_state(&mut s);
        assert!(s.keep_alive_without_windows);
        assert_eq!(s.pending_restore_route.as_deref(), Some("/home"));

        apply_exit_request_state(&mut s);
        assert!(s.allow_app_exit);
        assert!(!s.keep_alive_without_windows);

        assert_eq!(close_window_action(TrayCloseBehavior::Hide), "hide");
        assert_eq!(
            close_window_action(TrayCloseBehavior::Lightweight),
            "destroy"
        );
    }

    #[test]
    fn tray_pure_kernel_json_and_tun_privilege_gates() {
        let ok = serde_json::json!({"success": true, "message": "ok"});
        assert!(kernel_json_command_ok(&ok, "x").is_ok());
        let bad = serde_json::json!({"success": false, "message": "fail-msg"});
        assert_eq!(
            kernel_json_command_ok(&bad, "default").unwrap_err(),
            "fail-msg"
        );
        let missing = serde_json::json!({});
        assert_eq!(
            kernel_json_command_ok(&missing, "fallback").unwrap_err(),
            "fallback"
        );

        assert_eq!(
            classify_tun_enable_privilege_windows(true),
            TunEnablePrivilegeGate::Proceed
        );
        assert_eq!(
            classify_tun_enable_privilege_windows(false),
            TunEnablePrivilegeGate::QueueForFrontend
        );
        assert_eq!(
            classify_tun_enable_privilege_unix(false, false),
            TunEnablePrivilegeGate::Unsupported
        );
        assert_eq!(
            classify_tun_enable_privilege_unix(true, false),
            TunEnablePrivilegeGate::QueueForFrontend
        );
        assert_eq!(
            classify_tun_enable_privilege_unix(true, true),
            TunEnablePrivilegeGate::Proceed
        );

        assert!(should_queue_tun_after_sudo_restart_error(
            crate::app::system::sudo_service::SUDO_PASSWORD_REQUIRED
        ));
        assert!(should_queue_tun_after_sudo_restart_error(
            crate::app::system::sudo_service::SUDO_PASSWORD_INVALID
        ));
        assert!(!should_queue_tun_after_sudo_restart_error("other error"));

        assert!(should_refresh_tray_after_backend_sync(true, false));
        assert!(should_refresh_tray_after_backend_sync(false, true));
        assert!(!should_refresh_tray_after_backend_sync(false, false));

        assert_eq!(
            dispatch_tray_menu(menu_ids::SHOW_WINDOW, false, false),
            TrayMenuDispatch::ShowWindow
        );
        assert_eq!(
            dispatch_tray_menu(menu_ids::KERNEL_RESTART, false, false),
            TrayMenuDispatch::KernelRestart
        );
        assert_eq!(
            dispatch_tray_menu(menu_ids::PROXY_SYSTEM, true, false),
            TrayMenuDispatch::ProxyToggle {
                feature: "systemProxy",
                enabled: false,
            }
        );
        assert_eq!(
            dispatch_tray_menu(menu_ids::PROXY_TUN, false, true),
            TrayMenuDispatch::ProxyToggle {
                feature: "tun",
                enabled: false,
            }
        );
        assert_eq!(
            dispatch_tray_menu(menu_ids::QUIT, false, false),
            TrayMenuDispatch::Quit
        );
        assert_eq!(
            dispatch_tray_menu("unknown-id", false, false),
            TrayMenuDispatch::Unknown
        );
    }

    #[test]
    fn tray_show_route_and_toggle_delivery_helpers() {
        assert_eq!(resolve_navigate_route(""), "/");
        assert_eq!(resolve_navigate_route("   "), "/");
        assert_eq!(resolve_navigate_route("/settings"), "/settings");
        assert!(should_queue_restore_route_after_show(true));
        assert!(!should_queue_restore_route_after_show(false));
        assert_eq!(
            proxy_toggle_delivery(true),
            ProxyToggleDelivery::EmitToWindow
        );
        assert_eq!(
            proxy_toggle_delivery(false),
            ProxyToggleDelivery::QueuePending
        );
        assert_eq!(startup_background_action(true), "destroy");
        assert_eq!(startup_background_action(false), "hide");
    }

    #[test]
    fn tray_menu_labels_window_visible_and_kernel_stopped() {
        let text = tray_text_for_locale("zh-CN");
        let mut state = TrayRuntimeState::default();
        state.window_visible = true;
        state.kernel_running = false;
        let labels = build_tray_menu_labels(&state, &text);
        assert_eq!(labels.primary_window_action, text.show_window);
        assert_eq!(labels.kernel_status, text.status_stopped);
        assert!(!labels.kernel_restart_enabled);
    }

    #[test]
    fn keep_alive_close_behavior_hide_only() {
        // 再次覆盖 close 行为匹配（Lightweight 已在另一测覆盖）
        assert!(!matches!(
            TrayCloseBehavior::from_raw("unknown"),
            TrayCloseBehavior::Lightweight
        ));
        assert_eq!(
            close_window_action(TrayCloseBehavior::from_raw("hide")),
            "hide"
        );
        assert_eq!(
            close_window_action(TrayCloseBehavior::from_raw("lightweight")),
            "destroy"
        );
        // unknown raw → Hide → keep alive
        assert!(keep_alive_for_close_behavior(TrayCloseBehavior::from_raw(
            "other"
        )));
    }
}
