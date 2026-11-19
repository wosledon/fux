//! 任务状态机（PRD 3.5.2 转移表 T1-T11）+ 评论语义（3.5.3）。
//! 所有转移集中在此，API 与调度器共用，杜绝非法转移。
use crate::server::AppState;
use crate::server::error::{ApiError, ApiResult};
use crate::server::events;
use crate::server::models::{Lane, Task};
use serde_json::json;

pub fn lane_kind(status: &str) -> Option<&'static str> {
    match status {
        "planning" => Some("planning"),
        "ready" => Some("ready"),
        "in_progress" => Some("work"),
        "review" => Some("review"),
        "blocked" => Some("blocked"),
        "done" => Some("done"),
        "archived" => Some("archived"),
        _ => None,
    }
}

/// 状态机合法性（与执行者无关）。
pub fn legal(from: &str, to: &str) -> bool {
    matches!(
        (from, to),
        ("planning", "ready")
            | ("planning", "cancelled")
            | ("ready", "in_progress")
            | ("ready", "cancelled")
            | ("in_progress", "review")
            | ("in_progress", "blocked")
            | ("in_progress", "in_progress")
            | ("review", "done")
            | ("review", "ready")
            | ("blocked", "ready")
            | ("blocked", "in_progress")
            | ("blocked", "review")
            | ("done", "ready")
            | ("done", "archived")
    )
}

/// 用户可手动触发的转移（其余由智能体/系统执行）。
pub fn user_can(from: &str, to: &str) -> bool {
    matches!(
        (from, to),
        ("planning", "ready")
            | ("planning", "cancelled")
            | ("ready", "cancelled")
            | ("review", "done")
            | ("review", "ready")
            | ("blocked", "ready")
            | ("blocked", "in_progress")
            | ("blocked", "review")
            | ("done", "ready")
            | ("done", "archived")
    )
}

#[derive(Default)]
pub struct MoveOpts {
    pub reason: Option<String>,
    pub lane_id: Option<String>,
    /// admin 人工确认 PR/MR 已合并（自动检测兜底）
    pub manual_confirm: bool,
    /// 乐观锁：多人并发冲突检测
    pub expected_version: Option<i64>,
}

pub async fn load_task(state: &AppState, task_id: &str) -> ApiResult<Task> {
    Ok(
        sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?")
            .bind(task_id)
            .fetch_one(&state.db)
            .await?,
    )
}

pub async fn move_task(
    state: &AppState,
    task_id: &str,
    to: &str,
    actor: &str,
    opts: MoveOpts,
) -> ApiResult<Task> {
    let task = load_task(state, task_id).await?;
    let from = task.status.clone();

    if let Some(v) = opts.expected_version {
        if v != task.version {
            return Err(ApiError::conflict("任务已被他人更新，请刷新后重试"));
        }
    }
    if from == to && to != "in_progress" {
        return Ok(task);
    }
    if !legal(&from, to) {
        return Err(ApiError::conflict(format!("非法状态转移: {from} → {to}")));
    }
    if actor.starts_with("user:") && !user_can(&from, to) {
        return Err(ApiError::forbidden("该转移只能由智能体/系统执行"));
    }
    // T5：审核通过必须检测到合并（或 admin 人工确认）；工作流要求全部交付已合并
    if to == "done" && !opts.manual_confirm {
        let merged = if task.workflow_id.is_some() {
            let total: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM deliveries WHERE task_id = ?")
                    .bind(task_id)
                    .fetch_one(&state.db)
                    .await?;
            let done: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM deliveries WHERE task_id = ? AND pr_state = 'merged'",
            )
            .bind(task_id)
            .fetch_one(&state.db)
            .await?;
            (total > 0 && done == total) || task.pr_state.as_deref() == Some("merged")
        } else {
            task.pr_state.as_deref() == Some("merged")
        };
        if !merged {
            return Err(ApiError::conflict(
                "PR/MR 尚未检测到合并，无法批准；如已线下合并可由 admin 人工确认",
            ));
        }
    }

    // 工作流：驳回/完成返工只重跑汇点节点；解除阻塞时重置失败节点以便重新派发
    if task.workflow_id.is_some() {
        if (from == "review" || from == "done") && to == "ready" {
            crate::server::workflow::reset_sinks_for_rework(state, task_id).await?;
        } else if from == "blocked" {
            crate::server::workflow::reset_failed_runs(state, task_id).await?;
        }
    }

    let mut blocked_from = task.blocked_from.clone();
    let mut block_reason = task.block_reason.clone();
    if to == "blocked" {
        blocked_from = Some(from.clone());
        block_reason = opts.reason.clone().or(Some("未知原因".into()));
    }
    if from == "blocked" {
        blocked_from = None;
        block_reason = None;
    }
    let mut retry_count = task.retry_count;
    if from == "review" && to == "ready" {
        retry_count += 1;
    }

    // 泳道解析：优先指定；in_progress 保留当前工作子泳道
    let lanes =
        sqlx::query_as::<_, Lane>("SELECT * FROM lanes WHERE board_id = ? ORDER BY position")
            .bind(&task.board_id)
            .fetch_all(&state.db)
            .await?;
    let lane_id = opts.lane_id.clone().or_else(|| {
        if to == "in_progress" {
            if let Some(cur) = &task.lane_id {
                if lanes.iter().any(|l| &l.id == cur && l.kind == "work") {
                    return Some(cur.clone());
                }
            }
        }
        lane_kind(to).and_then(|k| lanes.iter().find(|l| l.kind == k).map(|l| l.id.clone()))
    });

    let now = crate::now();
    let clear_lease = to != "in_progress";
    sqlx::query(
        "UPDATE tasks SET status=?, blocked_from=?, block_reason=?, lane_id=?, retry_count=?, version=version+1, updated_at=?, lease_run_id=CASE WHEN ? THEN NULL ELSE lease_run_id END, lease_expires_at=CASE WHEN ? THEN NULL ELSE lease_expires_at END WHERE id=?",
    )
    .bind(to)
    .bind(&blocked_from)
    .bind(&block_reason)
    .bind(&lane_id)
    .bind(retry_count)
    .bind(&now)
    .bind(clear_lease)
    .bind(clear_lease)
    .bind(task_id)
    .execute(&state.db)
    .await?;

    let updated = load_task(state, task_id).await?;
    events::append(
        &state.db,
        &state.broadcast,
        task_id,
        "task.moved",
        actor,
        None,
        json!({ "from": from, "to": to, "reason": opts.reason, "task": updated }),
    )
    .await?;
    Ok(updated)
}

/// 评论语义（PRD 3.5.3）：进行中→引导；已完成→返工；已阻塞→解答并自动解除。
pub async fn add_comment(
    state: &AppState,
    task_id: &str,
    user_id: &str,
    username: &str,
    text: &str,
) -> ApiResult<Task> {
    let task = load_task(state, task_id).await?;
    events::append(
        &state.db,
        &state.broadcast,
        task_id,
        "comment.added",
        &format!("user:{user_id}"),
        None,
        json!({ "text": text, "username": username }),
    )
    .await?;
    match task.status.as_str() {
        "done" => {
            // T7 返工闭环
            move_task(
                state,
                task_id,
                "ready",
                &format!("user:{user_id}"),
                MoveOpts {
                    reason: Some("已完成任务收到评论，触发返工".into()),
                    ..Default::default()
                },
            )
            .await
        }
        "blocked" => {
            // T9：评论视为解答，自动解除阻塞回来源状态
            let target = task.blocked_from.clone().unwrap_or_else(|| "ready".into());
            let target = if legal("blocked", &target) {
                target
            } else {
                "ready".into()
            };
            move_task(
                state,
                task_id,
                &target,
                &format!("user:{user_id}"),
                MoveOpts {
                    reason: Some("评论解答，自动解除阻塞".into()),
                    ..Default::default()
                },
            )
            .await
        }
        _ => Ok(task),
    }
}

/// 待注入的引导评论：comment.added 中尚未被 comment.injected 标记的。
pub async fn pending_guidance(state: &AppState, task_id: &str) -> Vec<(i64, String)> {
    let rows = sqlx::query_as::<_, crate::server::models::TaskEvent>(
        "SELECT * FROM task_events WHERE task_id = ? AND kind IN ('comment.added','comment.injected') ORDER BY seq",
    )
    .bind(task_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    let mut injected: Vec<i64> = vec![];
    let mut added: Vec<(i64, String)> = vec![];
    for r in rows {
        let payload: serde_json::Value = serde_json::from_str(&r.payload).unwrap_or(json!({}));
        if r.kind == "comment.injected" {
            if let Some(seqs) = payload.get("seqs").and_then(|s| s.as_array()) {
                injected.extend(seqs.iter().filter_map(|s| s.as_i64()));
            }
        } else if let Some(text) = payload.get("text").and_then(|t| t.as_str()) {
            added.push((r.seq, text.to_string()));
        }
    }
    added
        .into_iter()
        .filter(|(seq, _)| !injected.contains(seq))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_table() {
        assert!(legal("planning", "ready"));
        assert!(legal("ready", "in_progress"));
        assert!(legal("in_progress", "review"));
        assert!(legal("in_progress", "blocked"));
        assert!(legal("review", "done"));
        assert!(legal("review", "ready"));
        assert!(legal("done", "ready"));
        assert!(legal("done", "archived"));
        assert!(legal("blocked", "in_progress"));
        // 非法
        assert!(!legal("planning", "in_progress"));
        assert!(!legal("ready", "done"));
        assert!(!legal("archived", "ready"));
        assert!(!legal("done", "in_progress"));
        assert!(!legal("review", "archived"));
    }

    #[test]
    fn user_permissions() {
        assert!(user_can("planning", "ready"));
        assert!(user_can("review", "done"));
        assert!(!user_can("ready", "in_progress")); // 只能调度器认领
        assert!(!user_can("in_progress", "review")); // 只能智能体报告
    }
}
