//! git 交付流：任务隔离 worktree、完成→push→PR/MR（git 原生）、合并检测、工作流交付。
use crate::git;
use crate::runner::RunOutcome;
use crate::server::SharedState;
use crate::server::api::{projects, workspaces};
use crate::server::domain::{self, MoveOpts};
use crate::server::error::ApiResult;
use crate::server::events;
use crate::server::models::{Project, Run, Task};
use crate::server::workflow;
use serde_json::json;
use std::path::PathBuf;

/// 为节点准备隔离工作区（本地执行）。
pub async fn prepare_workspace(
    state: &SharedState,
    project: Option<&Project>,
    workdir: &std::path::Path,
    branch: &str,
    base: Option<&str>,
) -> ApiResult<PathBuf> {
    let base_branch = base
        .or_else(|| project.map(|p| p.default_branch.as_str()))
        .unwrap_or("main");
    match project {
        Some(p) if p.kind == "git" => {
            let repo = git::checkout_dir(&state.data_dir, &p.id);
            git::ensure_clone(p.remote_url.as_deref().unwrap_or(""), &repo).await?;
            git::prepare_worktree(&repo, workdir, branch, base_branch).await?;
        }
        Some(p) => {
            let lp = std::path::Path::new(p.local_path.as_deref().unwrap_or(""));
            if git::is_repo(lp).await {
                git::prepare_worktree(lp, workdir, branch, base_branch).await?;
            } else if !workdir.exists() {
                git::copy_dir_recursive(lp, workdir)?;
                git::init_repo(workdir).await?;
                git::commit_all(workdir, "fux: baseline").await.ok();
            }
        }
        None => {
            tokio::fs::create_dir_all(workdir).await?;
            if !git::is_repo(workdir).await {
                git::init_repo(workdir).await?;
            }
        }
    }
    Ok(workdir.to_path_buf())
}

pub struct Completion {
    pub pushed: bool,
    pub head: Option<String>,
    pub pr_url: Option<String>,
    pub pr_state: String,
}

/// 本地完成：commit + push + 推导 PR/MR 创建链接。
pub async fn complete_local(
    _state: &SharedState,
    task: &Task,
    project: Option<&Project>,
    workdir: &std::path::Path,
    branch: &str,
) -> anyhow::Result<Completion> {
    let head = git::commit_all(workdir, &format!("fux(task): {}", task.title)).await?;
    let base = project
        .map(|p| p.default_branch.clone())
        .unwrap_or_else(|| "main".into());
    let mut remote = project
        .and_then(|p| p.remote_url.clone())
        .filter(|r| !r.is_empty());
    if remote.is_none() && git::has_remote(workdir).await {
        remote = git::git(workdir, &["remote", "get-url", "origin"])
            .await
            .ok();
    }
    match remote {
        Some(r) => {
            git::push(workdir, &branch).await?;
            Ok(Completion {
                pushed: true,
                head,
                pr_url: git::pr_create_url(&r, &branch, &base),
                pr_state: "open".into(),
            })
        }
        None => {
            // 无 remote：本地合并模式（唯一例外，批准需 admin 人工确认）
            Ok(Completion {
                pushed: false,
                head,
                pr_url: None,
                pr_state: "local".into(),
            })
        }
    }
}

/// Run 结束后的统一处置（本地与远程共用）。
pub async fn finish_run(
    state: &SharedState,
    run_id: &str,
    outcome: RunOutcome,
    remote: Option<(bool, Option<String>)>,
) {
    if let Err(e) = finish_inner(state, run_id, outcome, remote).await {
        tracing::error!("finish_run {run_id}: {e:#}");
    }
}

async fn finish_inner(
    state: &SharedState,
    run_id: &str,
    outcome: RunOutcome,
    remote: Option<(bool, Option<String>)>,
) -> ApiResult<()> {
    // 任务可能在 Run 执行期间被删除，runs 会随任务级联删除；此时结束回调直接忽略
    let Some(run) = sqlx::query_as::<_, Run>("SELECT * FROM runs WHERE id = ?")
        .bind(run_id)
        .fetch_optional(&state.db)
        .await?
    else {
        tracing::debug!("忽略已删除 Run {run_id} 的结束回调");
        return Ok(());
    };
    let Some(task) = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?")
        .bind(&run.task_id)
        .fetch_optional(&state.db)
        .await?
    else {
        tracing::debug!("忽略已删除任务 {} 的 Run {run_id} 结束回调", run.task_id);
        return Ok(());
    };
    // 工作流：先完成本节点交付（commit/push），再落 Run 状态，避免 DAG 引擎提前收敛
    if task.workflow_id.is_some() {
        let head_from_remote = remote.as_ref().and_then(|(_, h)| h.clone());
        if outcome.status != "finished" {
            sqlx::query("UPDATE runs SET status = ?, stop_reason = ?, turn_count = ?, acp_session_id = ?, head_sha = COALESCE(?, head_sha), ended_at = ? WHERE id = ?")
                .bind(&outcome.status).bind(&outcome.stop_reason).bind(outcome.turn_count as i64)
                .bind(&outcome.session_id).bind(&head_from_remote).bind(crate::now()).bind(run_id)
                .execute(&state.db).await?;
            workflow::finish_stage(state, &run, &outcome, head_from_remote, vec![]).await?;
            return Ok(());
        }
        let mut local_workdir: Option<std::path::PathBuf> = None;
        let head = match head_from_remote {
            Some(h) => Some(h),
            None => {
                let project = match &run.project_id {
                    Some(pid) => projects::load_project(state, pid).await.ok(),
                    None => None,
                };
                let workdir = match &run.stage_key {
                    Some(key) => git::stage_worktree_dir(&state.data_dir, &task.id, key),
                    None => git::run_worktree_dir(&state.data_dir, &task.id, run_id),
                };
                local_workdir = Some(workdir.clone());
                let branch = run.branch.clone().unwrap_or_default();
                match complete_local(state, &task, project.as_ref(), &workdir, &branch).await {
                    Ok(c) => match c.head {
                        Some(h) => Some(h),
                        // 无新提交时仍记录分支 HEAD，供下游节点引用产物
                        None => git::git(&workdir, &["rev-parse", "HEAD"]).await.ok(),
                    },
                    Err(e) => {
                        let stop_reason = "interrupted".to_string();
                        sqlx::query("UPDATE runs SET status = 'failed', stop_reason = ?, turn_count = ?, acp_session_id = ?, ended_at = ? WHERE id = ?")
                            .bind(&stop_reason).bind(outcome.turn_count as i64)
                            .bind(&outcome.session_id).bind(crate::now()).bind(run_id)
                            .execute(&state.db).await?;
                        let out = RunOutcome {
                            status: "failed".into(),
                            stop_reason,
                            turn_count: outcome.turn_count,
                            session_id: outcome.session_id.clone(),
                            error: Some(format!("交付失败: {e:#}")),
                        };
                        workflow::finish_stage(state, &run, &out, None, vec![]).await?;
                        return Ok(());
                    }
                }
            }
        };
        sqlx::query("UPDATE runs SET status = 'finished', stop_reason = ?, turn_count = ?, acp_session_id = ?, head_sha = ?, ended_at = ? WHERE id = ?")
            .bind(&outcome.stop_reason).bind(outcome.turn_count as i64)
            .bind(&outcome.session_id).bind(&head).bind(crate::now()).bind(run_id)
            .execute(&state.db).await?;
        let files = match (&local_workdir, &head) {
            (Some(wd), Some(h)) => git::git(wd, &["show", "--name-only", "--pretty=format:", h])
                .await
                .map(|s| {
                    s.lines()
                        .map(str::trim)
                        .filter(|l| !l.is_empty())
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default(),
            _ => vec![],
        };
        workflow::finish_stage(state, &run, &outcome, head, files).await?;
        return Ok(());
    }

    let head_from_remote = remote.as_ref().and_then(|(_, h)| h.clone());
    sqlx::query("UPDATE runs SET status = ?, stop_reason = ?, turn_count = ?, acp_session_id = ?, head_sha = COALESCE(?, head_sha), ended_at = ? WHERE id = ?")
        .bind(&outcome.status).bind(&outcome.stop_reason).bind(outcome.turn_count as i64)
        .bind(&outcome.session_id).bind(&head_from_remote).bind(crate::now()).bind(run_id)
        .execute(&state.db).await?;

    // 守卫：任务已被质量阀/租约回收处置时不再重复转移
    if task.status != "in_progress" || task.lease_run_id.as_deref() != Some(run_id) {
        return Ok(());
    }
    let actor = format!("agent:{}", run.agent_id);
    let project = match &task.project_id {
        Some(pid) => projects::load_project(state, pid).await.ok(),
        None => None,
    };

    match outcome.status.as_str() {
        "finished" => {
            // 完成交付：commit/push/PR
            let completion = match remote {
                Some((pushed, head)) => Completion {
                    pushed,
                    head,
                    pr_url: project
                        .as_ref()
                        .and_then(|p| p.remote_url.as_deref())
                        .and_then(|r| {
                            git::pr_create_url(
                                r,
                                task.branch.as_deref().unwrap_or(""),
                                &project
                                    .as_ref()
                                    .map(|p| p.default_branch.clone())
                                    .unwrap_or_else(|| "main".into()),
                            )
                        }),
                    pr_state: if pushed {
                        "open".into()
                    } else {
                        "local".into()
                    },
                },
                None => {
                    let wt = git::worktree_dir(&state.data_dir, &task.id);
                    let branch = task.branch.clone().unwrap_or_else(|| {
                        format!("fux/task-{}", &task.id[..8.min(task.id.len())])
                    });
                    match complete_local(state, &task, project.as_ref(), &wt, &branch).await {
                        Ok(c) => c,
                        Err(e) => {
                            domain::move_task(
                                state,
                                &task.id,
                                "blocked",
                                &actor,
                                MoveOpts {
                                    reason: Some(format!("交付失败（git push/认证？）: {e:#}")),
                                    ..Default::default()
                                },
                            )
                            .await?;
                            workspaces::notify(
                                state,
                                &task.workspace_id,
                                "blocked",
                                Some(&task.id),
                                &format!("任务「{}」交付失败被阻塞", task.title),
                            )
                            .await;
                            return Ok(());
                        }
                    }
                }
            };
            sqlx::query("UPDATE tasks SET pr_url = COALESCE(?, pr_url), pr_state = ?, merge_commit = NULL WHERE id = ?")
                .bind(&completion.pr_url).bind(&completion.pr_state).bind(&task.id).execute(&state.db).await?;
            events::append(&state.db, &state.broadcast, &task.id, "pr.linked", &actor, Some(run_id),
                json!({ "prUrl": completion.pr_url, "prState": completion.pr_state, "branch": task.branch, "pushed": completion.pushed })).await?;
            domain::move_task(state, &task.id, "review", &actor, MoveOpts::default()).await?;
            workspaces::notify(
                state,
                &task.workspace_id,
                "review",
                Some(&task.id),
                &format!("任务「{}」已完成，等待审核（PR/MR）", task.title),
            )
            .await;
        }
        "cancelled" => {
            domain::move_task(
                state,
                &task.id,
                "blocked",
                &actor,
                MoveOpts {
                    reason: Some("运行被取消".into()),
                    ..Default::default()
                },
            )
            .await?;
        }
        _ => {
            // 突然中断：自动重试 ≤ 2 次（新会话 + 摘要上下文）
            if outcome.stop_reason == "interrupted" && run.retry_count < 2 {
                let _summary = run_summary(state, run_id).await;
                events::append(
                    &state.db,
                    &state.broadcast,
                    &task.id,
                    "watchdog.interrupted",
                    "system",
                    Some(run_id),
                    json!({ "error": outcome.error, "retry": run.retry_count + 1 }),
                )
                .await?;
                let new_run = crate::new_id();
                sqlx::query("INSERT INTO runs (id, task_id, agent_id, node_id, stage, stage_key, project_id, branch, lease_expires_at, retry_count, retry_of) VALUES (?,?,?,?,?,?,?,?,?,?,?)")
                    .bind(&new_run).bind(&task.id).bind(&run.agent_id).bind(&run.node_id)
                    .bind(run.stage).bind(&run.stage_key).bind(&run.project_id).bind(&run.branch).bind(lease_deadline())
                    .bind(run.retry_count + 1).bind(run_id)
                    .execute(&state.db).await?;
                sqlx::query("UPDATE tasks SET lease_run_id = ?, lease_expires_at = ? WHERE id = ?")
                    .bind(&new_run)
                    .bind(lease_deadline())
                    .bind(&task.id)
                    .execute(&state.db)
                    .await?;
                crate::server::scheduler::dispatch(state, &new_run, vec![]).await;
            } else {
                domain::move_task(
                    state,
                    &task.id,
                    "blocked",
                    &actor,
                    MoveOpts {
                        reason: Some(format!(
                            "运行失败: {}",
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
                    &format!("任务「{}」运行失败被阻塞", task.title),
                )
                .await;
            }
        }
    }
    Ok(())
}

pub fn lease_deadline() -> String {
    (chrono::Utc::now() + chrono::Duration::seconds(30))
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

/// 取某 Run 的 agent 输出摘要（用于重试/交接上下文）。
pub async fn run_summary(state: &SharedState, run_id: &str) -> String {
    let rows = sqlx::query_as::<_, crate::server::models::TaskEvent>(
        "SELECT * FROM task_events WHERE run_id = ? AND kind = 'agent.message' ORDER BY seq DESC LIMIT 40",
    ).bind(run_id).fetch_all(&state.db).await.unwrap_or_default();
    let mut text = String::new();
    for r in rows.iter().rev() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&r.payload) {
            if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
                text.push_str(t);
            }
        }
    }
    if text.len() > 2000 {
        text = text[text.len() - 2000..].to_string();
    }
    text
}

/// 单任务合并检测（API 手动触发）。
pub async fn check_merge_now(state: &SharedState, task: &Task) -> anyhow::Result<bool> {
    if task.workflow_id.is_some() {
        return workflow::check_deliveries(state, task).await;
    }
    if task.pr_state.as_deref() == Some("merged") {
        return Ok(true);
    }
    let pid = match &task.project_id {
        Some(p) => p.clone(),
        None => return Ok(false),
    };
    let project = projects::load_project(state, &pid).await?;
    let remote = match &project.remote_url {
        Some(r) if !r.is_empty() => r.clone(),
        _ => return Ok(false),
    };
    let branch = match &task.branch {
        Some(b) => b.clone(),
        None => return Ok(false),
    };
    let mirror = state.data_dir.join("mirrors").join(&pid);
    git::ensure_clone(&remote, &mirror).await?;
    if git::is_merged(&mirror, &branch, &project.default_branch).await? {
        let head = git::git(&mirror, &["rev-parse", &branch]).await.ok();
        sqlx::query("UPDATE tasks SET pr_state = 'merged', merge_commit = ? WHERE id = ?")
            .bind(&head)
            .bind(&task.id)
            .execute(&state.db)
            .await?;
        events::append(
            &state.db,
            &state.broadcast,
            &task.id,
            "pr.merged_detected",
            "system",
            None,
            json!({ "mergeCommit": head }),
        )
        .await?;
        workspaces::notify(
            state,
            &task.workspace_id,
            "merged",
            Some(&task.id),
            &format!("任务「{}」的 PR/MR 已检测到合并，可批准", task.title),
        )
        .await;
        return Ok(true);
    }
    Ok(false)
}

/// 周期合并检测（review 中的任务）。
pub fn spawn_merge_checker(state: SharedState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let tasks = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE status = 'review' AND COALESCE(pr_state, '') != 'merged'")
                .fetch_all(&state.db).await.unwrap_or_default();
            for t in tasks {
                if let Err(e) = check_merge_now(&state, &t).await {
                    tracing::debug!("merge check {}: {e}", t.id);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{AppState, NodeRegistry};
    use std::collections::HashMap;
    use tokio::sync::{Mutex, broadcast};

    #[tokio::test]
    async fn finish_run_ignores_deleted_run() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::server::db::init(dir.path()).await.unwrap();
        let (broadcast, _) = broadcast::channel(8);
        let state = actix_web::web::Data::new(AppState {
            db,
            jwt_secret: "test".into(),
            data_dir: dir.path().to_path_buf(),
            broadcast,
            nodes: NodeRegistry::default(),
            local_runs: Mutex::new(HashMap::new()),
            api_base: "http://127.0.0.1:0".into(),
        });
        let outcome = RunOutcome {
            status: "finished".into(),
            stop_reason: String::new(),
            turn_count: 0,
            session_id: String::new(),
            error: None,
        };
        // 任务被删除时 runs 已级联删除，迟到的 Run 结束回调应被忽略而不是报错
        assert!(
            finish_inner(&state, "missing-run", outcome, None)
                .await
                .is_ok()
        );
    }
}
