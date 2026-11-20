//! 工作流（DAG 多节点编排）：模板校验、节点推进、产物交接、多项目交付。
//! 设计见 docs/WORKFLOW.md；节点状态由 runs 派生，不新增任务状态。
use crate::runner::UpstreamSpec;
use crate::server::SharedState;
use crate::server::api::{agents, projects, workspaces};
use crate::server::domain::{self, MoveOpts};
use crate::server::error::{ApiError, ApiResult};
use crate::server::events;
use crate::server::gitflow;
use crate::server::models::{Delivery, Project, Run, Task, Workflow};
use crate::server::scheduler;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageSpec {
    pub key: String,
    pub name: String,
    pub agent_id: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub prompt_append: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub when: Option<String>,
    /// 画布坐标（仅展示/编辑用）
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageState {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Skipped,
}

impl StageState {
    pub fn terminal(&self) -> bool {
        matches!(
            self,
            StageState::Succeeded
                | StageState::Failed
                | StageState::Cancelled
                | StageState::Skipped
        )
    }
    pub fn ok(&self) -> bool {
        matches!(self, StageState::Succeeded | StageState::Skipped)
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            StageState::Pending => "pending",
            StageState::Running => "running",
            StageState::Succeeded => "succeeded",
            StageState::Failed => "failed",
            StageState::Cancelled => "cancelled",
            StageState::Skipped => "skipped",
        }
    }
}

pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 40
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !key.starts_with('-')
        && !key.ends_with('-')
}

/// 保存前校验：key 唯一、依赖存在、无环。
pub fn validate(stages: &[StageSpec]) -> Result<(), String> {
    if stages.is_empty() {
        return Err("工作流至少需要一个节点".into());
    }
    let mut keys = HashSet::new();
    for s in stages {
        if !valid_key(&s.key) {
            return Err(format!(
                "节点 key 非法（仅小写字母/数字/连字符，1-40 字符）：{}",
                s.key
            ));
        }
        if !keys.insert(s.key.clone()) {
            return Err(format!("节点 key 重复：{}", s.key));
        }
        if s.agent_id.trim().is_empty() {
            return Err(format!("节点「{}」未绑定智能体", s.name));
        }
    }
    for s in stages {
        for d in &s.depends_on {
            if d == &s.key {
                return Err(format!("节点「{}」不能依赖自身", s.key));
            }
            if !keys.contains(d) {
                return Err(format!("节点「{}」依赖了不存在的节点：{}", s.key, d));
            }
        }
    }
    // DFS 三色标记检测环
    fn dfs(
        key: &str,
        map: &HashMap<String, &StageSpec>,
        color: &mut HashMap<String, u8>,
    ) -> Result<(), String> {
        match color.get(key).copied().unwrap_or(0) {
            1 => return Err(format!("工作流存在循环依赖：{key}")),
            2 => return Ok(()),
            _ => {}
        }
        color.insert(key.to_string(), 1);
        let deps = map
            .get(key)
            .map(|s| s.depends_on.clone())
            .unwrap_or_default();
        for d in deps {
            dfs(&d, map, color)?;
        }
        color.insert(key.to_string(), 2);
        Ok(())
    }
    let map: HashMap<String, &StageSpec> = stages.iter().map(|s| (s.key.clone(), s)).collect();
    let mut color = HashMap::new();
    for s in stages {
        dfs(&s.key, &map, &mut color)?;
    }
    Ok(())
}

pub fn parse_stages(wf: &Workflow) -> ApiResult<Vec<StageSpec>> {
    serde_json::from_str(&wf.stages)
        .map_err(|e| ApiError::bad_request(format!("工作流节点定义损坏：{e}")))
}

pub async fn load(state: &SharedState, id: &str) -> ApiResult<Workflow> {
    Ok(
        sqlx::query_as::<_, Workflow>("SELECT * FROM workflows WHERE id = ?")
            .bind(id)
            .fetch_one(&state.db)
            .await?,
    )
}

pub async fn stages_of(
    state: &SharedState,
    task: &Task,
) -> ApiResult<Option<(Workflow, Vec<StageSpec>)>> {
    let Some(wid) = &task.workflow_id else {
        return Ok(None);
    };
    let wf = load(state, wid).await?;
    let stages = parse_stages(&wf)?;
    Ok(Some((wf, stages)))
}

/// 取任务所有 run，按节点归并出「最新一次」的状态。
pub async fn latest_runs(state: &SharedState, task_id: &str) -> ApiResult<HashMap<String, Run>> {
    let runs =
        sqlx::query_as::<_, Run>("SELECT * FROM runs WHERE task_id = ? ORDER BY started_at ASC")
            .bind(task_id)
            .fetch_all(&state.db)
            .await?;
    let mut map = HashMap::new();
    for r in runs {
        if let Some(k) = &r.stage_key {
            map.insert(k.clone(), r);
        }
    }
    Ok(map)
}

pub fn state_of(run: Option<&Run>) -> StageState {
    match run.map(|r| r.status.as_str()) {
        None => StageState::Pending,
        Some("running") => StageState::Running,
        Some("finished") => StageState::Succeeded,
        Some("skipped") => StageState::Skipped,
        Some("cancelled") => StageState::Cancelled,
        Some(_) => StageState::Failed,
    }
}

/// 依赖已满足且自身尚未开始的节点（按模板顺序）。
pub fn ready_candidates(stages: &[StageSpec], latest: &HashMap<String, Run>) -> Vec<StageSpec> {
    let mut out = vec![];
    for s in stages {
        if latest.contains_key(&s.key) {
            continue;
        }
        if s.depends_on.iter().all(|d| state_of(latest.get(d)).ok()) {
            out.push(s.clone());
        }
    }
    out
}

/// 图的汇点（无下游节点）。
pub fn sinks(stages: &[StageSpec]) -> Vec<StageSpec> {
    let mut has_downstream = HashSet::new();
    for s in stages {
        for d in &s.depends_on {
            has_downstream.insert(d.clone());
        }
    }
    stages
        .iter()
        .filter(|s| !has_downstream.contains(&s.key))
        .cloned()
        .collect()
}

/// 某节点及其全部下游。
pub fn descendants(stages: &[StageSpec], from: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    let mut stack = vec![from.to_string()];
    while let Some(k) = stack.pop() {
        if !set.insert(k.clone()) {
            continue;
        }
        for s in stages {
            if s.depends_on.iter().any(|d| d == &k) {
                stack.push(s.key.clone());
            }
        }
    }
    set
}

/// 声明式跳过条件：tag:<name> / var:<name>=<value> / path:<substr>。
async fn eval_when(
    state: &SharedState,
    task: &Task,
    stage: &StageSpec,
    latest: &HashMap<String, Run>,
) -> bool {
    let Some(expr) = stage.when.as_deref() else {
        return true;
    };
    let expr = expr.trim();
    if let Some(tag) = expr.strip_prefix("tag:") {
        let tags: Vec<String> = serde_json::from_str(&task.tags).unwrap_or_default();
        return tags.iter().any(|t| t == tag.trim());
    }
    if let Some(kv) = expr.strip_prefix("var:") {
        if let Some((k, v)) = kv.split_once('=') {
            return task.content.lines().any(|line| {
                let line = line.trim();
                line == format!("{}: {}", k.trim(), v.trim())
                    || line == format!("{}={}", k.trim(), v.trim())
            });
        }
        return false;
    }
    if let Some(sub) = expr.strip_prefix("path:") {
        let events = sqlx::query_as::<_, crate::server::models::TaskEvent>(
            "SELECT * FROM task_events WHERE task_id = ? AND kind = 'workflow.stage_completed' ORDER BY seq",
        )
        .bind(&task.id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
        for e in events {
            let v: serde_json::Value = serde_json::from_str(&e.payload).unwrap_or(json!({}));
            let key = v.get("stageKey").and_then(|x| x.as_str()).unwrap_or("");
            if !stage.depends_on.iter().any(|d| d == key) {
                continue;
            }
            if let Some(files) = v.get("files").and_then(|f| f.as_array()) {
                if files
                    .iter()
                    .any(|f| f.as_str().unwrap_or("").contains(sub.trim()))
                {
                    return true;
                }
            }
        }
        return false;
    }
    let _ = latest;
    false
}

/// 产出该节点的上游引用（供 runtime 检出到 .fux/upstream）。
pub async fn upstream_for(
    state: &SharedState,
    stage: &StageSpec,
    latest: &HashMap<String, Run>,
) -> Vec<UpstreamSpec> {
    let mut out = vec![];
    for dep in &stage.depends_on {
        let Some(run) = latest.get(dep) else { continue };
        if run.status != "finished" {
            continue;
        }
        let Some(pid) = &run.project_id else { continue };
        let Ok(p) = projects::load_project(state, pid).await else {
            continue;
        };
        let remote = p
            .remote_url
            .clone()
            .filter(|r| !r.is_empty())
            .or_else(|| p.local_path.clone());
        let Some(remote) = remote else { continue };
        out.push(UpstreamSpec {
            key: dep.clone(),
            name: p.name.clone(),
            remote,
            branch: run.branch.clone().unwrap_or_default(),
            sha: run.head_sha.clone().unwrap_or_default(),
        });
    }
    out
}

/// 由交付图收敛：建 deliveries + 进入审核。
async fn finalize(
    state: &SharedState,
    task: &Task,
    wf: &Workflow,
    stages: &[StageSpec],
) -> ApiResult<bool> {
    let latest = latest_runs(state, &task.id).await?;
    let all_done = stages.iter().all(|s| state_of(latest.get(&s.key)).ok());
    if !all_done {
        return Ok(false);
    }
    let mut main_branch: Option<String> = None;
    let mut has_remote = false;
    for sink in sinks(stages) {
        let Some(run) = latest.get(&sink.key) else {
            continue;
        };
        if run.status != "finished" {
            continue;
        }
        let pid = sink.project_id.clone().or_else(|| task.project_id.clone());
        let Some(pid) = pid else { continue };
        let branch = run.branch.clone();
        if main_branch.is_none() {
            main_branch = branch.clone();
        }
        if let Ok(p) = projects::load_project(state, &pid).await {
            if p.remote_url
                .as_deref()
                .map(|r| !r.is_empty())
                .unwrap_or(false)
            {
                has_remote = true;
            }
        }
        sqlx::query(
            "INSERT OR IGNORE INTO deliveries (id, task_id, project_id, node_key, branch, pr_state) VALUES (?,?,?,?,?,?)",
        )
        .bind(crate::new_id()).bind(&task.id).bind(&pid).bind(&sink.key).bind(&branch)
        .bind(if has_remote { "open" } else { "local" })
        .execute(&state.db).await?;
    }
    let pr_state = if has_remote { "open" } else { "local" };
    sqlx::query("UPDATE tasks SET pr_state = ?, branch = COALESCE(branch, ?) WHERE id = ?")
        .bind(pr_state)
        .bind(&main_branch)
        .bind(&task.id)
        .execute(&state.db)
        .await?;
    events::append(
        &state.db,
        &state.broadcast,
        &task.id,
        "workflow.completed",
        "system",
        None,
        json!({ "workflowId": wf.id, "workflowName": wf.name }),
    )
    .await?;
    domain::move_task(state, &task.id, "review", "system", MoveOpts::default()).await?;
    workspaces::notify(
        state,
        &task.workspace_id,
        "review",
        Some(&task.id),
        &format!("任务「{}」的工作流已全部完成，等待审核", task.title),
    )
    .await;
    Ok(true)
}

/// 扫描并启动所有就绪节点；全部完成时收敛到审核。
pub async fn spawn_ready(state: &SharedState, task_id: &str) -> ApiResult<()> {
    let task = domain::load_task(state, task_id).await?;
    if task.status != "in_progress" {
        return Ok(());
    }
    let Some((wf, stages)) = stages_of(state, &task).await? else {
        return Ok(());
    };

    loop {
        let latest = latest_runs(state, &task.id).await?;

        let Some(stage) = ready_candidates(&stages, &latest).into_iter().next() else {
            break;
        };
        if !eval_when(state, &task, &stage, &latest).await {
            let idx = stages.iter().position(|s| s.key == stage.key).unwrap_or(0) as i64;
            sqlx::query(
                "INSERT INTO runs (id, task_id, agent_id, node_id, stage, stage_key, status, ended_at) \
                 SELECT ?,?,?,?,?,?, 'skipped', ? \
                 WHERE NOT EXISTS (SELECT 1 FROM runs WHERE task_id = ? AND stage_key = ? AND status IN ('running','finished','skipped'))",
            )
            .bind(crate::new_id()).bind(&task.id).bind(&stage.agent_id).bind("local").bind(idx)
            .bind(&stage.key).bind(crate::now())
            .bind(&task.id).bind(&stage.key)
            .execute(&state.db).await?;
            events::append(
                &state.db,
                &state.broadcast,
                &task.id,
                "workflow.stage_skipped",
                "system",
                None,
                json!({ "stageKey": stage.key, "stageName": stage.name, "reason": stage.when }),
            )
            .await?;
            continue;
        }
        if start_stage(state, &task, &wf, &stages, &stage, &latest).await? {
            continue;
        }
        break;
    }
    finalize(state, &task, &wf, &stages).await?;
    Ok(())
}

/// 启动一个节点。返回 false 表示因智能体并发/暂停等暂时不可启动。
async fn start_stage(
    state: &SharedState,
    task: &Task,
    _wf: &Workflow,
    stages: &[StageSpec],
    stage: &StageSpec,
    latest: &HashMap<String, Run>,
) -> ApiResult<bool> {
    let agent = match agents::load_agent(state, &stage.agent_id).await {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!("工作流节点 {} 智能体不可用: {e:#}", stage.key);
            return Ok(false);
        }
    };
    if agent.paused == 1 {
        return Ok(false);
    }
    let active: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE agent_id = ? AND status = 'running'")
            .bind(&agent.id)
            .fetch_one(&state.db)
            .await
            .unwrap_or(0);
    if active >= agent.max_concurrent.max(1) {
        return Ok(false);
    }
    let project_id = stage.project_id.clone().or_else(|| task.project_id.clone());
    let project: Option<Project> = match &project_id {
        Some(pid) => projects::load_project(state, pid).await.ok(),
        None => None,
    };
    let node_id = scheduler::pick_node(state, project.as_ref(), &agent).await;
    let idx = stages.iter().position(|s| s.key == stage.key).unwrap_or(0) as i64;
    let branch = format!(
        "fux/task-{}/{}",
        &task.id[..8.min(task.id.len())],
        stage.key
    );
    let run_id = crate::new_id();
    let inserted = sqlx::query(
        "INSERT INTO runs (id, task_id, agent_id, node_id, stage, stage_key, project_id, branch, lease_expires_at, status) \
         SELECT ?,?,?,?,?,?,?,?,?, 'running' \
         WHERE NOT EXISTS (SELECT 1 FROM runs WHERE task_id = ? AND stage_key = ? AND status IN ('running','finished','skipped'))",
    )
    .bind(&run_id).bind(&task.id).bind(&agent.id).bind(&node_id).bind(idx)
    .bind(&stage.key).bind(&project_id).bind(&branch).bind(gitflow::lease_deadline_secs(120))
    .bind(&task.id).bind(&stage.key)
    .execute(&state.db).await?;
    if inserted.rows_affected() == 0 {
        return Ok(false);
    }
    events::append(&state.db, &state.broadcast, &task.id, "run.started", "system", Some(&run_id),
        json!({
            "agentId": agent.id, "agentName": agent.name, "nodeId": node_id,
            "stageKey": stage.key, "stageName": stage.name, "projectId": project_id, "branch": branch,
        })).await?;
    let upstream = upstream_for(state, stage, latest).await;
    scheduler::dispatch(state, &run_id, upstream).await;
    Ok(true)
}

/// 工作流 run 结束：记录产物 → 结束判定 → 推进/收敛。
pub async fn finish_stage(
    state: &SharedState,
    run: &Run,
    outcome: &crate::runner::RunOutcome,
    head: Option<String>,
    files: Vec<String>,
) -> ApiResult<()> {
    let task = domain::load_task(state, &run.task_id).await?;
    if task.status != "in_progress" {
        return Ok(());
    }
    let Some((wf, stages)) = stages_of(state, &task).await? else {
        return Ok(());
    };
    let Some(stage) = stages
        .iter()
        .find(|s| Some(&s.key) == run.stage_key.as_ref())
        .cloned()
    else {
        return Ok(());
    };
    // 过期 Run 不再推进：被重试/跳过覆盖、或被 rewind 重置后，旧进程的结束回调直接忽略
    let latest = latest_runs(state, &task.id).await?;
    match run.stage_key.as_deref().and_then(|k| latest.get(k)) {
        Some(current) if current.id == run.id => {}
        _ => {
            tracing::debug!("忽略过期工作流 Run {}（节点已被重置/跳过/重试）", run.id);
            return Ok(());
        }
    }
    let actor = format!("agent:{}", run.agent_id);

    match outcome.status.as_str() {
        "finished" => {
            events::append(
                &state.db,
                &state.broadcast,
                &task.id,
                "workflow.stage_completed",
                &actor,
                Some(&run.id),
                json!({
                    "stageKey": stage.key, "stageName": stage.name,
                    "projectId": run.project_id, "branch": run.branch,
                    "headSha": head, "files": files,
                    "summary": gitflow::run_summary(state, &run.id).await,
                }),
            )
            .await?;
            spawn_ready(state, &task.id).await?;
        }
        "cancelled" => {
            events::append(
                &state.db,
                &state.broadcast,
                &task.id,
                "workflow.stage_failed",
                "system",
                Some(&run.id),
                json!({ "stageKey": stage.key, "reason": "运行被取消" }),
            )
            .await?;
            cancel_active(state, &task.id, Some(&run.id)).await;
            domain::move_task(
                state,
                &task.id,
                "blocked",
                &actor,
                MoveOpts {
                    reason: Some(format!("工作流节点「{}」被取消", stage.name)),
                    ..Default::default()
                },
            )
            .await?;
        }
        _ => {
            // 突然中断：同节点自动重试 ≤ 2 次（续接会话 + 摘要由 dispatch_inner 统一处理）
            if outcome.stop_reason == "interrupted" && run.retry_count < 2 {
                events::append(&state.db, &state.broadcast, &task.id, "watchdog.interrupted", "system", Some(&run.id),
                    json!({ "error": outcome.error, "retry": run.retry_count + 1, "stageKey": stage.key })).await?;
                let new_run = crate::new_id();
                sqlx::query(
                    "INSERT INTO runs (id, task_id, agent_id, node_id, stage, stage_key, project_id, branch, lease_expires_at, retry_count, retry_of, status) VALUES (?,?,?,?,?,?,?,?,?,?,?, 'running')",
                )
                .bind(&new_run).bind(&task.id).bind(&run.agent_id).bind(&run.node_id).bind(run.stage)
                .bind(&stage.key).bind(&run.project_id).bind(&run.branch).bind(gitflow::lease_deadline_secs(120))
                .bind(run.retry_count + 1).bind(&run.id)
                .execute(&state.db).await?;
                events::append(&state.db, &state.broadcast, &task.id, "run.started", "system", Some(&new_run),
                    json!({ "agentId": run.agent_id, "nodeId": run.node_id, "stageKey": stage.key, "retry": run.retry_count + 1 })).await?;
                scheduler::dispatch(state, &new_run, vec![]).await;
            } else {
                events::append(&state.db, &state.broadcast, &task.id, "workflow.stage_failed", &actor, Some(&run.id),
                    json!({ "stageKey": stage.key, "error": outcome.error, "stopReason": outcome.stop_reason })).await?;
                cancel_active(state, &task.id, Some(&run.id)).await;
                domain::move_task(
                    state,
                    &task.id,
                    "blocked",
                    &actor,
                    MoveOpts {
                        reason: Some(format!(
                            "工作流节点「{}」运行失败: {}",
                            stage.name,
                            outcome
                                .error
                                .clone()
                                .unwrap_or_else(|| outcome.stop_reason.clone())
                        )),
                        ..Default::default()
                    },
                )
                .await?;
                workspaces::notify(
                    state,
                    &task.workspace_id,
                    "blocked",
                    Some(&task.id),
                    &format!("任务「{}」在节点「{}」失败被阻塞", task.title, stage.name),
                )
                .await;
            }
        }
    }
    let _ = wf;
    Ok(())
}

pub(crate) async fn cancel_active(state: &SharedState, task_id: &str, except: Option<&str>) {
    let running =
        sqlx::query_as::<_, Run>("SELECT * FROM runs WHERE task_id = ? AND status = 'running'")
            .bind(task_id)
            .fetch_all(&state.db)
            .await
            .unwrap_or_default();
    for r in running {
        if Some(r.id.as_str()) == except {
            continue;
        }
        scheduler::cancel_run(state, &r).await;
        let _ = sqlx::query("UPDATE runs SET status = 'cancelled', stop_reason = 'task_blocked', ended_at = ? WHERE id = ?")
            .bind(crate::now()).bind(&r.id).execute(&state.db).await;
    }
}

/// 后台兜底：推进所有 in_progress 的工作流任务。
pub async fn tick(state: &SharedState) {
    let tasks = sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks WHERE status = 'in_progress' AND workflow_id IS NOT NULL ORDER BY updated_at LIMIT 20",
    )
    .fetch_all(&state.db).await.unwrap_or_default();
    for t in tasks {
        if let Err(e) = spawn_ready(state, &t.id).await {
            tracing::warn!("workflow tick {}: {e:#}", t.id);
        }
    }
}

/// 从节点 N 返工：重置该节点及其下游。
pub async fn rewind(state: &SharedState, task: &Task, from: &str, actor: &str) -> ApiResult<()> {
    let Some((_wf, stages)) = stages_of(state, task).await? else {
        return Err(ApiError::bad_request("任务不属于工作流"));
    };
    if !stages.iter().any(|s| s.key == from) {
        return Err(ApiError::bad_request("节点不存在"));
    }
    let affected = descendants(&stages, from);
    let list: Vec<String> = affected.iter().cloned().collect();
    // 先取消仍在执行的 Run，避免旧进程回写
    cancel_active(state, &task.id, None).await;
    reset_stage_worktrees(state, &task, &stages, &affected).await;
    let marks = vec!["?"; list.len()].join(",");
    sqlx::query(&format!(
        "DELETE FROM runs WHERE task_id = ? AND stage_key IN ({marks})"
    ))
    .bind(&task.id)
    .execute(&state.db)
    .await?;
    sqlx::query(&format!(
        "DELETE FROM deliveries WHERE task_id = ? AND node_key IN ({marks})"
    ))
    .bind(&task.id)
    .execute(&state.db)
    .await?;
    events::append(
        &state.db,
        &state.broadcast,
        &task.id,
        "workflow.rewound",
        actor,
        None,
        json!({ "fromStage": from, "stages": list }),
    )
    .await?;
    if task.status == "blocked" || task.status == "review" || task.status == "done" {
        domain::move_task(
            state,
            &task.id,
            "ready",
            actor,
            MoveOpts {
                reason: Some(format!("从节点「{from}」返工")),
                ..Default::default()
            },
        )
        .await?;
    }
    Ok(())
}

/// 返工时清理节点的工作区与分支，使重跑从新的上游产物开始。
async fn reset_stage_worktrees(
    state: &SharedState,
    task: &Task,
    stages: &[StageSpec],
    keys: &HashSet<String>,
) {
    for stage in stages.iter().filter(|s| keys.contains(&s.key)) {
        let pid = stage.project_id.clone().or_else(|| task.project_id.clone());
        let Some(pid) = pid else { continue };
        let Ok(p) = projects::load_project(state, &pid).await else {
            continue;
        };
        let repo = if p.kind == "git" {
            crate::git::checkout_dir(&state.data_dir, &p.id)
        } else {
            std::path::PathBuf::from(p.local_path.clone().unwrap_or_default())
        };
        let wt = crate::git::stage_worktree_dir(&state.data_dir, &task.id, &stage.key);
        crate::git::remove_worktree(&repo, &wt).await;
        let branch = format!(
            "fux/task-{}/{}",
            &task.id[..8.min(task.id.len())],
            stage.key
        );
        let _ = crate::git::git(&repo, &["branch", "-D", &branch]).await;
    }
}

/// 人工跳过节点（仅运行中的实例）。
pub async fn skip_stage(state: &SharedState, task: &Task, key: &str, actor: &str) -> ApiResult<()> {
    let Some((_wf, stages)) = stages_of(state, task).await? else {
        return Err(ApiError::bad_request("任务不属于工作流"));
    };
    let Some(stage) = stages.iter().find(|s| s.key == key) else {
        return Err(ApiError::bad_request("节点不存在"));
    };
    if task.status != "in_progress" {
        return Err(ApiError::conflict("仅运行中的工作流可以跳过节点"));
    }
    let latest = latest_runs(state, &task.id).await?;
    if state_of(latest.get(key)) == StageState::Running {
        if let Some(r) = latest.get(key) {
            scheduler::cancel_run(state, r).await;
        }
    }
    let idx = stages.iter().position(|s| s.key == stage.key).unwrap_or(0) as i64;
    sqlx::query(
        "INSERT INTO runs (id, task_id, agent_id, node_id, stage, stage_key, status, ended_at) VALUES (?,?,?,?,?,?, 'skipped', ?)",
    )
    .bind(crate::new_id()).bind(&task.id).bind(&stage.agent_id).bind("local").bind(idx)
    .bind(&stage.key).bind(crate::now())
    .execute(&state.db).await?;
    events::append(
        &state.db,
        &state.broadcast,
        &task.id,
        "workflow.stage_skipped",
        actor,
        None,
        json!({ "stageKey": stage.key, "stageName": stage.name, "manual": true }),
    )
    .await?;
    spawn_ready(state, &task.id).await?;
    Ok(())
}

/// 驳回/已完成返工：重置汇点节点（及其产物），让重跑覆盖最终交付。
pub async fn reset_sinks_for_rework(
    state: &crate::server::AppState,
    task_id: &str,
) -> ApiResult<()> {
    let task = domain::load_task(state, task_id).await?;
    let Some(wid) = &task.workflow_id else {
        return Ok(());
    };
    let wf = sqlx::query_as::<_, Workflow>("SELECT * FROM workflows WHERE id = ?")
        .bind(wid)
        .fetch_one(&state.db)
        .await?;
    let stages = parse_stages(&wf)?;
    let keys: Vec<String> = sinks(&stages).into_iter().map(|s| s.key).collect();
    if keys.is_empty() {
        return Ok(());
    }
    let marks = vec!["?"; keys.len()].join(",");
    let sql = format!("DELETE FROM runs WHERE task_id = ? AND stage_key IN ({marks})");
    let mut q = sqlx::query(&sql).bind(task_id);
    for k in &keys {
        q = q.bind(k);
    }
    q.execute(&state.db).await?;
    let sql = format!("DELETE FROM deliveries WHERE task_id = ? AND node_key IN ({marks})");
    let mut q = sqlx::query(&sql).bind(task_id);
    for k in &keys {
        q = q.bind(k);
    }
    q.execute(&state.db).await?;
    events::append(
        &state.db,
        &state.broadcast,
        task_id,
        "workflow.rework_reset",
        "system",
        None,
        json!({ "stages": keys }),
    )
    .await?;
    Ok(())
}

/// 解除阻塞：清掉失败/取消的节点 Run，使其可以重新派发。
pub async fn reset_failed_runs(state: &crate::server::AppState, task_id: &str) -> ApiResult<()> {
    let res =
        sqlx::query("DELETE FROM runs WHERE task_id = ? AND status IN ('failed','cancelled')")
            .bind(task_id)
            .execute(&state.db)
            .await?;
    if res.rows_affected() > 0 {
        events::append(
            &state.db,
            &state.broadcast,
            task_id,
            "workflow.retry_reset",
            "system",
            None,
            json!({ "runs": res.rows_affected() }),
        )
        .await?;
    }
    Ok(())
}

/// 全部交付是否已合并（节点推进与审批共用）。
pub async fn all_deliveries_merged(state: &SharedState, task_id: &str) -> ApiResult<bool> {
    let rows = sqlx::query_as::<_, Delivery>("SELECT * FROM deliveries WHERE task_id = ?")
        .bind(task_id)
        .fetch_all(&state.db)
        .await?;
    if rows.is_empty() {
        return Ok(false);
    }
    Ok(rows.iter().all(|d| d.pr_state.as_deref() == Some("merged")))
}

/// 逐条检测交付合并（每项目独立 fetch + merge-base）。
pub async fn check_deliveries(state: &SharedState, task: &Task) -> anyhow::Result<bool> {
    let rows = sqlx::query_as::<_, Delivery>("SELECT * FROM deliveries WHERE task_id = ?")
        .bind(&task.id)
        .fetch_all(&state.db)
        .await?;
    if rows.is_empty() {
        return Ok(false);
    }
    let mut all = true;
    for d in rows {
        if d.pr_state.as_deref() == Some("merged") {
            continue;
        }
        let Ok(p) = projects::load_project(state, &d.project_id).await else {
            all = false;
            continue;
        };
        let Some(remote) = p.remote_url.clone().filter(|r| !r.is_empty()) else {
            all = false;
            continue;
        };
        let Some(branch) = d.branch.clone() else {
            all = false;
            continue;
        };
        let mirror = state.data_dir.join("mirrors").join(&p.id);
        crate::git::ensure_clone(&remote, &mirror).await?;
        if crate::git::is_merged(&mirror, &branch, &p.default_branch)
            .await
            .unwrap_or(false)
        {
            let head = crate::git::git(&mirror, &["rev-parse", &branch]).await.ok();
            sqlx::query("UPDATE deliveries SET pr_state = 'merged', merge_commit = ? WHERE id = ?")
                .bind(&head)
                .bind(&d.id)
                .execute(&state.db)
                .await?;
            events::append(&state.db, &state.broadcast, &task.id, "pr.merged_detected", "system", None,
                json!({ "deliveryId": d.id, "projectId": d.project_id, "branch": d.branch, "mergeCommit": head })).await?;
        } else {
            all = false;
        }
    }
    if all {
        sqlx::query("UPDATE tasks SET pr_state = 'merged' WHERE id = ?")
            .bind(&task.id)
            .execute(&state.db)
            .await?;
        workspaces::notify(
            state,
            &task.workspace_id,
            "merged",
            Some(&task.id),
            &format!("任务「{}」的全部交付已合并，可批准", task.title),
        )
        .await;
    }
    Ok(all)
}

/// 交付所属项目的远端是否全部具备（用于节点选择校验，当前仅日志）。
pub fn project_has_remote(p: &Project) -> bool {
    p.remote_url
        .as_deref()
        .map(|r| !r.is_empty())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(key: &str, deps: &[&str]) -> StageSpec {
        StageSpec {
            key: key.into(),
            name: key.into(),
            agent_id: "a".into(),
            project_id: None,
            prompt_append: None,
            depends_on: deps.iter().map(|d| d.to_string()).collect(),
            when: None,
            x: None,
            y: None,
        }
    }

    #[test]
    fn validate_dag() {
        assert!(validate(&[s("a", &[]), s("b", &["a"])]).is_ok());
        assert!(validate(&[]).is_err());
        assert!(validate(&[s("a", &["b"]), s("b", &["a"])]).is_err());
        assert!(validate(&[s("a", &[]), s("a", &[])]).is_err());
        assert!(validate(&[s("a", &["missing"])]).is_err());
        assert!(validate(&[s("a", &["a"])]).is_err());
    }

    #[test]
    fn readiness_and_sinks() {
        let stages = vec![
            s("design", &[]),
            s("fe", &["design"]),
            s("be", &["design"]),
            s("qa", &["fe", "be"]),
        ];
        fn row(k: &str, st: &str) -> Run {
            Run {
                id: format!("r-{k}"),
                task_id: "t".into(),
                agent_id: "a".into(),
                node_id: "local".into(),
                stage: 0,
                stage_key: Some(k.into()),
                project_id: None,
                branch: None,
                head_sha: None,
                lease_expires_at: None,
                retry_count: 0,
                retry_of: None,
                acp_session_id: None,
                status: st.into(),
                stop_reason: None,
                turn_count: 0,
                last_event_at: None,
                usage_tokens: 0,
                started_at: "1".into(),
                ended_at: None,
            }
        }
        let mut latest = HashMap::new();
        latest.insert("design".to_string(), row("design", "finished"));
        let ready = ready_candidates(&stages, &latest);
        assert_eq!(
            ready.iter().map(|s| s.key.as_str()).collect::<Vec<_>>(),
            vec!["fe", "be"]
        );
        latest.insert("fe".to_string(), row("fe", "finished"));
        assert_eq!(ready_candidates(&stages, &latest).len(), 1);
        assert_eq!(
            sinks(&stages)
                .iter()
                .map(|s| s.key.as_str())
                .collect::<Vec<_>>(),
            vec!["qa"]
        );
        assert_eq!(descendants(&stages, "design").len(), 4);
    }
}
