//! Tauri command compatibility layer for kernel runtime operations.
//!
//! Lifecycle implementation lives in `lifecycle`; this module keeps the
//! historic command paths stable for the frontend and Tauri handler table.

pub use super::lifecycle::{
    apply_proxy_settings, kernel_restart_fast, kernel_start_enhanced, kernel_stop_enhanced,
    orchestrated_apply_change, orchestrated_apply_change_with_deps, orchestrated_restart_kernel,
    orchestrated_resume_after_maintenance, orchestrated_start_kernel,
    orchestrated_startup_reconcile, orchestrated_stop_kernel, orchestrated_suspend_for_maintenance,
    resolve_proxy_runtime_state, start_kernel_with_state, stop_kernel, stop_kernel_with_process,
    ProxyOverrides, ResolvedProxyState,
};
