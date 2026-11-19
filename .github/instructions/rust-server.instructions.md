---
description: "Use when editing the Rust control plane or database: adding /api/v1 endpoints, task state machine and transitions, task events/SSE timeline, scheduler claims and leases, watchdog quality gates, node WebSocket protocol, ACP runner, SQLx migrations and models."
applyTo: ["src/**/*.rs", "tests/**/*.rs"]
---

# Rust 后端约定

## 新增 HTTP 端点

1. 在 `src/server/api/` 新建（或复用）资源文件，末尾实现 `pub fn routes(cfg: &mut web::ServiceConfig)`。
2. 在 `src/server/api/mod.rs` 里加 `pub mod` 和 `cfg.configure(<mod>::routes)` —— 这是唯一注册入口。
3. handler 签名惯例：`async fn h(state: web::Data<AppState>, user: AuthUser, path: web::Path<String>, body: web::Json<FooReq>) -> ApiResult<impl Responder>`。
4. 请求体结构体放同文件，`#[serde(rename_all = "camelCase")]`；响应用 `models.rs` 的行结构 + `json!({...})` 包裹。

错误：统一 `ApiError::{bad_request, unauthorized, forbidden, not_found, conflict}`，`?` 自动转换 `sqlx::Error`（`RowNotFound`→404，其余→500 并 `tracing::error!`）。`thiserror` 虽在依赖里但未使用，不要新引入错误枚举。

行模型：`src/server/models.rs` 中 `#[derive(FromRow, Serialize)] #[serde(rename_all = "camelCase")]`，字段名=列名，`TEXT`→`String`、可空→`Option<String>`、`INTEGER`→`i64`。

## 状态机（`src/server/domain.rs`）

- 状态：`planning, ready, in_progress, review, blocked, done, archived, cancelled`（`cancelled` 有 CHECK 允许但**无泳道映射**，仅 planning/ready 可进入）。
- `move_task(&state, task_id, to, actor, MoveOpts)` 内依次处理：`expected_version` 不匹配→409；同状态幂等返回（`in_progress` 例外）；`!legal`→409；`actor:user:*` 且 `!user_can`→403；`to=="done"` 且 `pr_state != "merged"` 且未 `manual_confirm`→409。
- actor 字符串约定：`user:<uuid>` / `agent:<id>` / `system`。
- 副作用都写在 `move_task` 里（`blocked_from`、`review→ready` 时 `retry_count+1`、泳道解析、`version+1`、清租约），不要在外层重复实现。

## 事件（`src/server/events.rs`）

- 追加：`events::append(&state.db, &state.broadcast, task_id, kind, actor, run_id, payload)`；已存事务内取 `MAX(seq)+1`，`UNIQUE(task_id, seq)` 保证单调，广播失败不算错误。
- `kind` 用点号命名：`task.created` / `task.moved` / `agent.message` / `agent.thought` / `tool.call` / `tool.result` / `plan.updated` / `usage.updated` / `comment.added` / `permission.requested` / `watchdog.loop` / `pr.merged_detected`。
- 事件是唯一事实来源：时间线、SSE、摘要、质量阀都读 `task_events`。不要在 `tasks` 表加"进度"冗余列。
- SSE（`api/sse.rs`）：`EventSource` 不能带 Header，鉴权走 `?token=`（`api/mod.rs::claims_from_req`）；续传用 `Last-Event-ID` 或 `?after=`。新增流式需求请复用该模式。

## 调度 / 租约 / 质量阀

- `scheduler::spawn` 跑两个循环：认领 tick 2s、租约扫描 5s。认领必须是带 `AND status='ready'` 的原子 UPDATE 并检查 `rows_affected()`。
- 租约 30s（`gitflow::lease_deadline`），心跳 10s；租约丢失重试 ≤3 次后 `blocked`。
- `watchdog.rs` 阈值在 `Gates::default`：loop_window 5、silence 120（实际按 `silence*2` = 240s 熔断）、no_progress 600s、max_turns 200、max_wall 3600s；工作区 `settings.gates` → 智能体 `gates` 逐级覆盖。
- 后台循环写法：`tokio::spawn(async move { loop { ...; sleep } })`，错误只 log 不退出。`dispatch` 因与 `finish_run` 递归必须返回 boxed future。

## 迁移与数据库

表结构、列类型、CHECK 取值、索引等细节见 [migrations.instructions.md](migrations.instructions.md)。要点：
- 主键 `id TEXT PRIMARY KEY`（`crate::new_id()`）；时间戳 `TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))`；业务表带 `workspace_id ... ON DELETE CASCADE`；JSON 列一律 `TEXT` + `serde_json` 手动序列化（项目不使用 SQLite JSON1）。
- 工作区创建在 `api/workspaces.rs::create_workspace` 同一事务内插 7 条泳道 + 5 个默认任务类型；改 `DEFAULT_LANES` / `DEFAULT_TASK_TYPES` 必须同步 `tests/api.rs` 里"7 条泳道"的断言。

## 节点通道

`src/proto.rs` 的 `ServerMsg` / `RuntimeMsg` 是 `#[serde(tag = "type", rename_all = "snake_case")]` 枚举，改动须同时更新 `src/cmd/fux-runtime.rs` 与 `src/server/nodes_ws.rs`。runtime 走 `POST /api/v1/nodes/pair` 后连 `ws /api/v1/node-channel?token=`，不参与 JWT。

## 测试

- 新增集成测试写在 `tests/api.rs`，复用 `setup()` / `app!` / `call()` / `register()` / `auth()` / `create_ws()`；测试构造 `AppState` 而不启动 `server::run()`。
- 需要真实二进制的 e2e 用 `env!("CARGO_BIN_EXE_fux-mock-agent")` 创建 CLI 条目；后台循环在测试内不会被回收，断言请用轮询 + 超时。

## 二进制

`Cargo.toml` 是 `autobins = false`、edition 2024。新增二进制必须同时写 `[[bin]]`（源码放 `src/cmd/`），否则不会构建。错误处理分层：边界层（main / runner / acp / git）用 `anyhow::Result`，HTTP 层用 `ApiResult`。