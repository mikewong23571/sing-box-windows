//! 测试基建（feature `test-util` 与 `cfg(test)` 共用的公共夹具）。
//! 生产路径不依赖此模块。

pub mod fake_process;
pub mod mock_app;
pub mod runtime_deps;
pub mod temp_workspace;

pub use fake_process::{Call, FakeProcessController};
pub use mock_app::MockAppEnv;
pub use temp_workspace::TempWorkspace;
