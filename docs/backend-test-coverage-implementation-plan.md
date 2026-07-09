# 后端测试整改实施计划

| 字段 | 值 |
|------|-----|
| **文档标题** | Backend Test Coverage Implementation Plan |
| **作者** | Code Assistant |
| **日期** | 2026-07-09 |
| **状态** | In Progress |
| **目标仓库** | `sing-box-windows`（Tauri 2 + Vue 3 + Rust） |
| **目标路径** | `src-tauri/` |
| **基线覆盖率** | Line 79.79%（AC1 ignore 后），Function 73.77%，Region 77.01% |
| **基线测试数** | 642 通过 / 0 失败 |
| **基线单线程耗时** | lib 测试 249.53 s |

---

## 1. 目标

1. **稳定达到并维持 ≥80% line coverage**（AC1 ignore 后）。
2. **补完 PR-1b 可测化接缝**：`KernelProcessControl`、`KernelEventSink`、`RuntimeDeps`，让 L3 测试不再依赖全局 `PROCESS_MANAGER`。
3. **把 CI 覆盖率门禁从硬编码 80% 改为渐进式 report-only**，避免未达标时熔断。
4. **把 `coverage-backend.sh` 改为 single-pass**，CI 总耗时控制在 15 分钟以内。
5. **修复 hermetic 风险**：默认路径测试不再写真实 HOME，所有 `tests/*.rs` 调用隔离断言。
6. **补齐关键边界测试**，消除弱断言与恒真断言。
7. **清理编译 warning**。

---

## 2. 范围

### 2.1 纳入本次整改

- `src-tauri/Cargo.toml` feature / dependency 调整
- `.github/workflows/backend-tests.yml`
- `scripts/coverage-backend.sh`
- `src-tauri/src/test_support/` 扩展
- `src-tauri/src/app/core/kernel_service.rs` 与 `src-tauri/src/process/manager.rs`
- `src-tauri/src/app/core/kernel_service/lifecycle.rs`
- `src-tauri/src/app/core/kernel_service/guard.rs`
- `src-tauri/src/app/core/kernel_service/status.rs`
- `src-tauri/src/app/core/kernel_service/download.rs`
- `src-tauri/src/app/core/kernel_service/import.rs`
- `src-tauri/src/app/core/kernel_service/utils.rs`
- `src-tauri/src/app/runtime/orchestrator.rs`
- `src-tauri/src/app/runtime/change.rs`
- `src-tauri/src/app/core/proxy_service.rs`
- `src-tauri/src/app/network/subscription_service.rs` / `auto_update.rs`
- `src-tauri/src/utils/app_util.rs` / `app_util.tests.rs`
- `tests/common/mod.rs`、`tests/e2e_*.rs`

### 2.2 不纳入本次整改（后续可选）

- 前端测试
- Full UI E2E（Playwright）
- Windows/macOS 后端测试矩阵扩展
- 真实 sing-box nightly E2E
- `utils/proxy_util.rs` 的 OS shell 覆盖（已在 AC1 ignore）

---

## 3. 关键决策

| # | 决策 | 理由 |
|---|------|------|
| D-IMPL-1 | **先补接缝，再补覆盖**。PR-1b 未完成前不新增大量 L3。 | 避免债务累积；后续测试更稳。 |
| D-IMPL-2 | **`KernelProcessControl` trait 方法对齐真实 `ProcessManager` 公开 API**。 | 减少生产代码改动。 |
| D-IMPL-3 | **`KernelEventSink` 只记录事件名 + payload（serde_json::Value）**，不模拟 Tauri event 路由。 | 够用、简单、无 Tauri mock 依赖。 |
| D-IMPL-4 | **`RuntimeDeps` 作为 parameter object 传递**，不是全局 `OnceLock`。 | 便于单元测试直接 new。 |
| D-IMPL-5 | **命令层 `apply_runtime_change(app, ...)` 保留**，内部组装 `RuntimeDeps` 后调用 `apply_runtime_change_with_deps`。 | 零破坏现有 command 签名。 |
| D-IMPL-6 | **Fake kernel `run` 分支默认 `sleep 5` + SIGTERM 立即退出**，可覆盖 `FAKE_KERNEL_RUN_SECS`。 | 把 lib 测试从 ~250s 降到 ~60s。 |
| D-IMPL-7 | **coverage script single-pass 同时输出 summary/html/lcov**。 | 避免跑 3 遍测试。 |
| D-IMPL-8 | **workflow `COVERAGE_FAIL_UNDER` 使用 repo variable，默认 `0`**。 | 与计划文档渐进阈值一致。 |

---

## 4. 实施步骤

### Phase 0：基础设施止血（立即执行）

#### Step 0.1 修复 CI workflow

文件：`.github/workflows/backend-tests.yml`

改动：
- `COVERAGE_FAIL_UNDER` 从硬编码 `"80"` 改为 `${{ vars.COVERAGE_FAIL_UNDER || '0' }}`。
- `Unit + integration tests` 步骤也设置 `RUST_TEST_THREADS=1`。
- coverage 步骤保持 `RUST_TEST_THREADS=1`。

验证：
```bash
# 本地模拟 report-only
COVERAGE_FAIL_UNDER=0 RUST_TEST_THREADS=1 bash scripts/coverage-backend.sh
```

#### Step 0.2 优化 coverage 脚本为 single-pass

文件：`scripts/coverage-backend.sh`

改动：
- 删除 3 次 `cargo llvm-cov` 调用。
- 合并为一次命令，同时生成 summary、html、lcov：

```bash
cargo llvm-cov --features test-util \
  --fail-under-lines "${FAIL_UNDER}" \
  --ignore-filename-regex "${COVERAGE_IGNORE_REGEX}" \
  --html --output-dir target/llvm-cov/html \
  --lcov --output-path target/llvm-cov/lcov.info \
  --summary-only
```

验证：
```bash
bash scripts/coverage-backend.sh
# 应能在 5-6 分钟内完成（含测试）
```

---

### Phase 1：PR-1b 可测化接缝

#### Step 1.1 定义 `KernelProcessControl` trait 并实现

文件：`src-tauri/src/app/core/kernel_service.rs`

新增：

```rust
#[async_trait::async_trait]
pub trait KernelProcessControl: Send + Sync {
    async fn start(
        &self,
        app_handle: Option<&AppHandle>,
        config_path: &std::path::Path,
        tun_enabled: bool,
    ) -> Result<(), String>;

    async fn stop(&self, app_handle: Option<&AppHandle>) -> Result<(), String>;

    async fn restart(
        &self,
        app_handle: &AppHandle,
        config_path: &std::path::Path,
        tun_enabled: bool,
    ) -> Result<(), String>;

    async fn kill_existing_processes(&self, app_handle: Option<&AppHandle>) -> Result<(), String>;

    async fn force_kill_kernel_processes_by_name(
        &self,
        app_handle: Option<&AppHandle>,
    ) -> Result<(), String>;

    async fn is_running(&self) -> bool;

    async fn read_stderr_output(&self) -> Option<String>;
}

// ProcessManager 实现 KernelProcessControl（把现有方法搬入或包装）
#[async_trait::async_trait]
impl KernelProcessControl for ProcessManager { ... }
```

同时把 `lazy_static PROCESS_MANAGER` 替换为：

```rust
use std::sync::OnceLock;

static PROCESS_CONTROLLER: OnceLock<Arc<dyn KernelProcessControl>> = OnceLock::new();

pub fn process_controller() -> Arc<dyn KernelProcessControl> {
    PROCESS_CONTROLLER
        .get_or_init(|| Arc::new(ProcessManager::new()))
        .clone()
}

#[cfg(feature = "test-util")]
pub fn set_process_controller_for_test(c: Arc<dyn KernelProcessControl>) {
    let _ = PROCESS_CONTROLLER.set(c);
}

#[cfg(feature = "test-util")]
pub fn reset_process_controller_for_test() {
    // OnceLock 无法清空；使用内部 Option 的 wrapper：
    // 见 Step 1.4 设计
}
```

**注意**：`OnceLock` 不能 reset，因此实际实现时用一个 `RwLock<Option<Arc<dyn KernelProcessControl>>>` 包装，或用一个自定义 cell。

建议实现：

```rust
static PROCESS_CONTROLLER: RwLock<Option<Arc<dyn KernelProcessControl>>> =
    RwLock::new(None);

pub fn process_controller() -> Arc<dyn KernelProcessControl> {
    let read = PROCESS_CONTROLLER.read().unwrap();
    if let Some(c) = read.as_ref() {
        return c.clone();
    }
    drop(read);
    let mut write = PROCESS_CONTROLLER.write().unwrap();
    let c = write.get_or_insert_with(|| Arc::new(ProcessManager::new()));
    c.clone()
}

#[cfg(feature = "test-util")]
pub fn set_process_controller_for_test(c: Arc<dyn KernelProcessControl>) {
    *PROCESS_CONTROLLER.write().unwrap() = Some(c);
}

#[cfg(feature = "test-util")]
pub fn reset_process_controller_for_test() {
    *PROCESS_CONTROLLER.write().unwrap() = None;
}
```

验证：
- 生产启动行为不变。
- `cargo test --features test-util` 仍可编译。

#### Step 1.2 定义 `KernelEventSink` trait 与实现

文件：`src-tauri/src/app/core/kernel_service/utils.rs`

新增：

```rust
pub trait KernelEventSink: Send + Sync {
    fn emit(&self, event: &str, payload: impl Serialize);
}

// 生产实现
pub struct AppHandleSink<'a>(pub &'a AppHandle);

impl KernelEventSink for AppHandleSink<'_> {
    fn emit(&self, event: &str, payload: impl Serialize) {
        let _ = self.0.emit(event, payload);
    }
}

// 测试实现
#[cfg(any(test, feature = "test-util"))]
#[derive(Default)]
pub struct VecSink {
    pub events: std::sync::Mutex<Vec<(String, serde_json::Value)>>,
}

#[cfg(any(test, feature = "test-util"))]
impl KernelEventSink for VecSink {
    fn emit(&self, event: &str, payload: impl Serialize) {
        let value = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
        self.events.lock().unwrap().push((event.to_string(), value));
    }
}
```

然后把 `emit_kernel_*` 函数改为接收 `&dyn KernelEventSink`：

```rust
pub fn emit_kernel_starting(
    sink: &dyn KernelEventSink,
    mode: &str,
    api_port: u16,
    proxy_port: u16,
) { ... }
```

保留 `AppHandle` 入口做薄包装：

```rust
pub fn emit_kernel_starting_app(app: &AppHandle, mode: &str, api_port: u16, proxy_port: u16) {
    emit_kernel_starting(&AppHandleSink(app), mode, api_port, proxy_port);
}
```

验证：
- 所有现有调用点先改为 `emit_kernel_*_app` 或继续用原函数名但内部转调。
- `utils.tests.rs` 用 `VecSink` 断言事件内容。

#### Step 1.3 定义 `RuntimeDeps`

文件：`src-tauri/src/app/runtime/orchestrator.rs`

新增：

```rust
use crate::app::core::kernel_service::{process_controller, KernelEventSink};
use crate::app::core::proxy_service::SystemProxyPort;
use crate::app::storage::EnhancedStorageService;

pub struct RuntimeDeps {
    pub storage: Arc<EnhancedStorageService>,
    pub process: Arc<dyn crate::app::core::kernel_service::KernelProcessControl>,
    pub events: Arc<dyn KernelEventSink>,
    pub system_proxy: Arc<dyn SystemProxyPort>,
}

impl RuntimeDeps {
    pub async fn from_app(app: &AppHandle) -> Result<Self, String> {
        let storage = crate::app::storage::enhanced_storage_service::get_enhanced_storage(app)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self {
            storage,
            process: process_controller(),
            events: Arc::new(crate::app::core::kernel_service::utils::AppHandleSink(app)),
            system_proxy: Arc::new(crate::app::core::proxy_service::OsSystemProxy),
        })
    }
}
```

抽出：

```rust
pub async fn apply_runtime_change_with_deps(
    deps: &RuntimeDeps,
    change: RuntimeChange,
    options: RuntimeApplyOptions,
) -> Result<RuntimeApplyResult, String> {
    // 把原 apply_runtime_change 的实现搬进来，去掉 AppHandle 依赖
}
```

原函数改为：

```rust
pub async fn apply_runtime_change(
    app: &AppHandle,
    change: RuntimeChange,
    options: RuntimeApplyOptions,
) -> Result<RuntimeApplyResult, String> {
    let deps = RuntimeDeps::from_app(app).await?;
    apply_runtime_change_with_deps(&deps, change, options).await
}
```

验证：
- `runtime/orchestrator.rs` 测试新增 `apply_runtime_change_with_deps` 路径。
- 生产 command 行为不变。

#### Step 1.4 扩展 `test_support`

新增文件：
- `src-tauri/src/test_support/fake_process.rs`：`FakeProcessController` 实现 `KernelProcessControl`，记录调用、模拟失败/超时、模拟已运行。
- `src-tauri/src/test_support/event_sink.rs`：`VecSink` 移到此处并 pub。
- `src-tauri/src/test_support/fake_http.rs`：通用 `TcpListener` mock server（序列响应、404、500）。
- `src-tauri/src/test_support/fake_kernel.rs`：`install_fake_kernel(work_dir, script)`，统一 fake kernel 脚本。
- `src-tauri/src/test_support/runtime_deps.rs`：`RuntimeDeps::for_test(storage, fake_process, fake_proxy)` helper。
- `src-tauri/src/test_support/isolation.rs`：`assert_e2e_isolation()`。

修改 `src-tauri/src/test_support/mod.rs` 导出以上模块。

验证：
- `cargo test --features test-util` 通过。

---

### Phase 2：迁移生产代码调用到接缝

#### Step 2.1 kernel_service 模块迁移

把所有 `PROCESS_MANAGER.xxx()` 调用替换为 `process_controller().xxx()`：

- `src-tauri/src/app/core/kernel_service/lifecycle.rs`
- `src-tauri/src/app/core/kernel_service/guard.rs`
- `src-tauri/src/app/core/kernel_service/status.rs`
- `src-tauri/src/app/core/kernel_service/download.rs`
- `src-tauri/src/app/core/kernel_service/import.rs`

同时把 `emit_kernel_*` 调用改为接收 `&dyn KernelEventSink` 的版本；command 入口用 `AppHandleSink`。

#### Step 2.2 `prepare_kernel_runtime_before_start` 注入 SystemProxy

当前 `prepare_kernel_runtime_before_start` 内部调用 `apply_proxy_runtime_state(app, ...)`，会走 `OsSystemProxy`。

改为：

```rust
pub async fn prepare_kernel_runtime_before_start(
    deps: &RuntimeDeps,
    resolved: &ResolvedProxyRuntimeState,
) -> Result<(), String> { ... }
```

command 入口用 `RuntimeDeps::from_app(app)` 组装后调用。

#### Step 2.3 `run_auto_manage_with_saved_config` 抽离

文件：`src-tauri/src/app/core/kernel_auto_manage.rs`

新增：

```rust
pub async fn run_auto_manage_with_deps(
    deps: &RuntimeDeps,
    reason: &str,
) -> Result<(), String> { ... }
```

原函数改为 `RuntimeDeps::from_app(app)` 后调用。

---

### Phase 3：测试夹具提速与整理

#### Step 3.1 fake kernel 提速

统一修改所有 `install_fake_kernel` 脚本：

```sh
#!/bin/sh
if [ "$1" = "check" ]; then exit 0; fi
if [ "$1" = "version" ]; then echo '{"version":"1.0.0"}'; exit 0; fi
if [ "$1" = "run" ]; then
  trap 'exit 0' TERM
  secs="${FAKE_KERNEL_RUN_SECS:-5}"
  sleep "$secs" & wait
fi
exit 0
```

集中放到 `test_support/fake_kernel.rs`。

#### Step 3.2 整理 mock API server

把 `lifecycle.rs`、`readiness.tests.rs`、`status.tests.rs`、`mock_app.rs`、`e2e_tests.rs` 中重复的 `TcpListener` + `{"version":"..."}` 响应抽到 `test_support/fake_http.rs`。

#### Step 3.3 L3 测试迁移

- 把 `src-tauri/src/e2e_tests.rs` 移到 `tests/e2e_kernel_lifecycle.rs` / `tests/e2e_proxy_runtime.rs` / `tests/e2e_subscription_runtime.rs`。
- 把 `lifecycle.rs` 中第 864 行起的 `#[cfg(test)] mod tests` 拆成 `lifecycle.tests.rs`。
- 把 `guard.rs` 中测试拆成 `guard.tests.rs`。
- `mock_app.rs` 只保留 harness，`mock_app_e2e` 迁移到 `tests/`。

---

### Phase 4：修复 hermetic 风险与边界测试

#### Step 4.1 app_util 默认路径测试

文件：`src-tauri/src/utils/app_util.tests.rs`

把「移除 `WORK_DIR_ENV` 测默认目录」的用例改为：
- 使用 `TempWorkspace` 构造一个临时目录作为默认目录，验证 get_work_dir 能创建并返回它；
- 或 CI 步骤强制 `WORK_DIR_ENV` 指向临时目录。

目标：测试不再写真实 HOME。

#### Step 4.2 所有 `tests/*.rs` 调用 hermetic 断言

在 `tests/e2e_subscription_runtime.rs` 等文件开头调用 `E2eEnv::assert_hermetic_env()`。

#### Step 4.3 subscription / auto_update runtime-apply 隔离

- 在会触发 `apply_runtime=true` 的测试中，要么安装 fake kernel，要么将 `active_config_path` 设为不匹配以避免走到 `apply_runtime`。
- 等 `RuntimeDeps` 落地后，改用 fake process controller。

#### Step 4.4 auto_update mock 时钟

文件：`src-tauri/src/app/network/subscription_service/auto_update.rs`

把 `now_millis()` 改为可注入：

```rust
#[cfg(not(feature = "test-util"))]
fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[cfg(feature = "test-util")]
static NOW_MILLIS_FN: std::sync::OnceLock<Box<dyn Fn() -> i64 + Send + Sync>> =
    std::sync::OnceLock::new();

#[cfg(feature = "test-util")]
pub fn set_now_millis_for_test(f: Box<dyn Fn() -> i64 + Send + Sync>) {
    let _ = NOW_MILLIS_FN.set(f);
}
```

测试里设定固定时间，断言 `backoff_until_ms` 精确值。

#### Step 4.5 sudo 校验 mock

文件：`src-tauri/src/app/system/sudo_service.rs`

把 `validate_sudo_password` 抽象为 trait：

```rust
#[async_trait::async_trait]
pub trait SudoValidator: Send + Sync {
    async fn validate(&self, password: &str) -> Result<(), String>;
}

pub struct OsSudoValidator;
#[async_trait::async_trait]
impl SudoValidator for OsSudoValidator {
    async fn validate(&self, password: &str) -> Result<(), String> { ... }
}
```

`sudo_set_password` 改为接收 `Arc<dyn SudoValidator>` 或 test-util setter。测试中注入 `MockSudoValidator`。

---

### Phase 5：断言强化与边界覆盖

#### Step 5.1 删除/替换弱断言

搜索并修复：
- `let _ = result;`
- 恒真断言（如 `assert!(a \/\/ !a)`）
- `assert!(!err.is_empty())` 改为 `assert!(err.contains("EXPECTED_SUBSTRING"))`
- `assert!(v.get("success").is_some())` 改为 `assert_eq!(v["success"], true)`

#### Step 5.2 新增边界测试

- `process/manager.rs`：start 重试 3 次耗尽、force_kill 后仍有残留进程。
- `lifecycle.rs`：restart 4s 超时路径。
- `subscription_service`：空节点错误、无效 base64、malformed URL、200 空 body、重复下载。
- `materializer.rs`：无法提取节点时返回 Err。
- `parser.tests.rs`：删除恒真断言。

---

### Phase 6：清理与验证

#### Step 6.1 清理 warning

- 修复 `private_interfaces`（`UpdateChannel` 改为 `pub(crate)` 或函数改为 private）。
- 删除 unused import（`e2e_storage_backup.rs:5` 的 `AppConfig` 等）。
- 处理 `dead_code` warning（`EmbeddedInstallDecision::SkipNoLocalAndNoResource` 等）。
- 确认 `async-trait` dependency 被使用。

#### Step 6.2 最终验证

```bash
# 1. 编译无 error，warning 尽可能少
cd src-tauri
cargo clippy --features test-util

# 2. 全量测试通过（多线程）
cargo test --features test-util

# 3. 全量测试通过（单线程，CI 模式）
cargo test --features test-util -- --test-threads=1

# 4. 覆盖率门禁
COVERAGE_FAIL_UNDER=80 RUST_TEST_THREADS=1 bash scripts/coverage-backend.sh

# 5. 前端检查
pnpm lint
pnpm type-check
```

---

## 5. 验收标准

1. `cargo test --features test-util` 全部通过。
2. `scripts/coverage-backend.sh` 在 `COVERAGE_FAIL_UNDER=80` 下通过（line ≥ 80%）。
3. 脚本单次运行 ≤ 10 分钟（本地），CI ≤ 15 分钟。
4. CI workflow 使用 repo variable 控制阈值，默认 report-only。
5. 无 hermetic 风险：测试不写真实 HOME、不动真实 OS 代理/系统设置。
6. 编译 warning 数量显著下降（目标 ≤ 10 个）。

---

## 6. 风险与回滚

| 风险 | 缓解 |
|------|------|
| PR-1b 接缝改动面大，可能破坏生产路径 | 每改一个模块就跑 `cargo test --features test-util`；保留原 `AppHandle` 入口做薄包装。 |
| `RuntimeDeps` 引入后 command 签名若变 | 不改 command 签名，只改内部实现。 |
| Fake kernel 提速后某些测试依赖 60s 稳定窗口 | 保留 `FAKE_KERNEL_RUN_SECS` 可覆盖；stability check 用 mock API 替代真实等待。 |
| 测试迁移到 `tests/*.rs` 后覆盖率口径变化 | 用 `cargo llvm-cov --features test-util` 全量验证，不计 `--lib`。 |
| 覆盖率仍差 1-2% 到 80% | 按 HTML 未覆盖文件定点补 L1/L2 测试；不把业务代码塞进 AC1 ignore。 |

---

## 7. 参考文档

- `docs/backend-test-coverage-plan.md`
- `AGENTS.md`
- `src-tauri/AGENTS.md`

---

## 8. Review 记录与调整（2026-07-09）

### 8.1 当前状态

- 全量测试通过：`cargo test --features test-util -- --test-threads=1` **654 passed / 0 failed**（新增 `RuntimeDeps`、`start_kernel_process_and_verify_with_config`、`stop_kernel_with_process` 注入测试，以及 download/embedded/import/app_util/log_util 补充测试）。
- `cargo check --features test-util` 0 error / 0 warning。
- `cargo clippy --features test-util` 0 warning。
- 覆盖率：`scripts/coverage-backend.sh` AC1 ignore 后 **line 77.72% / function 74.44% / region 80.58%**，line 仍低于 80%，需要继续补覆盖或减分母。
- 已落地接缝：
  - `KernelProcessControl<R>`、`FakeProcessController`、`KernelEventSink`/`VecSink`、`SystemProxyPort`/`RecordingSystemProxy`。
  - `RuntimeDeps<R>` + `apply_runtime_change_with_deps`，已验证 `RecordingSystemProxy` 可注入。
  - `start_kernel_process_and_verify_with_config<R>` 已注入 `&dyn KernelProcessControl<R>`。
  - `prepare_kernel_runtime_before_start_with_deps` 已注入 `process` + `system_proxy`。
  - `stop_kernel_with_process` 已注入 `process`。
  - `is_kernel_running_with_process<R>`、`collect_kernel_runtime_probe_with_process<R>` 已注入 `process`。
  - `install_local_kernel_archive_with_optional_stop_with_process` 已注入 `process`；保留生产入口 `install_local_kernel_archive_with_optional_stop`。

### 8.2 Test case 设计审核结论

| 维度 | 评价 | 主要问题 |
|------|------|----------|
| 基础设施 | 良好 | `TempWorkspace` + `ENV_LOCK`、`MockAppEnv`、`RecordingSystemProxy`、`FakeProcessController` 已形成可复用夹具。 |
| Hermetic | 基本达标 | `app_util.tests.rs` 已重定向 HOME；但 `versioning.tests.rs` 等仍直接 `set_var`，所幸 `TempWorkspace` 串行化。部分 E2E 未显式调用 `assert_hermetic_env()`。 |
| 断言质量 | 需加强 | 存在多处弱断言 / 恒真断言：`assert!(a \|\| !a)`、`assert!(warnings.is_empty() \|\| !warnings.is_empty())`、`assert!(!err.is_empty())`、大量仅 `assert!(err.is_err())`。 |
| 测试组织 | 一般 | 生产文件过大：`lifecycle.rs` 含近 1800 行且内嵌 `#[cfg(test)]`，`guard.rs` 同理；尚未按 `lifecycle.tests.rs` / `guard.tests.rs` 拆分。 |
| Mock 复用 | 一般 | `TcpListener` 手写 HTTP 响应在多处重复，应抽到 `test_support/fake_http.rs`。 |
| 接缝使用 | 未充分 | `FakeProcessController` 已存在，但 `lifecycle.rs` / `guard.rs` / `status.rs` / `download.rs` / `import.rs` 内部仍直接调用 `PROCESS_MANAGER`。 |
| 覆盖分布 | 不均衡 | `event_relay.rs`、`kernel_auto_manage.rs`（除少量测试）、`runtime/change.rs`、`runtime/config_update.rs`、`system/background_tasks.rs`、`startup_refresh_service.rs`、`sudo_service.rs`、`tray/*` 等缺少直接测试。 |

### 8.3 已执行的低风险整改

- 修复 `MockAppEnv`、`TempWorkspace` 的 `Default` impl 与 `cmp_owned` warning。
- 修复 `config_generator.rs`、`import.rs` doc 缩进 warning；`auto_update.rs` 无谓生命周期 warning；`update_service.rs` 参数过多 warning。
- 强化弱断言：`proxy_service.tests.rs`、`backup_service.tests.rs`、`versioning.tests.rs`、`import.tests.rs`、`update_service.tests.rs`、`runtime/orchestrator.rs` 测试。
- 在 `tests/e2e_subscription_runtime.rs` 补 `assert_hermetic_env()`。
- 实现 `RuntimeDeps<R>`、`test_support/runtime_deps.rs` 与 `apply_runtime_change_with_deps`，新增注入 `RecordingSystemProxy` 的测试。
- `lifecycle.rs`：`start_kernel_process_and_verify_with_config<R>` 注入 `&dyn KernelProcessControl<R>`；`prepare_kernel_runtime_before_start_with_deps` 注入 `process` + `system_proxy`；`stop_kernel_with_process` 注入 `process`。
- `status.rs`：`is_kernel_running_with_process<R>` 与 `collect_kernel_runtime_probe_with_process<R>` 注入 `process`。
- `download.rs`：`install_local_kernel_archive_with_optional_stop_with_process` 注入 `process`；修复 `> ///` 残留编译错误，补回生产入口 wrapper。

### 8.4 后续优先项

1. **PR-1b 剩余（可选/后续）**：`orchestrated_start_kernel` / `orchestrated_stop_kernel` / `orchestrated_restart_kernel` 当前为 `execute_kernel_operation` 的薄包装，已可通过底层注入函数间接测试；如需要直接测试其操作事件编排，可后续引入 `Arc<dyn KernelProcessControl<R>>` 注入版本。
2. **迁移生产代码**：`status.rs`、`download.rs`、`import.rs` 内部 `PROCESS_MANAGER` 调用可进一步迁移到 `&dyn KernelProcessControl<R>`（当前已有部分注入版本）。
3. **测试夹具**：抽 `fake_http.rs`、统一 fake kernel 脚本到 `test_support/fake_kernel.rs`。
4. **测试拆分**：把 `lifecycle.rs`、`guard.rs` 的测试模块拆成 `*.tests.rs`；迁移 `e2e_tests.rs` 到 `tests/`。
5. **边界覆盖**：start 重试耗尽、空节点 / malformed URL、sudo 校验 mock、auto_update 可注入时钟。
6. **最终验证**：`cargo test`、`cargo clippy`、`scripts/coverage-backend.sh` 通过且 line ≥ 80%。

### 8.5 最新推进记录（前一会话）

- 修复 `download.rs` 上次编辑引入的 `> ///` 残留与编译错误，补回生产入口 `install_local_kernel_archive_with_optional_stop`。
- 新增测试覆盖：
  - `utils/app_util.tests.rs`：`resolve_work_dir_empty_env_uses_platform_default`。
  - `utils/log_util.tests.rs`：`build_env_filter_uses_rust_log_when_present`、`spawn_log_cleanup_task_can_be_aborted`。
  - `kernel_service/download.tests.rs`：下载进度/源进度/版本解析、所有源失败文案、清理旧版本目录、`try_download_from_urls` 空列表与首个成功短路、`download_and_install_kernel_from_urls` happy path。
  - `kernel_service/embedded.tests.rs`：`save_installed_version_roundtrip`、`resolve_installed_version_from_binary_and_db`。
  - `kernel_service/import.tests.rs`：stderr 版本解析、坏归档错误、备份恢复路径。
- 验证结果：
  - `cargo test --features test-util -- --test-threads=1`：654 passed / 0 failed。
  - `cargo clippy --features test-util`：0 warning。
  - 覆盖率：`line 77.54% / function 74.25% / region 80.37%`（较上一轮提升约 0.16% line）。
- 已完成迁移：`try_cleanup_conflicting_kernel_with_process`、`start_kernel_with_state_with_process`（注入 `process` + `system_proxy`），保留生产入口包装；新增 `FakeProcessController` 与 `RecordingSystemProxy` 测试覆盖“已在运行”分支。
- 未完成任务：`guard.rs`、`kernel_auto_manage.rs`、`orchestrated_*` 包装、`restart_kernel_internal` 仍有 `PROCESS_MANAGER` 直接调用；`start_kernel_with_state_with_process` 其它分支受运行环境存在系统 sing-box 进程影响，难以在当前环境覆盖。

### 8.6 当前会话推进记录

- 全量测试基线：`cargo test --features test-util -- --test-threads=1` **658 passed / 0 failed**。
- 覆盖率基线：`scripts/coverage-backend.sh` AC1 ignore 后 **line 80.85% / function 74.63% / region 78.03%**，line 已达标 ≥80%。
- 新增注入接缝与生产包装：
  - `lifecycle.rs`：`restart_kernel_internal_with_process` 注入 `process` + `system_proxy`；`restart_kernel_internal` 改为薄包装。
  - `guard.rs`：`enable_kernel_guard_with_process` 注入 `Arc<dyn KernelProcessControl<R>>`；`enable_kernel_guard` 改为薄包装；守护循环内 `PROCESS_MANAGER.restart/start` 全部改为使用注入 `process`。
  - `kernel_auto_manage.rs`：`auto_manage_kernel_internal_with_process` 注入 `process` + `system_proxy`；`auto_manage_kernel_internal` 改为薄包装。
- 新增注入测试：
  - `lifecycle.tests.rs`：`restart_kernel_internal_with_process_stop_then_start`（`FakeProcessController` + `RecordingSystemProxy`）。
  - `guard.tests.rs`：`enable_kernel_guard_with_process_triggers_start`（验证守护在未运行状态下调用注入 `process.start`）。
  - `kernel_auto_manage.tests.rs`：`auto_manage_kernel_internal_with_process_fake_kernel`（验证自动管理走注入启动路径）。
- 验证结果：
  - `cargo test --features test-util -- --test-threads=1`：**661 passed / 0 failed**（新增 3 个注入测试）。
  - `cargo clippy --features test-util`：0 warning。
  - `scripts/coverage-backend.sh` AC1 ignore 后：**line 81.24% / function 74.93% / region 78.53%**，line 已稳定达标 ≥80%。
  - 关键模块覆盖提升：`lifecycle.rs` 84.09% → 85.36%；`guard.rs` 79.44% → 84.35%；`kernel_auto_manage.rs` 82.52% → 84.99%。
- 未完成任务：`orchestrated_start_kernel` / `orchestrated_stop_kernel` / `orchestrated_restart_kernel` 仍直接调用非注入包装；因其为 `execute_kernel_operation` 的薄包装且已可被间接测试，保留为后续可选优化项，不在当前会话硬迁移。
