---
description: "Use when editing the React frontend: pages, workspace layout and routing, api.ts types and request helpers, TanStack Query data fetching, SSE task timeline, MUI theme and shared ui.tsx components, board drag-and-drop."
applyTo: ["frontend/src/**"]
---

# 前端约定（React 19 + MUI + TanStack Query）

代码风格靠约定维持（无 ESLint/Prettier）：2 空格缩进、**无分号**、单引号、`const fn = async () => {}`、中文 UI 文案与注释。`npm run build`（tsc 双检 + vite build）是唯一门禁，提交前必须通过。

## 页面与路由

- 页面放 `frontend/src/pages/XxxPage.tsx`，默认导出 `export default function XxxPage()`；路由在 `src/App.tsx` 注册。
- 工作区内页面必须挂在 `/ws/:wsId` 子路由下，用 `const { wsId, role } = useOutletContext<WsCtx>()` 取上下文（**不要**用 `useParams` 读 wsId）。`WsCtx` 从 `./WorkspaceLayout` 当 `import type` 引入。
- 角色控制 `const isAdmin = role === 'owner' || role === 'admin'` 只用于隐藏按钮，权限以后端为准。

## 数据请求

- 只用 `src/api.ts` 的 `get / post / patch / put / del`，不要手写 `fetch`（AI 助理的流式接口是唯一例外）。
- token 只经 `getToken() / setToken()` 访问；401 已由 `req()` 统一清 token 并硬跳 `/login`，不要重复实现。
- `api.ts` 里的接口类型是**手写**的（无 codegen）：后端字段变化时同步改这里的 `interface`，并留意 camelCase。
- Query key 约定：`['entity', wsId]`（projects/agents/teams/skills/mcps/types/members/board/…）、`['tasks', boardId]`、`['task', taskId]`、`['pr', taskId]`、`['nodes']`、`['workspaces']`。
- **不使用** `useMutation`、乐观更新或 `setQueryData`。写操作统一：`await post(...)` → `qc.invalidateQueries({ queryKey: [...] })` → 成功 toast / 失败 `setErr` 或 `toast(e.message, 'error')`。

## 数据形状陷阱

- 后端大量列是 JSON **字符串**：`Task.tags`、`Workspace.settings`、`Agent.policy/gates`、`CliEntry.args/envTemplate`、`TaskType.variablesSchema/defaults`、`EventEnvelope.payload`。一律 `parseJson(v, fallback)`，不要直接 `JSON.parse` 或 `.map`。
- 响应常被包裹：`GET /board` → `{ board, lanes }`、`GET /tasks/{id}` → `{ task, runs, project? }`、`GET /teams` → `{ team, members }[]`。
- `verbatimModuleSyntax: true`：类型必须 `import type { Task } from '../api'`，混入值导入会编译失败。

## 实时与看板

- 任务时间线：`new EventSource(taskEventsUrl(taskId, 0))`，按 `seq` 去重追加到本地 `useState<Envelope[]>`；`task.*` / `pr.*` / `comment.added` 时 invalidate 相关 query。cleanup 必须 `es.close()`。
- Create 模式下 `useEffect` 执行两次导致的双连接是正常的，不要加守卫。
- 状态流转必须带 `expectedVersion: task.version`（后端乐观锁，冲突返回 409）。
- 看板拖拽在 `pages/BoardPage.tsx`：业务规则集中在纯函数 `dropPlan(task, lane)`，落位分别映射到 `/transition`、`/unblock`、`PATCH { laneId }`，不要在 `onDragEnd` 里另写判断。

## UI

- 共享组件只在 `src/ui.tsx`：`PageHeader`、`EmptyState`、`StatusChip`、`PersonAvatar`、`DeleteButton`（已封装确认 + toast + 错误），以及 `useToast()`、`useConfirm()`。删除操作优先用 `DeleteButton`。
- 颜色语义来自 `src/theme.ts` 的 `LANE_ACCENT` / `STATUS_ACCENT` 与 `STATUS_LABEL`，不要新造色值。
- 表单用多个 `useState` + MUI `Dialog`；列表用 `<Table size="small">`。MUI v7：新代码用 `Grid size={{ xs: 12 }}`，不要混用已弃用的 `item` / `InputProps` 写法。
- 任务 ID 展示用 `#${task.id.slice(0, 8)}`，不要整串上屏。