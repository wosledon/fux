# Fux — AI 工作台

以「自驱动看板 + 标准工作流」为核心的 AI 工作台：人定义目标、评审结果；智能体按固化工作流认领任务、执行工作、流式交付过程。

设计文档见 [`docs/PRD.md`](docs/PRD.md)（v0.3 决策确认稿）。

## 核心原则

- **不自建 Agent loop**：一切智能体（含 AI 助理）绑定 ACP CLI + 模型 + 推理强度
- **不接 Git 平台 API**：git 原生（worktree / push / 合并检测），凭据用户在节点自理
- **不做集群**：单控制面 + runtime 安装即注册（出站 WS，配对码 + 一键批准）
- **不做成本预算**：质量阀防失控（循环 / 停滞 / 中断 / 硬上限熔断）
- **审核强制 PR/MR**：推分支 → 链接 → `merge-base` 合并检测 → 才可批准
- **不做多智能体编排**：只固化串行工作流模板（节点绑定智能体），不做主管分派 / 条件分支 / 并行

## 技术栈

| 层   | 技术                                                                            |
| ---- | ------------------------------------------------------------------------------- |
| 后端 | Rust · Actix-web · SQLx（SQLite 默认，PG 可选）· tokio                          |
| 前端 | React 19 · TypeScript · MUI (Material Design) · TanStack Query · dnd-kit · Vite |
| 协议 | ACP（JSON-RPC 2.0 over stdio）· SSE（时间线/看板）· WebSocket（节点通道）       |

## 目录结构

```
src/
  main.rs            控制面入口（fux）
  lib.rs             共享库
  acp.rs             ACP Client（NDJSON framing、权限请求、取消）
  git.rs             git 工具（worktree/push/合并检测/PR 链接推导）
  proto.rs           节点通道协议 + CLI 探测
  runner.rs          通用运行器（RunSink 抽象，本地/远程共用）
  server/            控制面：api/ 调度器 质量阀 事件溯源 节点WS 助理
  cmd/               辅助二进制（显式 [[bin]]，autobins=false）
    fux-runtime.rs     节点执行器：安装即注册
    fux-mock-agent.rs  演示/测试用 ACP mock（LOOP/FAIL/SLEEP/ASK 关键字驱动）
    fux-mcp.rs         内置 workspace MCP Server（助理管理工作台）
migrations/          SQLx 迁移
tests/api.rs         集成测试（RBAC/状态机/端到端/重试/质量阀）
frontend/            React 19 + MUI 前端
```

## 快速开始

```powershell
# 1. 构建（产出 fux / fux-runtime / fux-mock-agent / fux-mcp）
cargo build

# 2. 启动控制面（把 target/debug 加入 PATH 以便探测 mock/mcp）
$env:PATH = "$PWD\target\debug;$env:PATH"
$env:FUX_DATA_DIR = "$PWD\data"        # 可选，默认 ./data
$env:FUX_JWT_SECRET = "change-me"      # 生产必设
$env:FUX_MASTER_KEY = "change-me-too"  # 密钥库加密，生产必设
.\target\debug\fux.exe                 # http://0.0.0.0:8080

# 3. 前端
cd frontend
npm install
npm run dev                            # http://localhost:5173（代理 /api → 8080）
```

浏览器打开 http://localhost:5173 → 注册 → 新建工作区。

### 5 分钟演示（mock agent）

1. **项目**页：添加本地目录项目（任意 git 仓库，或空目录自动 init）
2. **智能体**页：新建智能体，CLI 绑定选「Mock（演示/测试）」，审批策略「全自动」
3. **看板**：新建任务（关联项目、指派该智能体）→ 打开抽屉点「规划完成 → 待办」
4. 观察：自动认领 → 进行中 → 时间线流式输出（思考/消息/计划/工具调用）→ git 提交 → 审核中
5. **审核与详情**：无 remote 时为「本地交付」，勾选人工确认 → 批准 → 归档
6. 已完成任务评论 → 自动返工回待办；进行中评论 → turn 结束后注入引导

mock 提示词关键字：`LOOP`=循环抽搐（触发质量阀）`FAIL`=突然中断（自动重试≤2）`SLEEP`=停滞 `ASK`=权限请求。

### 接入真实 CLI

节点上安装并认证任一 CLI（认证用户自理）：

| CLI         | 启动                                     | 认证                                            |
| ----------- | ---------------------------------------- | ----------------------------------------------- |
| Codex       | `codex-acp`（适配器）                    | codex 登录                                      |
| OpenCode    | `opencode acp`（原生）                   | `opencode auth login`                           |
| Pi          | `pi-acp`（桥接 `pi --mode rpc`）         | pi 供应商配置                                   |
| Copilot CLI | `copilot --acp --stdio`（原生，preview） | `/login` 或 `COPILOT_GITHUB_TOKEN`（支持 BYOK） |

在「CLI 接入」页探测可用性，创建智能体时绑定并设置模型/推理强度（OpenCode 走 ACP 配置项，Copilot 走启动参数 `--effort`）。

### 多节点

1. 「节点」页生成配对码（24h 一次性）
2. 另一台机器：`fux-runtime join http://<控制面>:8080 --code <配对码>`
3. 节点页批准 → 上线；调度按「项目本地性 → CLI 能力 → 负载」亲和分发
4. 节点失联 → 租约过期任务自动回收（重试≤3 后阻塞）

## 测试

```powershell
cargo test    # 12/12：状态机/RBAC/审核返工归档/mock端到端交付/中断重试/质量阀熔断/工作流 DAG
cd frontend; npm run build   # tsc + vite
```

## 关键机制

- **状态机**（`server/domain.rs`）：T1–T11 显式转移表，非法转移 409；用户/智能体/系统权限分离；乐观锁防并发冲突
- **事件溯源**：`task_events` 单调 seq，SSE `Last-Event-ID` 续传；时间线=事件投影
- **认领租约**：原子 UPDATE 认领，10s 心跳/30s 租约，过期回收
- **工作流**：`workflows` 模板（发起变量 + 智能体/逻辑节点 DAG + 节点级重试）+ `runs` 节点实例，依赖满足即并行；任务即流程实例，看板卡片显示工作流，详情可看每个节点的状态与过程
- **质量阀**（`server/watchdog.rs`）：连续 5 次相同工具调用/3 次振荡→熔断不重试；静默 240s→熔断；中断→自动重试≤2；200 轮/60min 硬上限
- **评论语义**：进行中→引导注入；已完成→返工；已阻塞→解答自动解除；审核中→评审意见
- **密钥库**：仅 MCP env/headers，AES-256-GCM；git/模型凭据不经 Fux

## 环境变量

| 变量           | 默认             | 说明                    |
| -------------- | ---------------- | ----------------------- |
| FUX_ADDR       | 0.0.0.0:8080     | 监听地址                |
| FUX_DATA_DIR   | ./data           | 数据/检出/worktree 目录 |
| FUX_JWT_SECRET | dev 默认（告警） | JWT 签名                |
| FUX_MASTER_KEY | dev 默认         | 密钥库加密              |
| RUST_LOG       | info             | 日志级别                |