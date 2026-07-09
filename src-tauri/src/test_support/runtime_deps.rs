//! `RuntimeDeps` 测试构造辅助。

use crate::app::core::kernel_service::utils::VecSink;
use crate::app::core::kernel_service::KernelProcessControl;
use crate::app::core::proxy_service::SystemProxyPort;
use crate::app::runtime::orchestrator::RuntimeDeps;
use crate::app::storage::enhanced_storage_service::EnhancedStorageService;
use std::sync::Arc;
use tauri::Runtime;

impl<R: Runtime> RuntimeDeps<R> {
    /// 构造一个全部依赖均可注入的测试 RuntimeDeps。
    pub fn for_test(
        storage: Arc<EnhancedStorageService>,
        process: Arc<dyn KernelProcessControl<R>>,
        system_proxy: Arc<dyn SystemProxyPort>,
    ) -> Self {
        Self {
            storage,
            process,
            events: Arc::new(VecSink::default()),
            system_proxy,
        }
    }
}
