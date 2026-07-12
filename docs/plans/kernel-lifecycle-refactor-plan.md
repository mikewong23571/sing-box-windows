# 内核应用生命周期重构计划

**状态：** 已完成
**创建日期：** 2026-07-11
**适用范围：** Vue 前端、Tauri Runtime 编排、Sing-Box 进程管理、订阅应用、托盘与守护逻辑

## 1. 背景

当前项目已经具备内核 start/stop/restart 命令、串行操作队列、运行状态事件和 Dashboard 控件，但配置应用与内核生命周期仍然存在错误耦合：

- `auto_start_kernel` 被映射成 `keep_alive`，却没有真正控制应用启动时是否拉起内核。
- 设置保存、活动配置切换、订阅应用和内核更新会进入 auto-manage；当内核已停止时，auto-manage 会重新启动内核。
- 用户点击“停止”只改变当前进程状态，没有形成可持续的会话级停止意图，后续配置操作可能推翻该操作。
- 显式生命周期命令进入串行队列，但 runtime auto-manage 和 guard 的部分路径可直接调用底层 start/restart，仍存在竞态。
- 前端同时维护 KernelStore 状态和 AppStore `isRunning` 镜像，增加状态漂移风险。
- 订阅下载、活动配置设置和 bootstrap 恢复存在重复 apply 路径。

本次重构的目标不是增加更多自动化，而是收敛生命周期控制权，使启停行为可预测、可测试、可解释。

## 2. 目标与非目标

### 2.1 目标

1. 后端成为内核生命周期的唯一权威写入者。
2. 分离“用户期望状态”和“进程观测状态”。
3. 所有生命周期变更通过同一个串行协调器执行。
4. 配置变更保持当前启停状态：运行时按需重启，停止时只保存。
5. `auto_start_kernel` 只决定应用启动时是否启动内核。
6. guard 只在期望状态为运行时执行自愈。
7. 前端只消费统一快照，并提供明确的启动、停止、重启交互。
8. 用测试和架构约束防止直接调用底层生命周期函数重新出现。

### 2.2 非目标

- 不在本次重构中引入新的状态管理或消息队列依赖。
- 不将当前运行状态持久化到数据库。
- 不顺带重构订阅解析、配置生成或代理 API 的非生命周期逻辑。
- 不引入复杂的分布式状态机框架。
- 不默认实现“取消启动”等新功能；如有需要另行设计。

## 3. 必须满足的产品语义

| 场景 | 内核原来运行 | 内核原来停止 |
| --- | --- | --- |
| 修改普通设置 | 热应用或保持运行 | 保存，保持停止 |
| 修改端口/TUN/DNS | 重启后生效 | 保存，保持停止 |
| 切换活动订阅 | 切换并重启 | 只切换配置，保持停止 |
| 刷新当前订阅 | 内容变化时按需重启 | 保持停止 |
| 安装或更新内核 | 更新后重启 | 保持停止 |
| 用户点击启动 | 幂等保持运行 | 启动 |
| 用户点击停止 | 停止 | 幂等保持停止 |
| 用户点击重启 | 重启 | 拒绝并提示内核未运行 |
| 应用启动 | 仅由 `auto_start_kernel` 决定 | — |
| 内核异常退出 | 期望运行时自愈 | 期望停止时不启动 |

核心约束：**配置变更只能改变配置；只有显式用户操作和应用启动策略可以改变期望启停状态。**

## 4. 目标状态模型

### 4.1 期望状态

```rust
enum KernelDesiredState {
    Running,
    Stopped,
}
```

`KernelDesiredState` 是会话级内存状态：

- 应用启动时由 `auto_start_kernel` 初始化。
- 用户点击启动后设为 `Running`。
- 用户点击停止后设为 `Stopped`。
- 配置、订阅和内核文件更新不得修改它。
- 应用重启后重新从启动策略初始化，不写入数据库。

### 4.2 观测状态

```rust
enum KernelObservedState {
    Stopped,
    Starting,
    Running,
    Degraded,
    Stopping,
    Failed,
    Crashed,
}
```

观测状态由进程句柄、平台进程探测、API readiness 和 relay readiness 共同决定。`Degraded` 表示进程存在但 API 或 relay 未就绪。

### 4.3 配置影响等级

```rust
enum KernelChangeImpact {
    PersistOnly,
    HotApply,
    RestartIfRunning,
}
```

- `PersistOnly`：只持久化，不触碰运行态。
- `HotApply`：允许调用稳定、幂等的运行时接口，但不得启动内核。
- `RestartIfRunning`：当前运行时重启，当前停止时只保存。

### 4.4 生命周期请求与动作

协调器接收语义化请求：

```rust
enum KernelRequest {
    UserStart,
    UserStop,
    UserRestart,
    ApplyRuntimeChange {
        impact: KernelChangeImpact,
        reason: String,
    },
    StartupReconcile {
        auto_start: bool,
    },
    ProcessCrashed,
    Shutdown,
}
```

纯规划函数根据请求和当前状态生成动作：

```rust
enum KernelAction {
    Noop,
    Start,
    Stop,
    Restart,
    HotApply,
    ApplyConfigOnly,
    Reject,
}
```

## 5. 目标架构

所有可能影响生命周期的入口统一提交给 `KernelLifecycleCoordinator`：

```text
Dashboard / Tray / App startup
Settings / Subscription / Kernel update
Guard / TUN self-heal / Shutdown
                  |
                  v
       KernelLifecycleCoordinator
       - single serialized queue
       - desired state
       - transition planner
       - operation generation
                  |
                  v
 process control / system proxy / relay / guard
                  |
                  v
       KernelLifecycleSnapshot event
```

协调器必须负责：

- 串行处理所有生命周期请求。
- 在执行请求时读取最新状态，不依赖入队时的陈旧快照。
- 管理 desired/observed state。
- 生成 `op_id` 和严格递增的 `state_version`。
- 调用底层 start/stop/restart。
- 控制系统代理、事件 relay 和 guard 的启动及清理顺序。
- 在每次有效状态过渡后发送统一快照。
- 将失败归一化为可诊断、可展示的结构。

## 6. 统一生命周期快照

后端命令和事件统一使用一个快照模型：

```rust
struct KernelLifecycleSnapshot {
    desired_state: KernelDesiredState,
    observed_state: KernelObservedState,
    process_running: bool,
    api_ready: bool,
    relay_ready: bool,
    readiness: KernelReadinessSnapshot,
    operation: Option<KernelOperationMeta>,
    last_failure: Option<KernelFailure>,
    state_version: u64,
}
```

迁移期间可以保留 `process_running`、`api_ready` 和 `websocket_ready` 兼容字段；前端完成迁移后再评估是否删除。

## 7. 实施阶段

### 阶段 0：现状基线与一次性探针

在正式实现前通过 disposable spike 验证尚不确定的运行边界。探针不作为最终实现直接合入。

验证场景：

1. 停止后保存端口设置是否启动内核。
2. 停止后调用 `set_active_config_path` 是否启动内核。
3. `auto_start_kernel=false` 时应用启动是否仍启动内核。
4. stop 与 runtime apply 并发时最终状态。
5. 内核异常退出后的 guard 行为。
6. 停止过程中的系统代理、relay、guard 清理顺序。
7. 真正退出、隐藏到托盘和轻量模式的行为差异。

产出：

- 当前行为矩阵和预期行为矩阵。
- 可稳定复现问题的 Rust 测试或临时测试入口。
- 对轻量关闭语义和失败重试语义的明确决策。

### 阶段 1：增加生命周期模型和纯规划测试

主要文件：

- `src-tauri/src/app/core/kernel_service/state.rs`
- 新增或现有对应测试文件

工作内容：

1. 增加 desired/observed/change-impact/request/action 类型。
2. 实现无 IO 的 `plan_transition`。
3. 用表驱动测试覆盖所有状态和请求组合。
4. 暂不改变生产调用链。

最低测试覆盖：

- stopped + config change -> `ApplyConfigOnly`
- running + restart-required change -> `Restart`
- desired stopped + crash -> `Noop`
- desired running + crash -> `Start`
- stopped + user restart -> `Reject`
- 重复 start/stop 的幂等性
- starting/stopping 时后续请求的排队结果

### 阶段 2：将现有队列升级为唯一协调器

主要文件：

- `src-tauri/src/app/core/kernel_service/orchestrator.rs`
- `src-tauri/src/app/core/kernel_service/lifecycle.rs`
- `src-tauri/src/app/core/kernel_service/state.rs`

工作内容：

1. 让协调器持有会话级 desired state。
2. 队列从任意闭包请求迁移为语义化 `KernelRequest`。
3. 将底层 start/stop/restart 限制为协调器内部调用。
4. 每次状态过渡递增 `state_version`。
5. 每次请求结束时发送统一 snapshot。
6. 先兼容现有 command 返回结构，降低前后端同时迁移风险。

验收条件：生产代码中没有协调器之外的底层生命周期调用。

### 阶段 3：拆除 auto-manage 的隐式启动语义

主要文件：

- `src-tauri/src/app/core/kernel_auto_manage.rs`
- `src-tauri/src/app/runtime/orchestrator.rs`
- `src-tauri/src/app/runtime/change.rs`

工作内容：

1. 将 auto-manage 拆成启动 reconcile 和保持状态的 runtime apply。
2. `RuntimeActionPlan` 使用 `KernelChangeImpact`，不再使用 `auto_manage_kernel`。
3. 将 `force_restart` 替换为 `RestartIfRunning` 语义。
4. 禁止配置 apply 在 stopped 状态启动内核。
5. 保留旧 API 适配层，直至所有调用者迁移完成。

### 阶段 4：重构配置应用路径

主要文件：

- `src-tauri/src/app/runtime/app_config_commands.rs`
- `src-tauri/src/app/runtime/config_update.rs`
- `src/components/common/PortSettingsDialog.vue`
- `src/views/SettingView.vue`
- `src/views/setting/useAdvancedSettingsForm.ts`

字段影响分类：

- `PersistOnly`：开机自启、托盘行为、UI 无关配置、`auto_start_kernel`。
- `HotApply`：经过验证可以稳定热应用的代理状态。
- `RestartIfRunning`：端口、TUN、DNS、Fake DNS、配置生成选项和活动配置路径。

统一流程：

```text
保存数据库
-> patch 当前配置文件
-> 提交 ApplyRuntimeChange
-> 协调器根据状态决定 restart / hot apply / no-op
```

停止状态下不得执行 start。

### 阶段 5：重构订阅生命周期

主要文件：

- `src-tauri/src/app/network/subscription_service.rs`
- `src/services/subscription-service.ts`
- `src/views/SubView.vue`
- `src/boot/useAppBootstrap.ts`

工作内容：

1. 下载订阅默认只负责下载和持久化。
2. 将“下载”和“应用”定义为两个明确动作。
3. `set_active_config_path` 只修改活动路径并报告 `RestartIfRunning`。
4. 删除后端下载 apply 后前端再次 `setActiveConfig` 的重复链路。
5. bootstrap 仅在实际路径不一致时同步活动配置。
6. bootstrap 同步配置不得改变 desired state。
7. 自动刷新仅在活动订阅内容实际变化且内核运行时触发重启。

### 阶段 6：修正应用启动和退出流程

主要文件：

- `src-tauri/src/lib.rs`
- 托盘/窗口退出相关后端模块

目标启动顺序：

```text
初始化存储
-> 恢复活动配置
-> 清理残留的受管进程
-> 初始化 lifecycle coordinator
-> 读取 auto_start_kernel
-> StartupReconcile
-> 启动后台任务
```

规则：

- `auto_start_kernel=false`：desired 初始化为 `Stopped`，不启动内核和 guard。
- `auto_start_kernel=true`：desired 初始化为 `Running`，校验配置后启动。
- 启动失败时保留明确的 desired/observed/failure 信息。
- 真正退出时提交 `Shutdown`，等待代理、relay、guard 和进程清理。
- 隐藏到托盘不得改变 desired state。
- 轻量模式必须单独定义是否停止内核，不能复用模糊关闭逻辑。

### 阶段 7：迁移 guard 与 TUN 自愈

主要文件：

- `src-tauri/src/app/core/kernel_service/guard.rs`
- 相关后台健康检查模块

工作内容：

1. `auto_start_kernel` 不再映射为 `keep_alive`。
2. guard 只在 desired 为 `Running` 时工作。
3. guard 检测到退出后只提交 `ProcessCrashed`，不得直接 restart。
4. TUN 自愈通过协调器提交重启请求。
5. 保留并测试 cooldown、失败阈值和连续失败诊断。
6. 用户 stop 后必须先更新 desired，再停止 guard 和进程，消除自愈竞态。

如需让用户控制 keep-alive，应单独设计 `kernel_keep_alive`，不能复用 `auto_start_kernel`。

### 阶段 8：统一前端状态和 Dashboard

主要文件：

- `src/stores/kernel/KernelStore.ts`
- `src/composables/useKernelStatus.ts`
- `src/views/HomeView.vue`
- `src/components/layout/MainLayout.vue`
- `src/stores/tray/TrayStore.ts`

工作内容：

1. KernelStore 只消费统一 snapshot。
2. 删除或只读化 AppStore `isRunning` 镜像。
3. Dashboard 分开展示期望状态和实际状态。
4. 根据 observed state 控制按钮：
   - `Stopped`：只显示启动。
   - `Starting` / `Stopping`：禁用生命周期按钮。
   - `Running` / `Degraded`：显示停止和重启。
   - `Failed` / `Crashed`：显示重试和停止。
5. stopped 状态下不再显示可执行的重启按钮。
6. 设置操作根据结果提示“已保存，稍后生效”“已热应用”或“内核已重启”。
7. 托盘动作复用 KernelStore action，不维护第二套状态判断。

### 阶段 9：删除兼容层并增加架构治理

确认所有调用者迁移完成后删除：

- `kernel_auto_manage` 命令和旧结果模型。
- `AutoManageOptions`。
- 启动命令中无效的 `keep_alive` 参数。
- `RuntimeActionPlan.auto_manage_kernel`。
- 前端重复的运行状态镜像。
- 重复的订阅 apply 调用。
- 无调用者的直接 lifecycle 内部入口。

增加架构测试，禁止协调器外出现底层 start/stop/restart 调用。统一 Rust、前端和 SQLite 中 `auto_start_kernel` 的默认值；默认开启或关闭由产品决策确定，但三处必须一致。

## 8. 测试计划

### 8.1 Rust 单元测试

- 所有 desired/observed/request 组合。
- 重复 start/stop 幂等性。
- stopped 状态 restart 的拒绝语义。
- operation id 和 state version 单调性。
- failed/crashed/degraded 的转换和恢复。
- 配置影响分类。

### 8.2 Rust 集成测试

1. stop -> 保存端口 -> 保持停止。
2. stop -> 切换订阅 -> 保持停止。
3. stop -> 更新内核 -> 保持停止。
4. running -> 保存端口 -> 只重启一次。
5. `auto_start=false` -> app startup -> 不启动。
6. `auto_start=true` -> app startup -> 只启动一次。
7. stop 与 config apply 并发 -> 最终停止。
8. crash + desired running -> 自愈。
9. crash + desired stopped -> 不自愈。
10. 失败后 snapshot 与真实进程一致。
11. 真正退出会清理系统代理、relay、guard 和进程。
12. 隐藏到托盘不改变 desired state。

### 8.3 前端验证

- Dashboard 按钮和状态文案。
- snapshot 初始化与事件增量衔接。
- 乱序事件不会覆盖新状态。
- stopped 状态保存设置不会出现启动提示。
- 订阅下载、应用和刷新行为区分。
- 托盘和 Dashboard 控制结果一致。

### 8.4 质量门禁

```bash
pnpm lint
pnpm type-check
cd src-tauri && cargo test
cd src-tauri && cargo clippy
```

## 9. 风险与控制措施

### 9.1 启动时序变化

风险：数据库恢复、订阅刷新和 coordinator 初始化顺序错误可能使用旧配置启动。

控制：启动流程只保留一个 `StartupReconcile`，并放在配置恢复和升级刷新之后。

### 9.2 系统代理残留

风险：stop 或失败路径未关闭系统代理会导致系统流量指向不存在的端口。

控制：将代理清理纳入 coordinator 的 stop/shutdown/failure 收尾，并增加 fake system proxy 断言。

### 9.3 事件兼容

风险：前端迁移期间新旧事件并存导致状态覆盖。

控制：后端先增加 snapshot 兼容字段，前端切换完成后再删除旧事件；所有事件携带 `state_version`。

### 9.4 守护竞态

风险：用户 stop 与 guard restart 同时发生。

控制：guard 只能提交请求；stop 先将 desired 设为 `Stopped`，队列处理 crash 时重新检查 desired。

### 9.5 重构范围过大

风险：同时修改启动、设置、订阅、托盘和 guard，回归面较大。

控制：保持兼容 API，按阶段小提交迁移；每个阶段均可独立通过测试并回滚。

## 10. 建议提交拆分

1. 增加生命周期模型和纯规划测试。
2. 引入统一 coordinator，暂时保持旧调用行为。
3. 将显式 start/stop/restart 迁入 coordinator。
4. 将 runtime apply 改为 `RestartIfRunning`。
5. 修复应用启动时 `auto_start_kernel` 语义。
6. 迁移 guard 和 TUN 自愈。
7. 重构订阅下载与应用链路。
8. 统一前端 snapshot 和 Dashboard。
9. 删除旧 auto-manage API 和重复状态。
10. 增加架构治理测试并更新开发文档。

## 11. 最终验收标准

- 用户停止内核后，修改任何配置、刷新或切换订阅、更新内核都不会自动启动。
- `auto_start_kernel=false` 时应用启动不会拉起内核。
- 运行中的内核在需要重启的配置变化后只重启一次。
- 所有生命周期操作均通过同一协调器串行执行。
- 用户 stop 与配置 apply、guard crash 并发时，最终保持停止。
- Dashboard、Header 和托盘显示同一个权威状态。
- stopped 状态不提供含糊的重启操作。
- 系统代理、relay、guard 和进程在 stop/shutdown 后均完成清理。
- 生命周期相关单元测试、集成测试、lint、type-check 和 clippy 全部通过。

## 12. 实施结果与决策

本计划已于 2026-07-11 完成实现与回归验证。关键落点如下：

- 已删除 `kernel_auto_manage`、`keep_alive` 和 `force_restart` 的隐式启停语义；配置路径统一使用 `KernelChangeImpact`。
- `KernelDesiredState` 与 `KernelObservedState` 由后端维护；启动、停止、重启、启动 reconcile、配置 apply、guard 自愈和内核维护均进入同一串行操作队列。
- stopped 状态的配置、订阅、TUN/代理设置和内核更新只保存或维护，不会启动内核；`RestartIfRunning` 会以实时进程状态为准，避免陈旧快照触发启动。
- `auto_start_kernel` 默认关闭，且仅在应用启动时决定 desired state；前端设置、Rust 默认值和 SQLite 默认值已对齐。
- Dashboard 显示实际状态与期望状态，stopped 时不展示重启；托盘复用同一内核状态来源。
- guard 仅在 desired 为 `Running` 时自愈，且其恢复操作进入串行队列，用户 stop 可阻止后续自愈。

产品决策：

1. 新安装默认不启动内核（`auto_start_kernel=false`）。
2. 轻量关闭沿用既有行为：仅隐藏/释放窗口，不改变内核期望状态。
3. 不引入通用启动失败重试；仅保留 desired-running 下 guard 的既有自愈与 cooldown 机制。
4. 不提供独立 keep-alive 配置，guard 是 desired-running 的内部保障。
5. stopped 状态隐藏重启操作；后端调用则返回“内核未运行，无法执行重启”。

验证记录：

```bash
pnpm lint                                    # passed
pnpm type-check                              # passed
cd src-tauri && cargo test --lib -- --test-threads=1  # 655 passed
cd src-tauri && cargo test --test e2e_subscription_runtime -- --test-threads=1 # 3 passed
cd src-tauri && cargo test --test e2e_storage_backup -- --test-threads=1       # 2 passed
cd src-tauri && cargo clippy                 # passed
```
