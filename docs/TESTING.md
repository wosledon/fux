# 测试指南

本文只写「怎么跑、怎么加」；功能需求见 [PRD.md](PRD.md)，命令速查见 [AGENTS.md](../AGENTS.md)。

## 现有测试（共 12 个：6 单元 + 6 集成）

| 位置                     | 名称                                    | 覆盖                                                                                |
| ------------------------ | --------------------------------------- | ----------------------------------------------------------------------------------- |
| `tests/api.rs`           | `auth_workspace_and_rbac`               | 建卡 / 非法转移 409 / 用户认领 403 / 邀请码一次性 / member 越权 403 / 事件 seq 递增 |
| `tests/api.rs`           | `review_rework_archive_loop`            | 未合并批准 409 → `manualConfirm` → 评论返工 → 置 merged 后批准 → 归档               |
| `tests/api.rs`           | `e2e_mock_agent_delivery`               | 真实拉起 mock agent：认领 → 时间线事件 → git 提交 → 审核中                          |
| `tests/api.rs`           | `e2e_interrupt_retry_then_block`        | 中断自动重试 2 次后阻塞                                                             |
| `tests/api.rs`           | `watchdog_loop_trip`                    | 5 条相同工具调用触发质量阀熔断                                                      |
| `tests/api.rs`           | `workflow_dag_run_and_rewind`           | 工作流 DAG 校验、两节点自动推进、节点产物、交付生成、驳回重跑、从节点返工           |
| `src/server/domain.rs`   | `transition_table` / `user_permissions` | T1–T11 合法性与用户子集                                                             |
| `src/server/workflow.rs` | `validate_dag` / `readiness_and_sinks`  | DAG 校验、就绪判定、汇点与下游集                                                    |
| `src/git.rs`             | 单元测试                                | PR/MR 链接推导等纯函数                                                              |
| `src/server/secrets.rs`  | `roundtrip`                             | AES-256-GCM 加解密                                                                  |

```powershell
cargo test                                                     # 全部
cargo test --test api                                          # 只跑集成
cargo test --test api e2e_mock_agent_delivery -- --nocapture    # 单个 + 打印输出
cargo test --lib transition_table                              # 单个单测
cd frontend; npm run build                                     # 前端唯一门禁（无测试框架）
```

## 集成测试约定（`tests/api.rs`）

- `setup()` 用 `tempfile::tempdir()` + `db::init()` 建独立库，手搓 `AppState`，**不启动** `server::run()` 与 HTTP 监听。
- `app!` 宏把 `api::routes` 挂到 `/api/v1` 并返回 `test::init_service`；请求统一走 `call(app, TestRequest) -> (u16, Value)`。
- 辅助：`register(name)` 注册并取 token、`auth(req, token)` 加 Bearer、`create_ws(token)` 建工作区并断言默认 7 条泳道。
- 断言错误码一律比对 `u16`，断言业务事实读事件流（`task_events`）而不是只看 `tasks` 行，避免漏掉「状态变了但没落事件」。

## e2e 前提

- `git` 必须在 PATH（测试直接 `Command::new("git")`）。
- 通过 `env!("CARGO_BIN_EXE_fux-mock-agent")` 拿到 mock 二进制路径，写入 CLI 条目，由调度器真实拉起子进程；`scheduler::spawn` 起的后台循环在测试结束后不会自动停止，断言请用「轮询 + 超时」（现有用例约 40s）。
- mock 提示词关键字：`LOOP` 循环、`FAIL` 中断、`SLEEP` 停滞、`ASK` 权限请求。

## 加新测试的检查清单

1. 需要前置数据时复用 `register` / `create_ws`，不要跨用例共享状态。
2. 涉及时间/租约的断言不要 `sleep` 猜时间，直接读库或用 `watchdog::scan_once` 这类可手动触发的入口。
3. 新增默认泳道 / 默认任务类型会破坏 `create_ws` 的「7 条泳道」断言，同步修改。
4. 前端改动没有自动化测试兜底，改动后至少跑 `npm run build` 并按 `README.md` 的 5 分钟演示路径手测一遍关键流程。