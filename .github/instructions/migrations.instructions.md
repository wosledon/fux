---
description: "Use when writing or changing SQLx migrations and table schemas: adding tables/columns/indexes, choosing column types for ids/timestamps/JSON, CHECK enum constraints, tenant workspace_id columns, and keeping models.rs FromRow structs in sync."
applyTo: ["migrations/**"]
---

# 迁移约定

迁移在 `server::db::init` 启动时由 `sqlx::migrate!("./migrations")` 自动执行，随后跑 `seed()` 写入 5 个内置 CLI 条目与本地节点。

## 命名与顺序

- 文件名 `000N_简短说明.sql`，N 递增；**不要修改已发布的 `0001_init.sql`**，需要变更就加新文件。
- 单文件内可写多语句；SQLite 不支持 `ALTER TABLE ... DROP COLUMN` 等复杂变更，改结构请「建新表 → 迁数据 → 删旧表 → 改名」。
- 加 `NOT NULL` 列必须给 `DEFAULT`，否则对已有行会失败。

## 列类型约定（全库一致）

| 用途   | 写法                                                                                       |
| ------ | ------------------------------------------------------------------------------------------ |
| 主键   | `id TEXT PRIMARY KEY`（值为 `crate::new_id()` 的 UUID v4）                                 |
| 时间戳 | `TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))`                             |
| 租户   | `workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE`                   |
| 枚举   | `TEXT NOT NULL CHECK (x IN ('a','b'))`                                                     |
| JSON   | `TEXT NOT NULL DEFAULT '{}'`（或 `'[]'`），由 `serde_json` 手工读写，**不用** SQLite JSON1 |
| 布尔   | `INTEGER NOT NULL DEFAULT 0`                                                               |

时间戳是固定毫秒 UTC 字符串，因此可以直接字符串比较排序（调度器、通知都依赖这一点）。写入时用 `crate::now()`，与 `DEFAULT` 格式保持一致。

## 枚举取值

- `workspace_members.role`：`owner | admin | member`
- `tasks.status`：`planning | ready | in_progress | review | blocked | done | archived | cancelled`
- `lanes.kind`：`planning | ready | work | review | blocked | done | archived`
- `projects.kind`：`git | local`
- `workflows.stages` 为节点 DAG JSON，无 CHECK；`runs.status` 为自由字符串（running/finished/failed/cancelled/skipped）
- `mcp_servers.transport`：`stdio | sse | http`

改这些取值必须同步：`server/domain.rs`（`legal` / `user_can` / `lane_kind`）、`migrations` 的 CHECK、前端 `api.ts` 的 `TaskStatus` / `LaneKind` 与 `STATUS_LABEL`。

## 索引

- 事件表靠 `UNIQUE(task_id, seq)` 保证单调，时间线查询按 `task_id` 过滤，勿删该约束。
- 新增按 `workspace_id` / `status` 过滤的列表查询时，补对应索引（参考 `idx_tasks_board_status`、`idx_tasks_status`、`idx_runs_task`）。

## 改完必须同步的地方

1. `src/server/models.rs`：加 `#[derive(FromRow, Serialize)] #[serde(rename_all = "camelCase")]` 行结构，字段名与列名一致，`TEXT`→`String`、可空→`Option<String>`、`INTEGER`→`i64`。
2. `tests/api.rs`：受影响用例（尤其 `create_ws` 的默认泳道断言）。
3. 前端 `frontend/src/api.ts` 的接口定义；JSON 类列在页面里用 `parseJson` 读取。
