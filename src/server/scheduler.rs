//! 调度器：自动认领（原子）+ 租约 + 派发（本地/远程节点）+ 本地运行器。
use crate::runner::{RunOutcome, RunSink, RunSpec, ProjectSpec};
use crate::server::api::{agents, mcp, projects};
use crate::server::domain::{self, MoveOpts};
use crate::server::error::ApiResult;
use crate::server::events;
use crate::server::gitflow;
use crate::server::models::{Agent, CliEntry, Node, Run, Skill, Task};
use crate::server::SharedState;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::mpsc;

pub fn spawn(state: SharedState) {
    {
        let state = state.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = tick(&state).await { tracing::warn!("scheduler tick: {e:#}"); }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        });
    }
    let state2 = state.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = lease_scan(&state2).await { tracing::warn!("lease scan: {e:#}"); }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
}

/// 认领匹配：指定指派优先，否则按 match_tags 自动匹配。
async fn pick_assignee(state: &SharedState, task: &Task) -> Option<(String, Option<String>, i64)> {
    if let Some(team_id) = &task.assignee_team_id {
        let members = crate::server::api::teams::team_members(state, team_id).await.ok()?;
        let first = members.first()?;
        return Some((first.agent_id.clone(), Some(team_id.clone()), 0));
    }
    if let Some(agent_id) = &task.assignee_agent_id {
        return Some((agent_id.clone(), None, 0));
    }
    let candidates = sqlx::query_as::<_, Agent>("SELECT * FROM agents WHERE workspace_id = ? AND paused = 0 ORDER BY name")
        .bind(&task.workspace_id).fetch_all(&state.db).await.ok()?;
    let tags: Vec<String> = serde_json::from_str(&task.tags).unwrap_or_default();
    for a in candidates {
        let match_tags: Vec<String> = serde_json::from_str(&a.match_tags).unwrap_or_default();
        if !match_tags.is_empty() && !match_tags.iter().any(|t| tags.contains(t)) { continue; }
        let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE agent_id = ? AND status = 'running'")
            .bind(&a.id).fetch_one(&state.db).await.unwrap_or(0);
        if active >= a.max_concurrent { continue; }
        return Some((a.id, None, 0));
    }
    None
}

/// 节点亲和：本地有所需 CLI 优先本地；否则选在线且具备 CLI 的远程节点。
async fn pick_node(state: &SharedState, task: &Task, agent: &Agent) -> String {
    let cli = sqlx::query_as::<_, CliEntry>("SELECT * FROM cli_entries WHERE id = ?").bind(&agent.cli_entry_id).fetch_one(&state.db).await.ok();
    let command = cli.map(|c| c.command).unwrap_or_default();
    let has_cli = |caps: &str| -> bool {
        serde_json::from_str::<Value>(caps).ok()
            .and_then(|v| v.get("clis").and_then(|c| c.as_array()).cloned())
            .map(|arr| arr.iter().any(|c| c.as_str() == Some(command.as_str())))
            .unwrap_or(false)
    };
    if let Some(pid) = &task.project_id {
        if let Ok(p) = projects::load_project(state, pid).await {
            if p.kind == "local" { return "local".into(); }
        }
    }
    let local = sqlx::query_as::<_, Node>("SELECT * FROM nodes WHERE id = 'local'").fetch_one(&state.db).await.ok();
    if let Some(l) = &local { if has_cli(&l.capabilities) { return "local".into(); } }
    let remotes = sqlx::query_as::<_, Node>("SELECT * FROM nodes WHERE approved = 1 AND conn_status = 'online' AND id != 'local' ORDER BY last_heartbeat DESC")
        .fetch_all(&state.db).await.unwrap_or_default();
    for n in remotes { if has_cli(&n.capabilities) { return n.id; } }
    "local".into()
}

async fn tick(state: &SharedState) -> ApiResult<()> {
    let ready = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE status = 'ready' ORDER BY priority DESC, created_at ASC LIMIT 10")
        .fetch_all(&state.db).await?;
    for task in ready {
        let Some((agent_id, team_id, stage)) = pick_assignee(state, &task).await else { continue };
        let agent = match agents::load_agent(state, &agent_id).await { Ok(a) => a, Err(_) => continue };
        if agent.paused == 1 { continue; }
        let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE agent_id = ? AND status = 'running'")
            .bind(&agent.id).fetch_one(&state.db).await.unwrap_or(0);
        if active >= agent.max_concurrent { continue; }
        let node_id = pick_node(state, &task, &agent).await;
        let run_id = crate::new_id();
        // 原子认领：多调度器/多节点下不重复
        let res = sqlx::query(
            "UPDATE tasks SET status = 'in_progress', lane_id = COALESCE((SELECT id FROM lanes WHERE board_id = tasks.board_id AND kind = 'work' ORDER BY position LIMIT 1), lane_id), lease_run_id = ?, lease_expires_at = ?, version = version + 1, updated_at = ? WHERE id = ? AND status = 'ready'",
        )
        .bind(&run_id).bind(gitflow::lease_deadline()).bind(crate::now()).bind(&task.id)
        .execute(&state.db).await?;
        if res.rows_affected() == 0 { continue; }
        sqlx::query("INSERT INTO runs (id, task_id, agent_id, team_id, node_id, stage) VALUES (?,?,?,?,?,?)")
            .bind(&run_id).bind(&task.id).bind(&agent.id).bind(&team_id).bind(&node_id).bind(stage)
            .execute(&state.db).await?;
        let updated = domain::load_task(state, &task.id).await?;
        events::append(&state.db, &state.broadcast, &task.id, "task.moved", "system", Some(&run_id),
            json!({ "from": "ready", "to": "in_progress", "task": updated })).await?;
        events::append(&state.db, &state.broadcast, &task.id, "run.started", "system", Some(&run_id),
            json!({ "agentId": agent.id, "agentName": agent.name, "nodeId": node_id })).await?;
        dispatch(state, &run_id, None).await;
    }
    Ok(())
}

/// 租约过期回收：回待办重试，超限阻塞。
async fn lease_scan(state: &SharedState) -> ApiResult<()> {
    let now = crate::now();
    let expired = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE status = 'in_progress' AND lease_expires_at IS NOT NULL AND lease_expires_at < ?")
        .bind(&now).fetch_all(&state.db).await?;
    for task in expired {
        if let Some(run_id) = &task.lease_run_id {
            sqlx::query("UPDATE runs SET status = 'failed', stop_reason = 'lease_expired', ended_at = ? WHERE id = ? AND status = 'running'")
                .bind(&now).bind(run_id).execute(&state.db).await?;
            cancel_run(state, &sqlx::query_as::<_, Run>("SELECT * FROM runs WHERE id = ?").bind(run_id).fetch_one(&state.db).await?).await;
        }
        let retry = task.retry_count + 1;
        if retry > 3 {
            sqlx::query("UPDATE tasks SET retry_count = ? WHERE id = ?").bind(retry).bind(&task.id).execute(&state.db).await?;
            domain::move_task(state, &task.id, "blocked", "system", MoveOpts {
                reason: Some("租约丢失且重试次数超限（节点失联/进程崩溃）".into()), ..Default::default()
            }).await?;
        } else {
            sqlx::query(
                "UPDATE tasks SET status = 'ready', retry_count = ?, lease_run_id = NULL, lease_expires_at = NULL, lane_id = COALESCE((SELECT id FROM lanes WHERE board_id = tasks.board_id AND kind = 'ready' LIMIT 1), lane_id), version = version + 1, updated_at = ? WHERE id = ? AND status = 'in_progress'",
            ).bind(retry).bind(&now).bind(&task.id).execute(&state.db).await?;
            let updated = domain::load_task(state, &task.id).await?;
            events::append(&state.db, &state.broadcast, &task.id, "task.moved", "system", None,
                json!({ "from": "in_progress", "to": "ready", "reason": "租约过期回收", "task": updated })).await?;
        }
    }
    Ok(())
}

/// 构建 RunSpec 并派发到节点。
/// 注意：boxed future 打断 dispatch ↔ finish_run 的递归 async 环（auto-trait 推导需要）。
pub fn dispatch<'a>(
    state: &'a SharedState,
    run_id: &'a str,
    context_summary: Option<String>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        if let Err(e) = dispatch_inner(state, run_id, context_summary).await {
            tracing::error!("dispatch {run_id}: {e:#}");
            let outcome = RunOutcome { status: "failed".into(), stop_reason: "interrupted".into(), turn_count: 0, session_id: String::new(), error: Some(format!("{e:#}")) };
            gitflow::finish_run(state, run_id, outcome, None).await;
        }
    })
}

async fn dispatch_inner(state: &SharedState, run_id: &str, context_summary: Option<String>) -> ApiResult<()> {
    let run = sqlx::query_as::<_, Run>("SELECT * FROM runs WHERE id = ?").bind(run_id).fetch_one(&state.db).await?;
    let task = domain::load_task(state, &run.task_id).await?;
    let agent = agents::load_agent(state, &run.agent_id).await?;
    let cli = sqlx::query_as::<_, CliEntry>("SELECT * FROM cli_entries WHERE id = ?").bind(&agent.cli_entry_id).fetch_one(&state.db).await?;
    let project = match &task.project_id { Some(pid) => projects::load_project(state, pid).await.ok(), None => None };

    let skills = sqlx::query_as::<_, Skill>(
        "SELECT s.* FROM skills s JOIN agent_skills a ON a.skill_id = s.id WHERE a.agent_id = ?",
    ).bind(&agent.id).fetch_all(&state.db).await?;

    // MCP 授权透传（按 CLI 能力；密钥解密后注入 env/headers）
    let grants = sqlx::query_as::<_, (String, String)>(
        "SELECT mcp_server_id, tool_whitelist FROM agent_mcp_servers WHERE agent_id = ?",
    ).bind(&agent.id).fetch_all(&state.db).await?;
    let mut mcp_servers = vec![];
    for (mid, _wl) in grants {
        if let Ok(m) = sqlx::query_as::<_, crate::server::models::McpServer>("SELECT * FROM mcp_servers WHERE id = ?").bind(&mid).fetch_one(&state.db).await {
            let env = mcp::env_plain(&m);
            let args: Vec<String> = serde_json::from_str(&m.args).unwrap_or_default();
            if m.transport == "stdio" {
                let env_arr: Vec<Value> = env.as_object().map(|o| o.iter().map(|(k, v)| json!({ "name": k, "value": v.as_str().unwrap_or("") })).collect()).unwrap_or_default();
                mcp_servers.push(json!({ "name": m.name, "command": m.command, "args": args, "env": env_arr }));
            } else {
                let headers: Vec<Value> = env.as_object().map(|o| o.iter().map(|(k, v)| json!({ "name": k, "value": v.as_str().unwrap_or("") })).collect()).unwrap_or_default();
                mcp_servers.push(json!({ "type": m.transport, "name": m.name, "url": m.url, "headers": headers }));
            }
        }
    }

    // 初始引导评论（认领前积累的）
    let pending = domain::pending_guidance(state, &task.id).await;
    let guidance: Vec<String> = pending.iter().map(|(_, t)| t.clone()).collect();
    if !pending.is_empty() {
        events::append(&state.db, &state.broadcast, &task.id, "comment.injected", "system", Some(run_id),
            json!({ "seqs": pending.iter().map(|(s, _)| s).collect::<Vec<_>>() })).await?;
    }

    let env: Vec<(String, String)> = serde_json::from_str::<serde_json::Map<String, Value>>(&cli.env_template)
        .map(|m| m.into_iter().map(|(k, v)| (k, v.as_str().unwrap_or("").to_string())).collect())
        .unwrap_or_default();
    let spec = RunSpec {
        run_id: run_id.to_string(),
        task_id: task.id.clone(),
        title: task.title.clone(),
        content: task.content.clone(),
        workdir: std::path::PathBuf::new(),
        command: cli.command.clone(),
        args: serde_json::from_str(&cli.args).unwrap_or_default(),
        env,
        cli_kind: cli.kind.clone(),
        model: agent.model.clone(),
        effort: agent.reasoning_effort.clone(),
        system_prompt: agent.system_prompt.clone(),
        skills: skills.into_iter().map(|s| (s.name, s.content)).collect(),
        mcp_servers,
        policy: agent.policy.clone(),
        guidance,
        context_summary: context_summary.unwrap_or_default(),
        project: project.map(|p| ProjectSpec {
            project_id: p.id, kind: p.kind, remote_url: p.remote_url, local_path: p.local_path, default_branch: p.default_branch,
        }),
        branch: task.branch.clone().unwrap_or_else(|| format!("fux/task-{}", &task.id[..8])),
    };

    if run.node_id == "local" {
        let state = state.clone();
        let run_id = run_id.to_string();
        tokio::spawn(async move { run_local(state, run_id, spec).await });
    } else {
        let sent = state.nodes.send(&run.node_id, crate::proto::ServerMsg::Dispatch { spec }).await;
        if !sent {
            return Err(crate::server::error::ApiError::conflict("节点通道不可用"));
        }
    }
    Ok(())
}

pub async fn cancel_run(state: &SharedState, run: &Run) {
    if run.node_id == "local" {
        let map = state.local_runs.lock().await;
        if let Some(tx) = map.get(&run.id) { let _ = tx.send(()); }
    } else {
        let _ = state.nodes.send(&run.node_id, crate::proto::ServerMsg::Cancel { run_id: run.id.clone() }).await;
    }
}

async fn run_local(state: SharedState, run_id: String, mut spec: RunSpec) {
    let run = match sqlx::query_as::<_, Run>("SELECT * FROM runs WHERE id = ?").bind(&run_id).fetch_one(&state.db).await {
        Ok(r) => r, Err(_) => return,
    };
    let task = match domain::load_task(&state, &run.task_id).await { Ok(t) => t, Err(_) => return };
    let project = match &task.project_id { Some(pid) => projects::load_project(&state, pid).await.ok(), None => None };
    let workdir = match gitflow::prepare_workspace(&state, &task, project.as_ref()).await {
        Ok(w) => w,
        Err(e) => {
            let outcome = RunOutcome { status: "failed".into(), stop_reason: "interrupted".into(), turn_count: 0, session_id: String::new(), error: Some(format!("准备工作区失败: {e:#}")) };
            gitflow::finish_run(&state, &run_id, outcome, None).await;
            return;
        }
    };
    spec.workdir = workdir;
    let (cancel_tx, cancel_rx) = mpsc::unbounded_channel();
    state.local_runs.lock().await.insert(run_id.clone(), cancel_tx);
    let sink = Arc::new(LocalSink { state: state.clone(), run_id: run_id.clone(), task_id: task.id.clone(), agent_id: run.agent_id.clone(), policy: spec.policy.clone() });
    let outcome = crate::runner::execute(spec, sink, cancel_rx).await;
    state.local_runs.lock().await.remove(&run_id);
    gitflow::finish_run(&state, &run_id, outcome, None).await;
}

/// 本地 Run 的事件/审批/心跳/引导接驳。
struct LocalSink {
    state: SharedState,
    run_id: String,
    task_id: String,
    agent_id: String,
    policy: String,
}

#[async_trait]
impl RunSink for LocalSink {
    async fn emit(&self, kind: &str, payload: Value) {
        let _ = events::append(&self.state.db, &self.state.broadcast, &self.task_id, kind,
            &format!("agent:{}", self.agent_id), Some(&self.run_id), payload).await;
        let _ = sqlx::query("UPDATE runs SET last_event_at = ? WHERE id = ?").bind(crate::now()).bind(&self.run_id).execute(&self.state.db).await;
        let _ = sqlx::query("UPDATE tasks SET lease_expires_at = ? WHERE lease_run_id = ?").bind(gitflow::lease_deadline()).bind(&self.run_id).execute(&self.state.db).await;
    }

    async fn permission(&self, request: Value) -> String {
        if self.policy == "full_auto" { return "allow_always".into(); }
        let kind = request.pointer("/toolCall/kind").and_then(|k| k.as_str()).unwrap_or("");
        if matches!(kind, "read" | "fetch" | "search" | "think" | "other") && self.policy != "full_auto" {
            // 只读类自动放行
            if matches!(kind, "read" | "fetch" | "search" | "think") { return "allow_once".into(); }
        }
        let approval_id = crate::new_id();
        let _ = sqlx::query("INSERT INTO approvals (id, run_id, task_id, request) VALUES (?,?,?,?)")
            .bind(&approval_id).bind(&self.run_id).bind(&self.task_id).bind(request.to_string())
            .execute(&self.state.db).await;
        let _ = events::append(&self.state.db, &self.state.broadcast, &self.task_id, "permission.requested",
            &format!("agent:{}", self.agent_id), Some(&self.run_id),
            json!({ "approvalId": approval_id, "request": request })).await;
        // 轮询决策（120s 超时 → 阻塞）
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let status: Option<String> = sqlx::query_scalar("SELECT status FROM approvals WHERE id = ?")
                .bind(&approval_id).fetch_optional(&self.state.db).await.ok().flatten();
            match status.as_deref() {
                Some("approved") => return "allow_once".into(),
                Some("rejected") => return "reject".into(),
                _ => {}
            }
        }
        let _ = sqlx::query("UPDATE approvals SET status = 'timeout', decided_at = ? WHERE id = ?").bind(crate::now()).bind(&approval_id).execute(&self.state.db).await;
        let _ = domain::move_task(&self.state, &self.task_id, "blocked", "system", MoveOpts {
            reason: Some("工具授权请求超时（120s）".into()), ..Default::default()
        }).await;
        "reject".into()
    }

    async fn heartbeat(&self, turn_count: u32) {
        let _ = sqlx::query("UPDATE runs SET turn_count = ?, last_event_at = ? WHERE id = ?")
            .bind(turn_count as i64).bind(crate::now()).bind(&self.run_id).execute(&self.state.db).await;
        let _ = sqlx::query("UPDATE tasks SET lease_expires_at = ? WHERE lease_run_id = ?")
            .bind(gitflow::lease_deadline()).bind(&self.run_id).execute(&self.state.db).await;
    }

    async fn pending_guidance(&self) -> Vec<String> {
        let pending = domain::pending_guidance(&self.state, &self.task_id).await;
        if pending.is_empty() { return vec![]; }
        let _ = events::append(&self.state.db, &self.state.broadcast, &self.task_id, "comment.injected", "system",
            Some(&self.run_id), json!({ "seqs": pending.iter().map(|(s, _)| s).collect::<Vec<_>>() })).await;
        pending.into_iter().map(|(_, t)| t).collect()
    }
}