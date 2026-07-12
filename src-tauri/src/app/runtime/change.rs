use crate::app::core::kernel_service::state::KernelChangeImpact;
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
    pub kernel_impact: KernelChangeImpact,
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

    pub fn restart_if_running(mut self, value: bool) -> Self {
        self.kernel_impact = if value {
            KernelChangeImpact::RestartIfRunning
        } else {
            KernelChangeImpact::PersistOnly
        };
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
            kernel_impact: KernelChangeImpact::PersistOnly,
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
    pub kernel_action: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeActionPlan {
    pub patch_active_config: bool,
    pub apply_proxy_runtime: bool,
    pub kernel_impact: KernelChangeImpact,
}

pub fn plan_runtime_actions(
    change: RuntimeChange,
    options: &RuntimeApplyOptions,
) -> RuntimeActionPlan {
    RuntimeActionPlan {
        patch_active_config: options.patch_active_config,
        apply_proxy_runtime: matches!(
            change,
            RuntimeChange::SubscriptionApplied | RuntimeChange::ProxySettingsChanged
        ),
        kernel_impact: if matches!(change, RuntimeChange::ProxySettingsChanged) {
            KernelChangeImpact::HotApply
        } else {
            options.kernel_impact
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{plan_runtime_actions, KernelChangeImpact, RuntimeApplyOptions, RuntimeChange};

    #[test]
    fn app_config_update_should_preserve_kernel_state_by_default() {
        let options = RuntimeApplyOptions::new("test").patch_active_config(true);
        let plan = plan_runtime_actions(RuntimeChange::AppConfigUpdated, &options);

        assert!(plan.patch_active_config);
        assert!(!plan.apply_proxy_runtime);
        assert_eq!(plan.kernel_impact, KernelChangeImpact::PersistOnly);
    }

    #[test]
    fn subscription_apply_should_preserve_kernel_state_by_default() {
        let options = RuntimeApplyOptions::new("test").patch_active_config(true);
        let plan = plan_runtime_actions(RuntimeChange::SubscriptionApplied, &options);

        assert!(plan.patch_active_config);
        assert!(plan.apply_proxy_runtime);
        assert_eq!(plan.kernel_impact, KernelChangeImpact::PersistOnly);
    }

    #[test]
    fn proxy_settings_change_should_be_hot_apply() {
        let options = RuntimeApplyOptions::new("test");
        let plan = plan_runtime_actions(RuntimeChange::ProxySettingsChanged, &options);

        assert!(!plan.patch_active_config);
        assert!(plan.apply_proxy_runtime);
        assert_eq!(plan.kernel_impact, KernelChangeImpact::HotApply);
    }

    #[test]
    fn runtime_change_as_str_covers_all_variants() {
        assert_eq!(
            RuntimeChange::AppConfigUpdated.as_str(),
            "app_config_updated"
        );
        assert_eq!(
            RuntimeChange::ActiveConfigChanged.as_str(),
            "active_config_changed"
        );
        assert_eq!(
            RuntimeChange::SubscriptionApplied.as_str(),
            "subscription_applied"
        );
        assert_eq!(
            RuntimeChange::ProxySettingsChanged.as_str(),
            "proxy_settings_changed"
        );
        assert_eq!(RuntimeChange::KernelUpdated.as_str(), "kernel_updated");
    }

    #[test]
    fn options_builder_and_defaults() {
        let opts = RuntimeApplyOptions::new("reason-x")
            .restart_if_running(true)
            .patch_active_config(true)
            .use_original_config_hint(Some(true));
        assert_eq!(opts.kernel_impact, KernelChangeImpact::RestartIfRunning);
        assert!(opts.patch_active_config);
        assert_eq!(opts.use_original_config_hint, Some(true));
        assert_eq!(opts.reason, "reason-x");

        let d = RuntimeApplyOptions::default();
        assert_eq!(d.kernel_impact, KernelChangeImpact::PersistOnly);
        assert!(!d.patch_active_config);
        assert!(d.use_original_config_hint.is_none());

        let plan = plan_runtime_actions(RuntimeChange::KernelUpdated, &opts);
        assert_eq!(plan.kernel_impact, KernelChangeImpact::RestartIfRunning);
        assert!(!plan.apply_proxy_runtime);
        assert!(plan.patch_active_config);
    }
}
