//! 调度器：自动认领（原子）+ 租约 + 派发（本地/远程节点）+ 本地运行器。
use crate::runner::{ProjectSpec, RunOutcome, RunSink, RunSpec, UpstreamSpec};
use crate::server::SharedState;
use crate::server::api::{agents, projects};
use crate::server::domain::{self, MoveOpts};
use crate::server::error::ApiResult;
use crate::server::events;
use crate::server::gitflow;
use crate::server::models::{Agent, CliEntry, Node, Project, Run, Task};
use crate::server::secrets;
use crate::server::workflow;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::mpsc;

pub fn spawn(state: SharedState) {
    {
        let state = state.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = tick(&state).await {
                    tracing::warn!("scheduler tick: {e:#}");
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        });
    }
    let state2 = state.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = lease_scan(&state2).await {
                tracing::warn!("lease scan: {e:#}");
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
    let state3 = state.clone();
    tokio::spawn(async move {
        loop {
            workflow::tick(&state3).await;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });
}

/// 认领匹配：指定指派优先，否则按 match_tags 自动匹配（仅非工作流任务）。
async fn pick_assignee(state: &SharedState, task: &Task) -> Option<String> {
    if let Some(agent_id) = &task.assignee_agent_id {
        return Some(agent_id.clone());
    }
    let candidates = sqlx::query_as::<_, Agent>(
        "SELECT * FROM agents WHERE workspace_id = ? AND paused = 0 ORDER BY name",
    )
    .bind(&task.workspace_id)
    .fetch_all(&state.db)
    .await
    .ok()?;
    let tags: Vec<String> = serde_json::from_str(&task.tags).unwrap_or_default();
    for a in candidates {
        let match_tags: Vec<String> = serde_json::from_str(&a.match_tags).unwrap_or_default();
        if !match_tags.is_empty() && !match_tags.iter().any(|t| tags.contains(t)) {
            continue;
        }
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM runs WHERE agent_id = ? AND status = 'running'",
        )
        .bind(&a.id)
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);
        if active >= a.max_concurrent {
            continue;
        }
        return Some(a.id);
    }
    None
}

/// 节点亲和：本地有所需 CLI 优先本地；否则选在线且具备 CLI 的远程节点。
pub(crate) async fn pick_node(
    state: &SharedState,
    project: Option<&Project>,
    agent: &Agent,
) -> String {
    let cli = sqlx::query_as::<_, CliEntry>("SELECT * FROM cli_entries WHERE id = ?")
        .bind(&agent.cli_entry_id)
        .fetch_one(&state.db)
        .await
        .ok();
    let command = cli.map(|c| c.command).unwrap_or_default();
    let has_cli = |caps: &str| -> bool {
        serde_json::from_str::<Value>(caps)
            .ok()
            .and_then(|v| v.get("clis").and_then(|c| c.as_array()).cloned())
            .map(|arr| arr.iter().any(|c| c.as_str() == Some(command.as_str())))
            .unwrap_or(false)
    };
    if let Some(p) = project {
        if p.kind == "local" {
            return "local".into();
        }
    }
    let local = sqlx::query_as::<_, Node>("SELECT * FROM nodes WHERE id = 'local'")
        .fetch_one(&state.db)
        .await
        .ok();
    if let Some(l) = &local {
        if has_cli(&l.capabilities) {
            return "local".into();
        }
    }
    let remotes = sqlx::query_as::<_, Node>("SELECT * FROM nodes WHERE approved = 1 AND conn_status = 'online' AND id != 'local' ORDER BY last_heartbeat DESC")
        .fetch_all(&state.db).await.unwrap_or_default();
    for n in remotes {
        if has_cli(&n.capabilities) {
            return n.id;
        }
    }
    "local".into()
}

async fn tick(state: &SharedState) -> ApiResult<()> {
    let ready = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE status = 'ready' ORDER BY priority DESC, created_at ASC LIMIT 10")
        .fetch_all(&state.db).await?;
    for task in ready {
        // 工作流：认领后交给 DAG 引擎派发入口节点（支持并行多个 Run）
        if task.workflow_id.is_some() {
            let res = sqlx::query(
                "UPDATE tasks SET status = 'in_progress', lane_id = COALESCE((SELECT id FROM lanes WHERE board_id = tasks.board_id AND kind = 'work' ORDER BY position LIMIT 1), lane_id), lease_run_id = NULL, lease_expires_at = NULL, version = version + 1, updated_at = ? WHERE id = ? AND status = 'ready'",
            ).bind(crate::now()).bind(&task.id).execute(&state.db).await?;
            if res.rows_affected() == 0 {
                continue;
            }
            let updated = domain::load_task(state, &task.id).await?;
            events::append(
                &state.db,
                &state.broadcast,
                &task.id,
                "task.moved",
                "system",
                None,
                json!({ "from": "ready", "to": "in_progress", "task": updated }),
            )
            .await?;
            if let Err(e) = workflow::spawn_ready(state, &task.id).await {
                tracing::error!("workflow spawn {}: {e:#}", task.id);
            }
            continue;
        }

        let Some(agent_id) = pick_assignee(state, &task).await else {
            continue;
        };
        let agent = match agents::load_agent(state, &agent_id).await {
            Ok(a) => a,
            Err(_) => continue,
        };
        if agent.paused == 1 {
            continue;
        }
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM runs WHERE agent_id = ? AND status = 'running'",
        )
        .bind(&agent.id)
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);
        if active >= agent.max_concurrent {
            continue;
        }
        let project = match &task.project_id {
            Some(pid) => projects::load_project(state, pid).await.ok(),
            None => None,
        };
        let node_id = pick_node(state, project.as_ref(), &agent).await;
        let run_id = crate::new_id();
        let branch = task
            .branch
            .clone()
            .unwrap_or_else(|| format!("fux/task-{}", &task.id[..8.min(task.id.len())]));
        // 原子认领：多调度器/多节点下不重复
        let res = sqlx::query(
            "UPDATE tasks SET status = 'in_progress', lane_id = COALESCE((SELECT id FROM lanes WHERE board_id = tasks.board_id AND kind = 'work' ORDER BY position LIMIT 1), lane_id), lease_run_id = ?, lease_expires_at = ?, version = version + 1, updated_at = ? WHERE id = ? AND status = 'ready'",
        )
        .bind(&run_id).bind(gitflow::lease_deadline()).bind(crate::now()).bind(&task.id)
        .execute(&state.db).await?;
        if res.rows_affected() == 0 {
            continue;
        }
        sqlx::query("INSERT INTO runs (id, task_id, agent_id, node_id, stage, branch, lease_expires_at) VALUES (?,?,?,?,?,?,?)")
            .bind(&run_id).bind(&task.id).bind(&agent.id).bind(&node_id).bind(0i64).bind(&branch)
            .bind(gitflow::lease_deadline())
            .execute(&state.db).await?;
        let updated = domain::load_task(state, &task.id).await?;
        events::append(
            &state.db,
            &state.broadcast,
            &task.id,
            "task.moved",
            "system",
            Some(&run_id),
            json!({ "from": "ready", "to": "in_progress", "task": updated }),
        )
        .await?;
        events::append(
            &state.db,
            &state.broadcast,
            &task.id,
            "run.started",
            "system",
            Some(&run_id),
            json!({ "agentId": agent.id, "agentName": agent.name, "nodeId": node_id }),
        )
        .await?;
        dispatch(state, &run_id, vec![]).await;
    }
    Ok(())
}

/// 租约过期回收：回待办重试，超限阻塞。
async fn lease_scan(state: &SharedState) -> ApiResult<()> {
    let now = crate::now();
    // 工作流：Run 级租约（同一任务可并行多个 Run）
    let expired_runs = sqlx::query_as::<_, Run>(
        "SELECT * FROM runs WHERE status = 'running' AND stage_key IS NOT NULL AND lease_expires_at IS NOT NULL AND lease_expires_at < ?",
    ).bind(&now).fetch_all(&state.db).await?;
    for run in expired_runs {
        sqlx::query("UPDATE runs SET status = 'failed', stop_reason = 'lease_expired', ended_at = ? WHERE id = ? AND status = 'running'")
            .bind(&now).bind(&run.id).execute(&state.db).await?;
        cancel_run(state, &run).await;
        let outcome = RunOutcome {
            status: "failed".into(),
            stop_reason: "lease_expired".into(),
            turn_count: 0,
            session_id: String::new(),
            error: Some("节点失联或进程崩溃，租约过期".into()),
        };
        gitflow::finish_run(state, &run.id, outcome, None).await;
    }
    // 非工作流：任务级租约
    let expired = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE status = 'in_progress' AND workflow_id IS NULL AND lease_expires_at IS NOT NULL AND lease_expires_at < ?")
        .bind(&now).fetch_all(&state.db).await?;
    for task in expired {
        if let Some(run_id) = &task.lease_run_id {
            sqlx::query("UPDATE runs SET status = 'failed', stop_reason = 'lease_expired', ended_at = ? WHERE id = ? AND status = 'running'")
                .bind(&now).bind(run_id).execute(&state.db).await?;
            cancel_run(
                state,
                &sqlx::query_as::<_, Run>("SELECT * FROM runs WHERE id = ?")
                    .bind(run_id)
                    .fetch_one(&state.db)
                    .await?,
            )
            .await;
        }
        let retry = task.retry_count + 1;
        if retry > 3 {
            sqlx::query("UPDATE tasks SET retry_count = ? WHERE id = ?")
                .bind(retry)
                .bind(&task.id)
                .execute(&state.db)
                .await?;
            domain::move_task(
                state,
                &task.id,
                "blocked",
                "system",
                MoveOpts {
                    reason: Some("租约丢失且重试次数超限（节点失联/进程崩溃）".into()),
                    ..Default::default()
                },
            )
            .await?;
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
    upstream: Vec<UpstreamSpec>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        if let Err(e) = dispatch_inner(state, run_id, upstream).await {
            tracing::error!("dispatch {run_id}: {e:#}");
            let outcome = RunOutcome {
                status: "failed".into(),
                stop_reason: "interrupted".into(),
                turn_count: 0,
                session_id: String::new(),
                error: Some(format!("{e:#}")),
            };
            gitflow::finish_run(state, run_id, outcome, None).await;
        }
    })
}

async fn dispatch_inner(
    state: &SharedState,
    run_id: &str,
    upstream: Vec<UpstreamSpec>,
) -> ApiResult<()> {
    let run = sqlx::query_as::<_, Run>("SELECT * FROM runs WHERE id = ?")
        .bind(run_id)
        .fetch_one(&state.db)
        .await?;
    // 重试的 run 续接上一个 run 的 ACP 会话与工作摘要：
    // CLI 不支持 loadSession 时会自动回退新建会话，摘要则拼进 prompt。
    let (resume_session_id, context_summary) = match &run.retry_of {
        Some(prev) => {
            let sid: Option<String> =
                sqlx::query_scalar("SELECT acp_session_id FROM runs WHERE id = ?")
                    .bind(prev)
                    .fetch_optional(&state.db)
                    .await
                    .ok()
                    .flatten()
                    .filter(|s: &String| !s.is_empty());
            let summary = gitflow::run_summary(state, prev).await;
            (sid, summary)
        }
        None => (None, String::new()),
    };
    let task = domain::load_task(state, &run.task_id).await?;
    let agent = agents::load_agent(state, &run.agent_id).await?;
    let cli = sqlx::query_as::<_, CliEntry>("SELECT * FROM cli_entries WHERE id = ?")
        .bind(&agent.cli_entry_id)
        .fetch_one(&state.db)
        .await?;
    let project_pid = run.project_id.clone().or_else(|| task.project_id.clone());
    let project = match &project_pid {
        Some(pid) => projects::load_project(state, pid).await.ok(),
        None => None,
    };

    let skills = agents::skills_for_agent(state, &agent.id).await?;

    // MCP 授权透传（按 CLI 能力；密钥解密后注入 env/headers）
    let mcp_servers = agents::mcp_configs(state, &agent.id).await?;

    // 初始引导评论（认领前积累的）
    let pending = domain::pending_guidance(state, &task.id).await;
    let guidance: Vec<String> = pending.iter().map(|(_, t)| t.clone()).collect();
    if !pending.is_empty() {
        events::append(
            &state.db,
            &state.broadcast,
            &task.id,
            "comment.injected",
            "system",
            Some(run_id),
            json!({ "seqs": pending.iter().map(|(s, _)| s).collect::<Vec<_>>() }),
        )
        .await?;
    }

    let env: Vec<(String, String)> = secrets::cli_env_plain(&cli.env_template)
        .as_object()
        .map(|m| {
            m.into_iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default();
    let stage_spec = match &run.stage_key {
        Some(sk) => workflow::stages_of(state, &task)
            .await?
            .and_then(|(_, stages)| stages.into_iter().find(|s| &s.key == sk)),
        None => None,
    };
    let mut content = task.content.clone();
    let mut stage_key = String::new();
    let mut stage_name = String::new();
    let mut base_branch: Option<String> = None;
    if let Some(st) = &stage_spec {
        let inputs = workflow::inputs_of(&task);
        stage_key = st.key.clone();
        stage_name = workflow::render_template(&st.name, &inputs);
        content.push_str(&format!("\n\n## 当前工作流节点：{}\n", stage_name));
        if let Some(extra) = &st.prompt_append {
            let extra = workflow::render_template(extra, &inputs);
            if !extra.trim().is_empty() {
                content.push_str(extra.trim());
                content.push('\n');
            }
        }
        if !upstream.is_empty() {
            content.push_str("\n## 上游产物（已检出到 .fux/upstream/<节点 key>，只读参考）\n");
            for u in &upstream {
                let sha = if u.sha.len() > 8 { &u.sha[..8] } else { &u.sha };
                content.push_str(&format!(
                    "- {}（{}）分支 {} @ {}\n  路径: .fux/upstream/{}\n",
                    u.key, u.name, u.branch, sha, u.key
                ));
            }
        }
        // 同项目上游分支作为本节点分支的起点
        for dep in &st.depends_on {
            let prev = sqlx::query_as::<_, Run>(
                "SELECT * FROM runs WHERE task_id = ? AND stage_key = ? AND status = 'finished' ORDER BY started_at DESC LIMIT 1",
            ).bind(&task.id).bind(dep).fetch_optional(&state.db).await?;
            if let Some(prev) = prev {
                if prev.project_id == run.project_id {
                    base_branch = prev.branch.clone();
                    break;
                }
            }
        }
    }
    let branch = run
        .branch
        .clone()
        .or_else(|| task.branch.clone())
        .unwrap_or_else(|| format!("fux/task-{}", &task.id[..8.min(task.id.len())]));
    let spec = RunSpec {
        run_id: run_id.to_string(),
        task_id: task.id.clone(),
        title: task.title.clone(),
        content,
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
        context_summary,
        resume_session_id,
        project: project.map(|p| ProjectSpec {
            project_id: p.id,
            kind: p.kind,
            remote_url: p.remote_url,
            local_path: p.local_path,
            default_branch: p.default_branch,
        }),
        branch,
        base_branch,
        stage_key,
        stage_name,
        upstream,
    };

    if run.node_id == "local" {
        let state = state.clone();
        let run_id = run_id.to_string();
        tokio::spawn(async move { run_local(state, run_id, spec).await });
    } else {
        let sent = state
            .nodes
            .send(&run.node_id, crate::proto::ServerMsg::Dispatch { spec })
            .await;
        if !sent {
            return Err(crate::server::error::ApiError::conflict("节点通道不可用"));
        }
    }
    Ok(())
}

pub async fn cancel_run(state: &SharedState, run: &Run) {
    if run.node_id == "local" {
        let map = state.local_runs.lock().await;
        if let Some(tx) = map.get(&run.id) {
            let _ = tx.send(());
        }
    } else {
        let _ = state
            .nodes
            .send(
                &run.node_id,
                crate::proto::ServerMsg::Cancel {
                    run_id: run.id.clone(),
                },
            )
            .await;
    }
}

async fn run_local(state: SharedState, run_id: String, mut spec: RunSpec) {
    let run = match sqlx::query_as::<_, Run>("SELECT * FROM runs WHERE id = ?")
        .bind(&run_id)
        .fetch_one(&state.db)
        .await
    {
        Ok(r) => r,
        Err(_) => return,
    };
    let task = match domain::load_task(&state, &run.task_id).await {
        Ok(t) => t,
        Err(_) => return,
    };
    let project = match run.project_id.clone().or_else(|| task.project_id.clone()) {
        Some(pid) => projects::load_project(&state, &pid).await.ok(),
        None => None,
    };
    let workdir = if spec.stage_key.is_empty() {
        crate::git::worktree_dir(&state.data_dir, &task.id)
    } else {
        crate::git::stage_worktree_dir(&state.data_dir, &task.id, &spec.stage_key)
    };
    let branch = run.branch.clone().unwrap_or_else(|| spec.branch.clone());
    let base = spec.base_branch.clone();
    if let Err(e) =
        gitflow::prepare_workspace(&state, project.as_ref(), &workdir, &branch, base.as_deref())
            .await
    {
        let outcome = RunOutcome {
            status: "failed".into(),
            stop_reason: "interrupted".into(),
            turn_count: 0,
            session_id: String::new(),
            error: Some(format!("准备工作区失败: {e:#}")),
        };
        gitflow::finish_run(&state, &run_id, outcome, None).await;
        return;
    }
    crate::git::materialize_upstreams(&workdir, &spec.upstream)
        .await
        .ok();
    spec.workdir = workdir;
    let (cancel_tx, cancel_rx) = mpsc::unbounded_channel();
    state
        .local_runs
        .lock()
        .await
        .insert(run_id.clone(), cancel_tx);
    let sink = Arc::new(LocalSink {
        state: state.clone(),
        run_id: run_id.clone(),
        task_id: task.id.clone(),
        agent_id: run.agent_id.clone(),
        policy: spec.policy.clone(),
    });
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
        let _ = events::append(
            &self.state.db,
            &self.state.broadcast,
            &self.task_id,
            kind,
            &format!("agent:{}", self.agent_id),
            Some(&self.run_id),
            payload,
        )
        .await;
        let _ = sqlx::query("UPDATE runs SET last_event_at = ?, lease_expires_at = ? WHERE id = ?")
            .bind(crate::now())
            .bind(gitflow::lease_deadline())
            .bind(&self.run_id)
            .execute(&self.state.db)
            .await;
        let _ = sqlx::query("UPDATE tasks SET lease_expires_at = ? WHERE lease_run_id = ?")
            .bind(gitflow::lease_deadline())
            .bind(&self.run_id)
            .execute(&self.state.db)
            .await;
    }

    async fn permission(&self, request: Value) -> String {
        if self.policy == "full_auto" {
            return "allow_always".into();
        }
        let kind = request
            .pointer("/toolCall/kind")
            .and_then(|k| k.as_str())
            .unwrap_or("");
        if matches!(kind, "read" | "fetch" | "search" | "think" | "other")
            && self.policy != "full_auto"
        {
            // 只读类自动放行
            if matches!(kind, "read" | "fetch" | "search" | "think") {
                return "allow_once".into();
            }
        }
        let approval_id = crate::new_id();
        let _ =
            sqlx::query("INSERT INTO approvals (id, run_id, task_id, request) VALUES (?,?,?,?)")
                .bind(&approval_id)
                .bind(&self.run_id)
                .bind(&self.task_id)
                .bind(request.to_string())
                .execute(&self.state.db)
                .await;
        let _ = events::append(
            &self.state.db,
            &self.state.broadcast,
            &self.task_id,
            "permission.requested",
            &format!("agent:{}", self.agent_id),
            Some(&self.run_id),
            json!({ "approvalId": approval_id, "request": request }),
        )
        .await;
        // 轮询决策（120s 超时 → 阻塞）
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let status: Option<String> =
                sqlx::query_scalar("SELECT status FROM approvals WHERE id = ?")
                    .bind(&approval_id)
                    .fetch_optional(&self.state.db)
                    .await
                    .ok()
                    .flatten();
            match status.as_deref() {
                Some("approved") => return "allow_once".into(),
                Some("rejected") => return "reject".into(),
                _ => {}
            }
        }
        let _ = sqlx::query("UPDATE approvals SET status = 'timeout', decided_at = ? WHERE id = ?")
            .bind(crate::now())
            .bind(&approval_id)
            .execute(&self.state.db)
            .await;
        let _ = domain::move_task(
            &self.state,
            &self.task_id,
            "blocked",
            "system",
            MoveOpts {
                reason: Some("工具授权请求超时（120s）".into()),
                ..Default::default()
            },
        )
        .await;
        "reject".into()
    }

    async fn session_started(&self, session_id: &str) {
        // 会话一建立就落库：run 进行中/崩溃后都能看到真实 ACP 会话 id（供重试续接）
        let _ = sqlx::query("UPDATE runs SET acp_session_id = ? WHERE id = ?")
            .bind(session_id)
            .bind(&self.run_id)
            .execute(&self.state.db)
            .await;
    }

    async fn heartbeat(&self, turn_count: u32) {
        let deadline = gitflow::lease_deadline();
        let _ = sqlx::query(
            "UPDATE runs SET turn_count = ?, last_event_at = ?, lease_expires_at = ? WHERE id = ?",
        )
        .bind(turn_count as i64)
        .bind(crate::now())
        .bind(&deadline)
        .bind(&self.run_id)
        .execute(&self.state.db)
        .await;
        let _ = sqlx::query("UPDATE tasks SET lease_expires_at = ? WHERE lease_run_id = ?")
            .bind(&deadline)
            .bind(&self.run_id)
            .execute(&self.state.db)
            .await;
    }

    async fn pending_guidance(&self) -> Vec<String> {
        let pending = domain::pending_guidance(&self.state, &self.task_id).await;
        if pending.is_empty() {
            return vec![];
        }
        let _ = events::append(
            &self.state.db,
            &self.state.broadcast,
            &self.task_id,
            "comment.injected",
            "system",
            Some(&self.run_id),
            json!({ "seqs": pending.iter().map(|(s, _)| s).collect::<Vec<_>>() }),
        )
        .await;
        pending.into_iter().map(|(_, t)| t).collect()
    }
}
