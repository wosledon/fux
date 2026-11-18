use crate::server::auth::{require, AuthUser};
use crate::server::domain::{self, MoveOpts};
use crate::server::error::{ApiError, ApiResult};
use crate::server::events;
use crate::server::models::{Lane, Run, Task};
use crate::server::SharedState;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use serde_json::json;

pub struct NewTask {
    pub board_id: String,
    pub workspace_id: String,
    pub type_id: Option<String>,
    pub project_id: Option<String>,
    pub title: String,
    pub content: String,
    pub priority: i64,
    pub tags: Vec<String>,
    pub assignee_agent_id: Option<String>,
    pub assignee_team_id: Option<String>,
    pub created_by: Option<String>,
}

pub async fn create_task(state: &SharedState, n: NewTask) -> ApiResult<Task> {
    let lane: Option<Lane> = sqlx::query_as("SELECT * FROM lanes WHERE board_id = ? AND kind = 'planning' ORDER BY position LIMIT 1")
        .bind(&n.board_id).fetch_optional(&state.db).await?;
    let id = crate::new_id();
    let branch = format!("fux/task-{}", &id[..8]);
    sqlx::query(
        "INSERT INTO tasks (id, board_id, workspace_id, type_id, project_id, title, content, status, lane_id, priority, tags, assignee_agent_id, assignee_team_id, branch, created_by) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(&id).bind(&n.board_id).bind(&n.workspace_id).bind(&n.type_id).bind(&n.project_id)
    .bind(&n.title).bind(&n.content).bind("planning")
    .bind(lane.map(|l| l.id)).bind(n.priority)
    .bind(serde_json::to_string(&n.tags).unwrap_or_else(|_| "[]".into()))
    .bind(&n.assignee_agent_id).bind(&n.assignee_team_id).bind(&branch).bind(&n.created_by)
    .execute(&state.db).await?;
    let task = domain::load_task(state, &id).await?;
    events::append(&state.db, &state.broadcast, &id, "task.created",
        &n.created_by.clone().map(|u| format!("user:{u}")).unwrap_or_else(|| "system".into()),
        None, json!({ "task": task })).await?;
    Ok(task)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListQuery {
    board_id: String,
    status: Option<String>,
    project_id: Option<String>,
    q: Option<String>,
}

async fn list(state: SharedState, user: AuthUser, query: web::Query<ListQuery>) -> ApiResult<HttpResponse> {
    let board = crate::server::api::boards::load_board(&state, &query.board_id).await?;
    require(&state.db, &board.workspace_id, &user.id, "member").await?;
    let mut sql = String::from("SELECT * FROM tasks WHERE board_id = ?");
    if query.status.is_some() { sql.push_str(" AND status = ?"); }
    if query.project_id.is_some() { sql.push_str(" AND project_id = ?"); }
    if query.q.is_some() { sql.push_str(" AND title LIKE ?"); }
    sql.push_str(" ORDER BY priority DESC, created_at ASC");
    let mut q = sqlx::query_as::<_, Task>(&sql).bind(&query.board_id);
    if let Some(s) = &query.status { q = q.bind(s); }
    if let Some(p) = &query.project_id { q = q.bind(p); }
    if let Some(s) = &query.q { q = q.bind(format!("%{s}%")); }
    let mut rows = q.fetch_all(&state.db).await?;
    // 已归档泳道只显示近 7 天（数据永久保留）
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(7)).format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    rows.retain(|t| t.status != "archived" || t.updated_at >= cutoff);
    Ok(HttpResponse::Ok().json(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateReq {
    board_id: String,
    title: String,
    content: Option<String>,
    type_id: Option<String>,
    project_id: Option<String>,
    priority: Option<i64>,
    tags: Option<Vec<String>>,
    assignee_agent_id: Option<String>,
    assignee_team_id: Option<String>,
}

async fn create(state: SharedState, user: AuthUser, body: web::Json<CreateReq>) -> ApiResult<HttpResponse> {
    let board = crate::server::api::boards::load_board(&state, &body.board_id).await?;
    require(&state.db, &board.workspace_id, &user.id, "member").await?;
    if body.title.trim().is_empty() { return Err(ApiError::bad_request("标题不能为空")); }
    let task = create_task(&state, NewTask {
        board_id: board.id.clone(),
        workspace_id: board.workspace_id.clone(),
        type_id: body.type_id.clone(),
        project_id: body.project_id.clone(),
        title: body.title.trim().to_string(),
        content: body.content.clone().unwrap_or_default(),
        priority: body.priority.unwrap_or(0),
        tags: body.tags.clone().unwrap_or_default(),
        assignee_agent_id: body.assignee_agent_id.clone(),
        assignee_team_id: body.assignee_team_id.clone(),
        created_by: Some(user.id.clone()),
    }).await?;
    Ok(HttpResponse::Ok().json(task))
}

async fn get(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let task = domain::load_task(&state, &id).await?;
    require(&state.db, &task.workspace_id, &user.id, "member").await?;
    let runs = sqlx::query_as::<_, Run>("SELECT * FROM runs WHERE task_id = ? ORDER BY started_at DESC").bind(&id).fetch_all(&state.db).await?;
    let project = match &task.project_id {
        Some(pid) => crate::server::api::projects::load_project(&state, pid).await.ok(),
        None => None,
    };
    Ok(HttpResponse::Ok().json(json!({ "task": task, "runs": runs, "project": project })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchReq {
    title: Option<String>,
    content: Option<String>,
    priority: Option<i64>,
    tags: Option<Vec<String>>,
    project_id: Option<String>,
    assignee_agent_id: Option<String>,
    assignee_team_id: Option<String>,
    lane_id: Option<String>,
}

async fn patch(state: SharedState, user: AuthUser, path: web::Path<String>, body: web::Json<PatchReq>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let task = domain::load_task(&state, &id).await?;
    let role = require(&state.db, &task.workspace_id, &user.id, "member").await?;
    if ["archived", "cancelled"].contains(&task.status.as_str()) {
        return Err(ApiError::conflict("已归档/已取消任务不可编辑"));
    }
    if let Some(lane_id) = &body.lane_id {
        let lane = sqlx::query_as::<_, Lane>("SELECT * FROM lanes WHERE id = ?").bind(lane_id).fetch_one(&state.db).await?;
        let want = domain::lane_kind(&task.status).unwrap_or("work");
        if lane.kind != want && role != "owner" && role != "admin" {
            return Err(ApiError::forbidden("跨状态移动请使用状态转移"));
        }
        sqlx::query("UPDATE tasks SET lane_id = ? WHERE id = ?").bind(lane_id).bind(&id).execute(&state.db).await?;
    }
    macro_rules! set {
        ($field:expr, $col:expr) => {
            if let Some(v) = $field {
                sqlx::query(&format!("UPDATE tasks SET {} = ? WHERE id = ?", $col)).bind(v).bind(&id).execute(&state.db).await?;
            }
        };
    }
    set!(&body.title, "title");
    set!(&body.content, "content");
    set!(body.priority, "priority");
    set!(&body.project_id, "project_id");
    set!(&body.assignee_agent_id, "assignee_agent_id");
    set!(&body.assignee_team_id, "assignee_team_id");
    if let Some(tags) = &body.tags {
        sqlx::query("UPDATE tasks SET tags = ? WHERE id = ?").bind(serde_json::to_string(tags).unwrap()).bind(&id).execute(&state.db).await?;
    }
    sqlx::query("UPDATE tasks SET version = version + 1, updated_at = ? WHERE id = ?").bind(crate::now()).bind(&id).execute(&state.db).await?;
    let updated = domain::load_task(&state, &id).await?;
    events::append(&state.db, &state.broadcast, &id, "task.updated", &format!("user:{}", user.id), None, json!({ "task": updated })).await?;
    Ok(HttpResponse::Ok().json(updated))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransitionReq {
    to: String,
    reason: Option<String>,
    lane_id: Option<String>,
    manual_confirm: Option<bool>,
    expected_version: Option<i64>,
}

async fn transition(state: SharedState, user: AuthUser, path: web::Path<String>, body: web::Json<TransitionReq>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let task = domain::load_task(&state, &id).await?;
    let role = require(&state.db, &task.workspace_id, &user.id, "member").await?;
    let manual = body.manual_confirm.unwrap_or(false);
    if manual && crate::server::auth::role_rank(&role) < 2 {
        return Err(ApiError::forbidden("人工确认合并需要 admin 权限"));
    }
    let updated = domain::move_task(&state, &id, &body.to, &format!("user:{}", user.id), MoveOpts {
        reason: body.reason.clone(),
        lane_id: body.lane_id.clone(),
        manual_confirm: manual,
        expected_version: body.expected_version,
    }).await?;
    if body.to == "archived" {
        crate::server::api::workspaces::notify(&state, &task.workspace_id, "archived", Some(&id), &format!("任务「{}」已归档", task.title)).await;
    }
    Ok(HttpResponse::Ok().json(updated))
}

async fn unblock(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let task = domain::load_task(&state, &id).await?;
    require(&state.db, &task.workspace_id, &user.id, "member").await?;
    if task.status != "blocked" { return Err(ApiError::conflict("任务未处于阻塞状态")); }
    let target = task.blocked_from.clone().unwrap_or_else(|| "ready".into());
    let target = if domain::legal("blocked", &target) { target } else { "ready".into() };
    let updated = domain::move_task(&state, &id, &target, &format!("user:{}", user.id), MoveOpts {
        reason: Some("手动解除阻塞".into()), ..Default::default()
    }).await?;
    Ok(HttpResponse::Ok().json(updated))
}

#[derive(Deserialize)]
struct CommentReq { text: String }

async fn comment(state: SharedState, user: AuthUser, path: web::Path<String>, body: web::Json<CommentReq>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let task = domain::load_task(&state, &id).await?;
    require(&state.db, &task.workspace_id, &user.id, "member").await?;
    if body.text.trim().is_empty() { return Err(ApiError::bad_request("评论不能为空")); }
    let updated = domain::add_comment(&state, &id, &user.id, &user.username, body.text.trim()).await?;
    Ok(HttpResponse::Ok().json(updated))
}

async fn archive(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let task = domain::load_task(&state, &id).await?;
    require(&state.db, &task.workspace_id, &user.id, "admin").await?;
    let updated = domain::move_task(&state, &id, "archived", &format!("user:{}", user.id), MoveOpts::default()).await?;
    Ok(HttpResponse::Ok().json(updated))
}

/// 取消正在执行的 Run（→ 阻塞）。
async fn cancel_run(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let task = domain::load_task(&state, &id).await?;
    require(&state.db, &task.workspace_id, &user.id, "member").await?;
    let run = sqlx::query_as::<_, Run>("SELECT * FROM runs WHERE task_id = ? AND status = 'running' ORDER BY started_at DESC LIMIT 1")
        .bind(&id).fetch_optional(&state.db).await?;
    if let Some(run) = run {
        crate::server::scheduler::cancel_run(&state, &run).await;
    }
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

async fn get_pr(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let task = domain::load_task(&state, &id).await?;
    require(&state.db, &task.workspace_id, &user.id, "member").await?;
    let mut create_url = None;
    if let Some(pid) = &task.project_id {
        if let Ok(p) = crate::server::api::projects::load_project(&state, pid).await {
            if let Some(remote) = &p.remote_url {
                create_url = crate::git::pr_create_url(remote, task.branch.as_deref().unwrap_or(""), &p.default_branch);
            }
        }
    }
    Ok(HttpResponse::Ok().json(json!({
        "prUrl": task.pr_url, "prState": task.pr_state, "mergeCommit": task.merge_commit,
        "branch": task.branch, "createUrl": create_url,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrReq { pr_url: String }

async fn set_pr(state: SharedState, user: AuthUser, path: web::Path<String>, body: web::Json<PrReq>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let task = domain::load_task(&state, &id).await?;
    require(&state.db, &task.workspace_id, &user.id, "member").await?;
    sqlx::query("UPDATE tasks SET pr_url = ?, pr_state = COALESCE(pr_state, 'open') WHERE id = ?")
        .bind(&body.pr_url).bind(&id).execute(&state.db).await?;
    events::append(&state.db, &state.broadcast, &id, "pr.linked", &format!("user:{}", user.id), None, json!({ "prUrl": body.pr_url })).await?;
    Ok(HttpResponse::Ok().json(domain::load_task(&state, &id).await?))
}

async fn check_merge(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let task = domain::load_task(&state, &id).await?;
    require(&state.db, &task.workspace_id, &user.id, "member").await?;
    let merged = crate::server::gitflow::check_merge_now(&state, &task).await.unwrap_or(false);
    Ok(HttpResponse::Ok().json(json!({ "merged": merged, "task": domain::load_task(&state, &id).await? })))
}

async fn delete(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let task = domain::load_task(&state, &id).await?;
    require(&state.db, &task.workspace_id, &user.id, "admin").await?;
    sqlx::query("DELETE FROM tasks WHERE id = ?").bind(&id).execute(&state.db).await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/tasks", web::get().to(list))
        .route("/tasks", web::post().to(create))
        .route("/tasks/{id}", web::get().to(get))
        .route("/tasks/{id}", web::patch().to(patch))
        .route("/tasks/{id}", web::delete().to(delete))
        .route("/tasks/{id}/transition", web::post().to(transition))
        .route("/tasks/{id}/unblock", web::post().to(unblock))
        .route("/tasks/{id}/comments", web::post().to(comment))
        .route("/tasks/{id}/archive", web::post().to(archive))
        .route("/tasks/{id}/cancel-run", web::post().to(cancel_run))
        .route("/tasks/{id}/pull-request", web::get().to(get_pr))
        .route("/tasks/{id}/pull-request", web::post().to(set_pr))
        .route("/tasks/{id}/check-merge", web::post().to(check_merge));
}