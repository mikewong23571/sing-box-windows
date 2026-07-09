# Backend Runtime Refactor Plan

## 目标

后端重构的核心目标不是减少文件数量，而是让运行时行为符合直觉：

- 保存配置只表示持久化，除非调用方明确请求应用运行态。
- 订阅服务只负责获取、解析、生成配置，不直接决定内核生命周期。
- 内核生命周期只有一个编排入口，负责 start / stop / restart / apply。
- 内核状态只有一个权威来源，事件和命令返回都从该来源生成快照。
- Tauri command 只做参数转换和调用服务，不承载跨模块业务流程。

## 当前主要问题

1. `storage::enhanced_storage_service` 同时承担数据库读写、活动配置 patch、内核自动管理，存储层存在运行时副作用。
2. `network::subscription_service` 同时做订阅解析、配置文件生成、活动配置切换、运行态应用。
3. `core::kernel_service::runtime` 同时包含命令入口、配置解析、进程生命周期、API readiness、事件中继、守护任务、状态广播。
4. 前后端对 starting / stopping / running 的推导来源不唯一，容易出现 UI 状态和真实运行态不一致。

## 目标模块

### `storage`

职责：

- SQLite 读写。
- AppConfig / Subscription / WindowConfig 等状态模型持久化。
- 持久化前的数据规范化。

边界：

- 不启动、停止、重启内核。
- 不 patch sing-box 配置文件。
- 不调用代理服务、订阅服务、内核服务。

### `singbox`

职责：

- 生成 sing-box 配置。
- 将 AppConfig 的设置同步到 sing-box JSON。
- 提供配置 schema、settings patch、公共 tag / DNS helper。

边界：

- 不读取数据库。
- 不启动内核。
- 不发 Tauri 事件。

### `runtime`

职责：

- 运行时变更的唯一编排层。
- 决定某个变更是仅保存、仅 patch 配置、应用系统代理、启动内核，还是重启内核。
- 连接 `storage`、`singbox`、`core::kernel_service`，但不直接实现底层进程管理。

建议结构：

```text
src-tauri/src/app/runtime/
├── mod.rs
├── app_config_commands.rs   # db_save_app_config 等兼容 Tauri command 包装
├── config_update.rs         # 活动配置 patch + auto manage 入口
├── change.rs                # RuntimeChange / RuntimeApplyOptions / RuntimeApplyResult
└── orchestrator.rs          # apply_runtime_change，后续统一入口
```

第一阶段只落地 `app_config_commands.rs` 和 `config_update.rs`，其余文件等调用链收敛后再加，避免空抽象。

### `core::kernel_service`

职责：

- 内核进程生命周期。
- API readiness 校验。
- WebSocket/Tauri 事件中继。
- kernel guard / crash recovery。
- `KERNEL_STATE` 状态维护和状态快照生成。

目标结构：

```text
src-tauri/src/app/core/kernel_service/
├── runtime.rs          # 临时保留 command 兼容入口，逐步瘦身
├── lifecycle.rs        # start / stop / restart 底层生命周期
├── readiness.rs        # API readiness / startup stability
├── relay.rs            # event relay task 生命周期
├── guard.rs            # keep-alive / crash recovery
├── state.rs            # KERNEL_STATE，唯一状态源
└── status.rs           # status command 和快照查询
```

### `network::subscription_service`

职责：

- 下载订阅。
- 解析订阅。
- 生成订阅配置文件。
- 更新订阅元信息。

边界：

- 不直接调用内核 start / restart。
- 切换 active config 和应用运行态最终交给 `runtime`。

目标结构：

```text
src-tauri/src/app/network/subscription_service/
├── parser.rs
├── materializer.rs     # 生成/写入订阅配置文件
├── manager.rs          # 订阅列表和用户信息维护
├── mode.rs             # global/rule runtime config
└── auto_update.rs
```

## 分批计划

当前状态：

- Batch 1 已完成：存储层只保留持久化职责，带运行时副作用的 command 已迁入 `runtime`。
- Batch 2 已完成：`RuntimeChange` / `RuntimeApplyOptions` / `apply_runtime_change` 已成为运行时变更入口。
- Batch 3 已完成：订阅配置生成已抽到 `subscription_service/materializer.rs`，订阅应用运行态经由 `runtime`。
- Batch 4 已完成：内核生命周期实现迁入 `kernel_service/lifecycle.rs`，readiness 迁入 `readiness.rs`，relay 兼容入口迁入 `relay.rs`，`runtime.rs` 仅保留兼容重导出。
- Batch 5 已完成：内核 lifecycle 事件 payload 由 `KERNEL_STATE` 快照生成，前端状态展示不再用本地 loading 推导 starting/stopping。
- Batch 6 已完成：新增架构边界测试和 runtime 决策测试。

### Batch 1: 存储层去运行时副作用

目标：

- 从 `storage::enhanced_storage_service` 移出 `apply_runtime_config_update`。
- 将 `db_save_app_config` Tauri command 移到 `runtime::app_config_commands`。
- 将带配置文件同步副作用的 `db_save_active_subscription_index` command 移到 `runtime::app_config_commands`。
- 保留前端命令名 `db_save_app_config`，避免前端改动。

验收：

- `db_save_app_config_internal` 只做持久化。
- `storage` 不再依赖 `core::kernel_auto_manage`。
- `storage` 不再直接 patch sing-box 配置文件。
- 现有行为保持不变：`applyRuntime=true` 仍会 patch 活动配置并按需重启。

### Batch 2: 明确运行时变更模型

目标：

- 新增 `RuntimeChange`：
  - `AppConfigUpdated`
  - `ActiveConfigChanged`
  - `SubscriptionApplied`
  - `ProxySettingsChanged`
  - `KernelUpdated`
- 新增 `RuntimeApplyOptions`：
  - `force_restart`
  - `patch_active_config`
  - `use_original_config_hint`
  - `reason`

验收：

- 新代码不再直接调用 `auto_manage_with_saved_config`，统一走 `runtime::orchestrator`。
- 旧函数保留兼容，但内部转发到新入口。

### Batch 3: 订阅服务去运行时编排

目标：

- `download_subscription` / `add_manual_subscription` 只返回生成结果。
- active config 切换和运行态应用交给 `runtime`。
- 抽出 `materializer`，把订阅内容到配置文件的逻辑从 command 中移出。

验收：

- 订阅服务可以在不启动内核的情况下单测配置生成。
- active config 改变是否重启由 runtime 统一决定。

### Batch 4: 内核服务瘦身

目标：

- 将 `core::kernel_service::runtime` 拆出 lifecycle / readiness / relay。
- `runtime.rs` 暂时只保留 Tauri command 和兼容导出。

验收：

- start / stop / restart 的状态转移集中在 lifecycle。
- readiness 失败、relay 失败、process exited early 的错误语义更清晰。

### Batch 5: 统一状态快照和事件

目标：

- 所有内核事件 payload 从 `KERNEL_STATE` 快照生成。
- 前端只消费后端状态，不用本地 loading 推导真实运行态。

验收：

- 标题栏、托盘、主页状态一致。
- 重启、崩溃恢复、事件中继重连后状态可恢复。

### Batch 6: 治理和回归

目标：

- 给关键边界补低成本测试。
- 对 recurring bad pattern 增加可执行检查。

候选规则：

- `storage` 禁止依赖 `core::kernel_*`。
- `storage` 禁止依赖 `network`。
- 新增 runtime orchestrator 单测覆盖常见决策。

## 重构约束

- 每批都必须可独立 review、可独立回滚。
- 先搬边界，再改行为；除非某批明确声明行为变化。
- 保留 Tauri command 名称兼容前端。
- 每批至少跑：
  - `pnpm type-check`
  - `pnpm lint`
  - `cd src-tauri && cargo test`
  - `cd src-tauri && cargo clippy`
