use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeChange {
    AppConfigUpdated,
    ActiveConfigChanged,
    SubscriptionApplied,
    ProxySettingsChanged,
    KernelUpdated,
}

impl RuntimeChange {
    pub fn as_str(self) -> &'static str {
        match self {
            RuntimeChange::AppConfigUpdated => "app_config_updated",
            RuntimeChange::ActiveConfigChanged => "active_config_changed",
            RuntimeChange::SubscriptionApplied => "subscription_applied",
            RuntimeChange::ProxySettingsChanged => "proxy_settings_changed",
            RuntimeChange::KernelUpdated => "kernel_updated",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeApplyOptions {
    pub force_restart: bool,
    pub patch_active_config: bool,
    pub use_original_config_hint: Option<bool>,
    pub reason: String,
}

impl RuntimeApplyOptions {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            ..Default::default()
        }
    }

    pub fn force_restart(mut self, value: bool) -> Self {
        self.force_restart = value;
        self
    }

    pub fn patch_active_config(mut self, value: bool) -> Self {
        self.patch_active_config = value;
        self
    }

    pub fn use_original_config_hint(mut self, value: Option<bool>) -> Self {
        self.use_original_config_hint = value;
        self
    }
}

impl Default for RuntimeApplyOptions {
    fn default() -> Self {
        Self {
            force_restart: false,
            patch_active_config: false,
            use_original_config_hint: None,
            reason: "runtime-change".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeApplyResult {
    pub change: String,
    pub reason: String,
    pub config_patched: bool,
    pub proxy_applied: bool,
    pub auto_manage_state: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeActionPlan {
    pub patch_active_config: bool,
    pub apply_proxy_runtime: bool,
    pub auto_manage_kernel: bool,
}

pub(crate) fn plan_runtime_actions(
    change: RuntimeChange,
    options: &RuntimeApplyOptions,
) -> RuntimeActionPlan {
    RuntimeActionPlan {
        patch_active_config: options.patch_active_config,
        apply_proxy_runtime: matches!(
            change,
            RuntimeChange::SubscriptionApplied | RuntimeChange::ProxySettingsChanged
        ),
        auto_manage_kernel: !matches!(change, RuntimeChange::ProxySettingsChanged),
    }
}

#[cfg(test)]
mod tests {
    use super::{plan_runtime_actions, RuntimeApplyOptions, RuntimeChange};

    #[test]
    fn app_config_update_should_patch_and_auto_manage_when_requested() {
        let options = RuntimeApplyOptions::new("test").patch_active_config(true);
        let plan = plan_runtime_actions(RuntimeChange::AppConfigUpdated, &options);

        assert!(plan.patch_active_config);
        assert!(!plan.apply_proxy_runtime);
        assert!(plan.auto_manage_kernel);
    }

    #[test]
    fn subscription_apply_should_apply_proxy_and_auto_manage() {
        let options = RuntimeApplyOptions::new("test").patch_active_config(true);
        let plan = plan_runtime_actions(RuntimeChange::SubscriptionApplied, &options);

        assert!(plan.patch_active_config);
        assert!(plan.apply_proxy_runtime);
        assert!(plan.auto_manage_kernel);
    }

    #[test]
    fn proxy_settings_change_should_not_auto_manage_kernel() {
        let options = RuntimeApplyOptions::new("test");
        let plan = plan_runtime_actions(RuntimeChange::ProxySettingsChanged, &options);

        assert!(!plan.patch_active_config);
        assert!(plan.apply_proxy_runtime);
        assert!(!plan.auto_manage_kernel);
    }
}
