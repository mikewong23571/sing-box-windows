use super::model::TrayToggleProxyFeaturePayload;
use super::model::{TrayCloseBehavior, TrayProxyMode, TrayRuntimeStateInput};

#[derive(Debug, Clone)]
pub struct TrayRuntimeState {
    pub kernel_running: bool,
    pub system_proxy_enabled: bool,
    pub tun_enabled: bool,
    pub active_subscription_name: Option<String>,
    pub locale: String,
    pub window_visible: bool,
    pub last_visible_route: String,
    pub close_behavior: TrayCloseBehavior,
    pub pending_restore_route: Option<String>,
    pub pending_proxy_toggle: Option<TrayToggleProxyFeaturePayload>,
    pub keep_alive_without_windows: bool,
    pub allow_app_exit: bool,
}

impl Default for TrayRuntimeState {
    fn default() -> Self {
        Self {
            kernel_running: false,
            system_proxy_enabled: false,
            tun_enabled: false,
            active_subscription_name: None,
            locale: "en-US".to_string(),
            window_visible: true,
            last_visible_route: "/".to_string(),
            close_behavior: TrayCloseBehavior::Hide,
            pending_restore_route: None,
            pending_proxy_toggle: None,
            keep_alive_without_windows: false,
            allow_app_exit: false,
        }
    }
}

impl TrayRuntimeState {
    pub fn apply_sync_payload(&mut self, payload: TrayRuntimeStateInput) -> bool {
        let mut changed = false;

        if self.kernel_running != payload.kernel_running {
            self.kernel_running = payload.kernel_running;
            changed = true;
        }

        if self.system_proxy_enabled != payload.system_proxy_enabled {
            self.system_proxy_enabled = payload.system_proxy_enabled;
            changed = true;
        }

        if self.tun_enabled != payload.tun_enabled {
            self.tun_enabled = payload.tun_enabled;
            changed = true;
        }

        let next_name = payload
            .active_subscription_name
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty());
        if self.active_subscription_name != next_name {
            self.active_subscription_name = next_name;
            changed = true;
        }

        let next_locale = normalize_locale(&payload.locale);
        if self.locale != next_locale {
            self.locale = next_locale;
            changed = true;
        }

        if self.window_visible != payload.window_visible {
            self.window_visible = payload.window_visible;
            changed = true;
        }

        if self.close_behavior != payload.close_behavior {
            self.close_behavior = payload.close_behavior;
            changed = true;
        }

        changed
    }

    pub fn set_last_visible_route(&mut self, path: &str) -> bool {
        let normalized = normalize_route(path);
        if normalized == self.last_visible_route {
            return false;
        }
        self.last_visible_route = normalized;
        true
    }

    pub fn set_window_visible(&mut self, visible: bool) -> bool {
        if self.window_visible == visible {
            return false;
        }
        self.window_visible = visible;
        true
    }

    pub fn display_mode(&self) -> TrayProxyMode {
        if self.tun_enabled {
            TrayProxyMode::Tun
        } else if self.system_proxy_enabled {
            TrayProxyMode::System
        } else {
            TrayProxyMode::Manual
        }
    }

    pub fn set_pending_restore_route(&mut self, path: &str) -> bool {
        let normalized = normalize_route(path);
        let next = Some(normalized);
        if self.pending_restore_route == next {
            return false;
        }
        self.pending_restore_route = next;
        true
    }

    pub fn take_pending_restore_route(&mut self) -> Option<String> {
        self.pending_restore_route.take()
    }

    pub fn set_pending_proxy_toggle(&mut self, payload: TrayToggleProxyFeaturePayload) -> bool {
        let changed = self
            .pending_proxy_toggle
            .as_ref()
            .map(|existing| {
                existing.feature != payload.feature || existing.enabled != payload.enabled
            })
            .unwrap_or(true);
        self.pending_proxy_toggle = Some(payload);
        changed
    }

    pub fn take_pending_proxy_toggle(&mut self) -> Option<TrayToggleProxyFeaturePayload> {
        self.pending_proxy_toggle.take()
    }
}

pub(crate) fn normalize_locale(locale: &str) -> String {
    let trimmed = locale.trim();
    if trimmed.is_empty() {
        "en-US".to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn normalize_route(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/blank" {
        "/".to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{}", trimmed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tray::model::{
        TrayCloseBehavior, TrayProxyMode, TrayRuntimeStateInput, TrayToggleProxyFeaturePayload,
    };

    #[test]
    fn normalize_locale_and_route() {
        assert_eq!(normalize_locale(""), "en-US");
        assert_eq!(normalize_locale("  zh-CN  "), "zh-CN");
        assert_eq!(normalize_route(""), "/");
        assert_eq!(normalize_route("/blank"), "/");
        assert_eq!(normalize_route("settings"), "/settings");
        assert_eq!(normalize_route("/proxies"), "/proxies");
    }

    #[test]
    fn tray_runtime_state_apply_and_pending() {
        let mut state = TrayRuntimeState::default();
        let payload = TrayRuntimeStateInput {
            kernel_running: true,
            system_proxy_enabled: true,
            tun_enabled: false,
            active_subscription_name: Some("  sub  ".into()),
            locale: "ja-JP".into(),
            window_visible: false,
            close_behavior: TrayCloseBehavior::Lightweight,
        };
        assert!(state.apply_sync_payload(payload));
        assert!(state.kernel_running);
        assert_eq!(state.active_subscription_name.as_deref(), Some("sub"));
        assert_eq!(state.display_mode(), TrayProxyMode::System);

        assert!(state.set_last_visible_route("home"));
        assert!(!state.set_last_visible_route("/home"));
        assert!(state.set_window_visible(true));
        assert!(!state.set_window_visible(true));

        assert!(state.set_pending_restore_route("/x"));
        assert_eq!(state.take_pending_restore_route().as_deref(), Some("/x"));
        assert!(state.take_pending_restore_route().is_none());

        let toggle = TrayToggleProxyFeaturePayload {
            feature: "tun".into(),
            enabled: true,
        };
        assert!(state.set_pending_proxy_toggle(toggle.clone()));
        assert!(!state.set_pending_proxy_toggle(toggle));
        assert_eq!(
            state.take_pending_proxy_toggle().map(|t| t.feature),
            Some("tun".into())
        );
    }

    #[test]
    fn display_mode_priority_tun_over_system() {
        let mut state = TrayRuntimeState::default();
        state.system_proxy_enabled = true;
        state.tun_enabled = true;
        assert_eq!(state.display_mode(), TrayProxyMode::Tun);
    }
}
