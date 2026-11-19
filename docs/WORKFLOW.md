# 工作流（Workflow）技术设计

需求见 [PRD 3.6.2 / 3.6.3](PRD.md)。本文是实现细则：数据结构、DAG 判定、调度、租约、产物交接、交付与接口。

## 1. 数据模型

### 1.1 `workflows`

| 列                          | 类型                                         | 说明                     |
| --------------------------- | -------------------------------------------- | ------------------------ |
| `id`                        | TEXT PK                                      | `crate::new_id()`        |
| `workspace_id`              | TEXT NOT NULL → workspaces ON DELETE CASCADE | 隔离边界                 |
| `name` / `description`      | TEXT                                         | 模板名 / 说明            |
| `stages`                    | TEXT NOT NULL DEFAULT `'[]'`                 | 节点数组（JSON，见 1.2） |
| `created_at` / `updated_at` | TEXT                                         | `crate::now()` 格式      |

### 1.2 节点 `StageSpec`

```jsonc
{
  "key": "design-proto",        // 模板内唯一，slug（^[a-z0-9-]{1,40}$）
  "name": "设计 proto",
  "agentId": "uuid",
  "projectId": "uuid|null",     // null = 继承任务主项目
  "promptAppend": "只改 proto/**",
  "dependsOn": [],              // 空数组 = 入口节点
  "when": "tag:backend",        // 可选，见 §3
  "x": 48, "y": 48               // 可选，画布坐标（仅展示/编辑用）
}
```

保存时校验（`workflow::validate`）：

1. 至少 1 个节点；`key` 唯一且匹配 `^[a-z0-9-]{1,40}$`。
2. `dependsOn` 引用的 key 必须存在，且不能自依赖。
3. 无环（DFS 白/灰/黑标记），否则 400。
4. `agentId` 必须属于同一工作区。

### 1.3 `runs`（节点执行实例）

在 0001 基础上新增：

| 列                 | 说明                                               |
| ------------------ | -------------------------------------------------- |
| `project_id`       | 该节点执行的项目（与 `tasks.project_id` 可能不同） |
| `branch`           | `fux/task-<id 前 8 位>/<nodeKey>`                  |
| `head_sha`         | 节点完成时的 commit（产物）                        |
| `stage_key`        | 节点 key（`stage` 仍保留为序号，用于列表排序）     |
| `lease_expires_at` | **Run 级租约**（工作流并行后不再依赖任务级租约）   |

`runs.status` 取值：`running / finished / failed / cancelled / skipped`（保留既有语义，新增 `skipped`）。

### 1.4 `deliveries`（多项目交付）

| 列                                                | 说明                      |
| ------------------------------------------------- | ------------------------- |
| `id`                                              | PK                        |
| `task_id`                                         | → tasks ON DELETE CASCADE |
| `project_id`                                      | 交付所属项目              |
| `node_key`                                        | 产生该交付的终端节点      |
| `branch` / `pr_url` / `pr_state` / `merge_commit` | 交付状态                  |
| UNIQUE(`task_id`,`project_id`,`node_key`)         | 幂等                      |
| INDEX(`task_id`)                                  | 列表查询                  |

## 2. DAG 判定与推进

### 2.1 状态来源

节点状态**不落表**，由 `runs` 派生：取 `task_id + stage_key` 的**最新一条** run（按 `started_at DESC`）：

| 最新 run                      | 节点状态                       |
| ----------------------------- | ------------------------------ |
| 无                            | `pending`                      |
| `running`                     | `running`（可多条，重试/并行） |
| `finished`                    | `succeeded`                    |
| `skipped`                     | `skipped`                      |
| `failed` 且 `retry_count < 2` | `retrying`（等待自动重试）     |
| `failed` 且重试耗尽           | `failed`                       |
| `cancelled`                   | `cancelled`                    |

终态：`succeeded / skipped / failed / cancelled`。

### 2.2 就绪算法（`workflow::ready_stages`）

```
候选 = 所有 latest 状态为 pending 的节点
for 节点 in 模板顺序:
    if 节点.when 存在且求值为 false:
        insert run(status=skipped) + 事件 workflow.stage_skipped
        继续（下游可马上就绪）
    if 任一 dependsOn 的 latest 状态 ∉ {succeeded, skipped}: continue
    启动节点：建 run(running) + dispatch
```

- **并行度由 DAG 结构决定**：同一时刻可运行的节点全部派发，没有并行配置项。
- 触发时机：任务认领后、任一 run 结束后、`skip` / `rewind` 后、以及每 2s 的 `workflow_tick` 兜底（只处理 `in_progress` 且 `workflow_id IS NOT NULL` 的任务）。
- 唯一的并发限制来自执行侧：每个智能体的 `agents.max_concurrent`（按 `runs.status='running'` 计数）。

### 2.3 失败与重试

- `outcome.status != finished` 且 `stop_reason == "interrupted" | "lease_expired"` 且 `retry_count < 2`：复制同 `stage_key` 的 run（`retry_count+1`、`retry_of=旧 run`），重新 dispatch，任务保持 `in_progress`。
- 重试耗尽或其它失败：该 run 置 `failed`；**取消同任务其余 running 的 run**；`move_task(blocked)`，理由带节点 key。
- `cancelled`：取消其余 run 并 `blocked`。
- **过期 Run 保护**：若该节点的最新 Run 不是当前 Run（被重试、跳过或 rewind 覆盖，或已被删除），结束回调直接忽略，不再推进状态。

### 2.4 收敛

全部节点 latest 状态 ∈ {succeeded, skipped}：

1. 计算图的**汇点**（无下游的节点）。每个汇点按 `project_id`（空则任务主项目）生成一条 `deliveries`（取该汇点最新 succeeded run 的 `branch`）。
2. 同项目多个汇点 → 多条交付。
3. `pr_state`：任一交付的 project 有 remote → `open`，否则 `local`；任务 `branch` 写主项目汇点的分支（兼容旧 UI）。
4. `move_task(review)`，事件 `workflow.completed`。

### 2.5 返工、跳过与解除阻塞

| 场景 | 行为 |
| --- | --- |
| 审核驳回（review → ready） | 删除**汇点节点**的 runs 与 deliveries（上游保留），重跑只覆盖最终交付；事件 `workflow.rework_reset` |
| 已完成返工（done → ready，评论触发） | 同上，重置汇点节点 |
| 从节点 N 返工（blocked / review / done） | 取消运行中的 Run，删除 N 及其下游的 runs 与 deliveries；事件 `workflow.rewound`，任务回 ready |
| 人工跳过（in_progress） | 取消该节点运行中的 Run，写入一条 `skipped` run（覆盖旧 Run），继续推进下游 |
| 解除阻塞（blocked → ready/in_progress） | 删除 `failed` / `cancelled` 的 runs（保留成功节点），使失败节点可重新派发；事件 `workflow.retry_reset` |

> 所有删除都保留 `task_events` 历史；节点状态始终以「最新 Run」为准。

## 3. `when` 表达式（声明式）

只允许以下三种，求值失败按 false 处理并写事件：

| 语法                 | 语义                                                        |
| -------------------- | ----------------------------------------------------------- |
| `tag:<name>`         | 任务 `tags` JSON 数组包含 `<name>`                          |
| `path:<substr>`      | 任一直接上游节点产物的变更文件路径包含 `<substr>`           |
| `var:<name>=<value>` | 任务内容里存在 `name: value` 行（任务模板变量回填后的文本） |

不支持布尔组合与函数调用（v1）。

## 4. 分支与产物

- 分支：`fux/task-<id 前 8 位>/<nodeKey>`（`/` 分隔，天然按任务分组）。
- 起点：
  - 入口节点 + 有 `projectId` 的节点 → 该项目 `default_branch`。
  - 下游同项目且**恰好一个**同项目上游节点 → 从该上游分支创建（`git worktree add -b <branch> <dir> origin/<upstream>`；本地无 remote 时用本地分支名）。
  - 同项目多个上游 → 取第一个 succeeded 的同项目上游分支，其余作为参考检出。
- 产物记录：`workflow.stage_completed` 事件 payload：

```jsonc
{ "stageKey": "design-proto", "projectId": "...", "branch": "fux/task-xxxx/design-proto",
  "headSha": "abc123", "files": ["proto/user.proto"], "summary": "…" }
```

- **跨机交接**：下游节点启动前，runtime 对每个上游产物执行
  `git clone <remote> .fux/upstream/<nodeKey>` + `git checkout <headSha>`；
  无 remote 时，仅当上游与本节点是同机路径才直接复用（否则节点选择阶段就排除）。
  提示词追加：

```
## 上游产物
- design-proto: 项目 proto 仓库，分支 fux/task-xxxx/design-proto @ abc123
  路径: /data/worktrees/<task>/.fux/upstream/design-proto
  变更: proto/user.proto
  摘要: …
```

## 5. 调度与租约

| 项                 | 规则                                                                                    |
| ------------------ | --------------------------------------------------------------------------------------- |
| 任务认领（工作流） | `ready → in_progress` 原子 UPDATE；不写任务级 `lease_run_id`，成功后立即 `ready_stages` |
| 节点派发           | 每个节点建 run；`node_id` 按「项目本地性 → CLI 能力 → 负载」独立选择（允许跨机）        |
| Run 租约           | `runs.lease_expires_at = now + 30s`；心跳 10s 续租；后台每 5s 扫描过期 run              |
| 过期处理           | run 置 `failed / stop_reason=lease_expired` → 走 §2.3（自动重试或阻塞）                 |
| 任务级租约         | 仅非工作流任务使用（兼容路径）                                                          |
| 质量阀             | 按 run 判定；熔断 → 取消同任务其余 run → `blocked`，事件 `watchdog.*` 带 `stageKey`     |

## 6. 接口

| 方法   | 路径                            | 说明                                                                        |
| ------ | ------------------------------- | --------------------------------------------------------------------------- |
| GET    | `/workspaces/{id}/workflows`    | 列表（含节点数）                                                            |
| POST   | `/workspaces/{id}/workflows`    | 创建（admin）                                                               |
| GET    | `/workflows/{id}`               | 详情                                                                        |
| PATCH  | `/workflows/{id}`               | 更新（admin）                                                               |
| DELETE | `/workflows/{id}`               | 删除（admin）                                                               |
| POST   | `/workflows/{id}/run`           | `{ title, content, projectId?, typeId?, priority?, tags? }` → 建 ready 任务 |
| GET    | `/tasks/{id}`                   | 增加 `workflow`、`deliveries` 字段                                          |
| POST   | `/tasks/{id}/deliveries`        | 登记/回填交付 PR：`{ deliveryId, prUrl }`                                   |
| POST   | `/tasks/{id}/check-merge`       | 逐条检测全部交付                                                            |
| POST   | `/tasks/{id}/rewind`            | `{ fromStage }`：重置该节点及下游（admin，blocked/review 可用）             |
| POST   | `/tasks/{id}/stages/{key}/skip` | 手动跳过（`in_progress`，admin）                                            |

`move_task` 的 T5 审批条件扩展：有 `deliveries` 时要求**全部** `pr_state='merged'`（或 `manualConfirm`）。

## 7. 前端

- **工作流页** `/ws/:wsId/workflows`：模板列表、画布式创建/编辑（拖拽节点、连线建立依赖、自动布局；节点属性面板编辑 key/名称/智能体/项目/`when`/追加提示词）、顶部「一句话发起」（标题 + 内容 + 目标项目）。
- **看板**：任务卡显示工作流名 chip（`workflowName` 由前端用模板列表映射）。
- **任务详情**：有 `workflowId` 时多一个「流程」页签：
  - 按层展示节点（依赖 → 下游），每节点 chip：`pending / running / succeeded / failed / skipped`；
  - 显示智能体、项目、轮次、耗时、`headSha`；
  - 操作：`从该节点返工`、`跳过`（admin）。
- **审核侧栏**：按项目列出 `deliveries`（分支、PR 链接输入、状态、检测合并）；全部 merged 才启用「批准交付」。
- 删除团队页与所有 team 引用。

## 8. 迁移与兼容

- `0002_drop_teams.sql`：删除 `team_members`、`agent_teams`、`tasks.assignee_team_id`、`runs.team_id`。
- `0003_workflows.sql`：建 `workflows` / `deliveries`；`tasks.workflow_id`；`runs` 新增 `project_id/branch/head_sha/stage_key/lease_expires_at`。
- `0004_drop_max_parallel.sql`：删除 `workflows.max_parallel`（并行度改由 DAG 结构决定）。
- 非工作流任务完全走原路径：单 Run、任务级租约、`tasks.pr_url` 交付字段保持不变。
- 工作流任务的 `tasks.branch` 仅用于主项目汇点展示；真实分支在 `runs.branch` 与 `deliveries.branch`。