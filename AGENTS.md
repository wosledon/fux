# Fux — AI 工作台

产品定位、核心原则、技术栈、环境变量、5 分钟演示见 [README.md](README.md)；需求与领域模型见 [docs/PRD.md](docs/PRD.md)。本文件只记录「动手改代码前必须知道」的内容。

## 常用命令

```powershell
cargo build            # 产出 fux / fux-runtime / fux-mock-agent / fux-mcp
cargo test             # 14 个测试（7 单元 + 7 集成）；e2e 需要 git 在 PATH，且会真实拉起 mock agent 子进程
cargo test --test api e2e_mock_agent_delivery -- --nocapture   # 只跑一个集成测试
cargo test --lib transition_table                              # 只跑 lib 单测
cd frontend; npm run build    # tsc 双检 + vite build（无 lint 脚本，类型检查是唯一门禁）
cd frontend; npm run dev      # http://127.0.0.1:5173，/api 代理到 8080
```

运行控制面时把 `target\debug` 加入 PATH（CLI 探测靠 `which::which`），可选 `FUX_DATA_DIR` / `FUX_JWT_SECRET` / `FUX_MASTER_KEY`。

## 架构速览

- **后端** Rust（Actix-web + SQLx/SQLite）：`src/main.rs` 控制面入口；`src/server/` 下 `api/`（HTTP 端点）、`domain.rs`（任务状态机）、`events.rs`（事件溯源 + 广播）、`scheduler.rs`（认领/租约/分发）、`watchdog.rs`（质量阀）、`workflow.rs`（工作流 DAG 引擎）、`gitflow.rs`（交付/合并检测）、`nodes_ws.rs` + `src/proto.rs`（节点通道）、`runner.rs` + `src/acp.rs`（本地执行 / ACP client）。
- **前端** React 19 + TypeScript + MUI + TanStack Query + dnd-kit，`frontend/src/pages/` 一页一文件，`api.ts` 集中放请求与手写类型。
- Fux 是 ACP **client**（NDJSON JSON-RPC over stdio）。不自建 agent loop、不接 Git 平台 API、不做集群。

## 设计方向（v0.4）

- 「智能体团队」（`agent_teams` / `team_members` / pipeline / supervisor）**已从产品设计中移除，代码已清理**（`0002_drop_teams.sql`）。
- 替代方案是「工作流」：`workflows` 模板 = 节点 DAG（节点 = 名称 + 智能体 + **执行项目** + 追加提示词 + `dependsOn`），节点可跨项目、依赖满足即并行；产物以「项目 + 分支 + commit」沉淀在 git（分支 `fux/task-<id>/<nodeKey>`），跨机靠下游 `git fetch`。节点推进复用 Run/租约/质量阀，**不新增任务状态**。
- 交付物不再只有一条 PR：一个任务多条 `deliveries`（每个项目一条），全部合并才可批准审核。
- 展示形态：流程实例就是看板上的普通任务卡（角标显示节点进度），详情里有「流程」页签；工作流页负责模板定义与一句话发起。
- 不要再往 teams / 团队面板方向扩展；新增编排类需求一律落在工作流上。

## 全局不变量

1. 任务状态**只能**经 `server::domain::move_task` 变更，且必须落 `task_events`。直接 `UPDATE tasks SET status` 仅存在于调度器原子认领、租约回退等系统路径，不要模仿。
2. 转移合法性由 `domain::legal`（T1–T11）判定，用户可用子集是 `domain::user_can`；非法转移返回 409，用户越权（如 `ready→in_progress`）返回 403。
3. 工作区隔离靠「先 `auth::require(db, workspace_id, user_id, "member"|"admin")` 再按 `workspace_id` 过滤 SQL」，没有中间件/RLS。
4. ID 一律 `crate::new_id()`，时间戳一律 `crate::now()`（固定毫秒 UTC 字符串，可字符串比较排序）。不要直接用 `Uuid::new_v4()` / `Utc::now().to_rfc3339()`。
5. SQL 全部使用 sqlx 运行时 API（`query_as::<_, T>` / `query_scalar`），项目里**没有** `query!` 宏，也无需 `DATABASE_URL` 或离线 `.sqlx` 缓存；列名/类型错误只会在运行时暴露。
6. 对外 JSON 一律 camelCase（`#[serde(rename_all = "camelCase")]`），ID 为 UUID v4 字符串。
7. 注释、日志、错误消息、UI 文案均为中文；`//!` 模块头注释说明职责，沿用此风格。

## 路径专属约定

| 范围                                   | 文件                                                                                                 |
| -------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `src/**`、`tests/**`                   | [.github/instructions/rust-server.instructions.md](.github/instructions/rust-server.instructions.md) |
| `frontend/src/**`                      | [.github/instructions/frontend.instructions.md](.github/instructions/frontend.instructions.md)       |
| `migrations/**`（列类型、CHECK、索引） | [.github/instructions/migrations.instructions.md](.github/instructions/migrations.instructions.md)   |
| 测试怎么跑、怎么加                     | [docs/TESTING.md](docs/TESTING.md)                                                                   |

新增一个「后端端点 + 前端页面 + 集成测试」的完整功能时，按 [.github/skills/add-feature/SKILL.md](.github/skills/add-feature/SKILL.md)（斜杠命令 `/add-feature`）的步骤走。

`.github/hooks/rustfmt-check.json` 会在每次编辑 `src/**`、`tests/**` 下的 `.rs` 后跑 `rustfmt --check`，只提示、不阻塞；提交前仍需 `cargo fmt`。