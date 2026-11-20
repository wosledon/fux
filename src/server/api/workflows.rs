//! 工作流模板：CRUD 与「一句话发起」（生成待办任务，由 DAG 引擎推进）。
use crate::server::SharedState;
use crate::server::auth::{AuthUser, require};
use crate::server::error::{ApiError, ApiResult};
use crate::server::events;
use crate::server::models::{Lane, Workflow};
use crate::server::workflow::{self, StageSpec, WorkflowVariable};
use actix_web::{HttpResponse, web};
use serde::Deserialize;
use serde_json::json;

async fn load(state: &SharedState, id: &str) -> ApiResult<Workflow> {
    Ok(
        sqlx::query_as::<_, Workflow>("SELECT * FROM workflows WHERE id = ?")
            .bind(id)
            .fetch_one(&state.db)
            .await?,
    )
}

async fn list(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
) -> ApiResult<HttpResponse> {
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &user.id, "member").await?;
    let rows = sqlx::query_as::<_, Workflow>(
        "SELECT * FROM workflows WHERE workspace_id = ? ORDER BY name",
    )
    .bind(&ws_id)
    .fetch_all(&state.db)
    .await?;
    Ok(HttpResponse::Ok().json(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowReq {
    name: String,
    description: Option<String>,
    #[serde(default)]
    variables: Option<Vec<WorkflowVariable>>,
    stages: Vec<StageSpec>,
}

async fn check_refs(state: &SharedState, ws_id: &str, stages: &[StageSpec]) -> ApiResult<()> {
    for s in stages {
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agents WHERE id = ? AND workspace_id = ?")
                .bind(&s.agent_id)
                .bind(ws_id)
                .fetch_one(&state.db)
                .await
                .unwrap_or(0);
        if n == 0 {
            return Err(ApiError::bad_request(format!(
                "节点「{}」绑定的智能体不属于该工作区",
                s.name
            )));
        }
        if let Some(pid) = &s.project_id {
            let n: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM projects WHERE id = ? AND workspace_id = ?",
            )
            .bind(pid)
            .bind(ws_id)
            .fetch_one(&state.db)
            .await
            .unwrap_or(0);
            if n == 0 {
                return Err(ApiError::bad_request(format!(
                    "节点「{}」的项目不属于该工作区",
                    s.name
                )));
            }
        }
    }
    Ok(())
}

async fn create(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
    body: web::Json<WorkflowReq>,
) -> ApiResult<HttpResponse> {
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &user.id, "admin").await?;
    if body.name.trim().is_empty() {
        return Err(ApiError::bad_request("工作流名称不能为空"));
    }
    workflow::validate(&body.stages).map_err(ApiError::bad_request)?;
    let variables = body.variables.clone().unwrap_or_default();
    workflow::validate_variables(&variables).map_err(ApiError::bad_request)?;
    check_refs(&state, &ws_id, &body.stages).await?;
    let id = crate::new_id();
    sqlx::query(
        "INSERT INTO workflows (id, workspace_id, name, description, variables, stages) VALUES (?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(&ws_id)
    .bind(body.name.trim())
    .bind(body.description.clone().unwrap_or_default())
    .bind(serde_json::to_string(&variables).unwrap_or_else(|_| "[]".into()))
    .bind(serde_json::to_string(&body.stages).unwrap_or_else(|_| "[]".into()))
    .execute(&state.db)
    .await?;
    Ok(HttpResponse::Ok().json(load(&state, &id).await?))
}

async fn get(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let wf = load(&state, &id).await?;
    require(&state.db, &wf.workspace_id, &user.id, "member").await?;
    Ok(HttpResponse::Ok().json(wf))
}

async fn patch(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
    body: web::Json<WorkflowReq>,
) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let wf = load(&state, &id).await?;
    require(&state.db, &wf.workspace_id, &user.id, "admin").await?;
    workflow::validate(&body.stages).map_err(ApiError::bad_request)?;
    let variables = match &body.variables {
        Some(variables) => variables.clone(),
        None => workflow::parse_variables(&wf)?,
    };
    workflow::validate_variables(&variables).map_err(ApiError::bad_request)?;
    check_refs(&state, &wf.workspace_id, &body.stages).await?;
    sqlx::query(
        "UPDATE workflows SET name = ?, description = ?, variables = ?, stages = ?, updated_at = ? WHERE id = ?",
    )
    .bind(body.name.trim())
    .bind(body.description.clone().unwrap_or_default())
    .bind(serde_json::to_string(&variables).unwrap_or_else(|_| "[]".into()))
    .bind(serde_json::to_string(&body.stages).unwrap_or_else(|_| "[]".into()))
    .bind(crate::now())
    .bind(&id)
    .execute(&state.db)
    .await?;
    Ok(HttpResponse::Ok().json(load(&state, &id).await?))
}

async fn delete(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let wf = load(&state, &id).await?;
    require(&state.db, &wf.workspace_id, &user.id, "admin").await?;
    sqlx::query("DELETE FROM workflows WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunReq {
    title: String,
    content: Option<String>,
    project_id: Option<String>,
    type_id: Option<String>,
    priority: Option<i64>,
    tags: Option<Vec<String>>,
    variables: Option<serde_json::Value>,
}

/// 一句话发起：直接生成 ready 任务，调度器认领后由 DAG 引擎派发入口节点。
async fn run(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
    body: web::Json<RunReq>,
) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let wf = load(&state, &id).await?;
    require(&state.db, &wf.workspace_id, &user.id, "member").await?;
    if body.title.trim().is_empty() {
        return Err(ApiError::bad_request("标题不能为空"));
    }
    let stages: Vec<StageSpec> = workflow::parse_stages(&wf)?;
    if stages.is_empty() {
        return Err(ApiError::bad_request("工作流没有节点"));
    }
    let board_id: String = sqlx::query_scalar("SELECT id FROM boards WHERE workspace_id = ?")
        .bind(&wf.workspace_id)
        .fetch_one(&state.db)
        .await?;
    let lane: Option<Lane> = sqlx::query_as(
        "SELECT * FROM lanes WHERE board_id = ? AND kind = 'ready' ORDER BY position LIMIT 1",
    )
    .bind(&board_id)
    .fetch_optional(&state.db)
    .await?;
    let project_id = body
        .project_id
        .clone()
        .or_else(|| stages[0].project_id.clone());
    if let Some(pid) = &project_id {
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE id = ? AND workspace_id = ?")
                .bind(pid)
                .bind(&wf.workspace_id)
                .fetch_one(&state.db)
                .await?;
        if n == 0 {
            return Err(ApiError::bad_request("主项目不属于该工作区"));
        }
    }
    let variable_specs = workflow::parse_variables(&wf)?;
    let inputs = workflow::resolve_inputs(&variable_specs, body.variables.as_ref())?;
    let title = workflow::render_template(body.title.trim(), &inputs);
    if title.trim().is_empty() {
        return Err(ApiError::bad_request("标题不能为空"));
    }
    let content = workflow::render_template(&body.content.clone().unwrap_or_default(), &inputs);
    let tags: Vec<String> = body
        .tags
        .clone()
        .unwrap_or_default()
        .iter()
        .map(|tag| workflow::render_template(tag, &inputs))
        .collect();
    let task_id = crate::new_id();
    sqlx::query(
        "INSERT INTO tasks (id, board_id, workspace_id, type_id, project_id, workflow_id, workflow_inputs, title, content, status, lane_id, priority, tags, created_by) VALUES (?,?,?,?,?,?,?,?,?, 'ready', ?,?,?,?)",
    )
    .bind(&task_id).bind(&board_id).bind(&wf.workspace_id).bind(&body.type_id)
    .bind(&project_id).bind(&wf.id)
    .bind(serde_json::to_string(&inputs).unwrap_or_else(|_| "{}".into()))
    .bind(title).bind(content)
    .bind(lane.map(|l| l.id)).bind(body.priority.unwrap_or(0))
    .bind(serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into()))
    .bind(&user.id)
    .execute(&state.db).await?;
    let task = crate::server::domain::load_task(&state, &task_id).await?;
    events::append(
        &state.db,
        &state.broadcast,
        &task_id,
        "task.created",
        &format!("user:{}", user.id),
        None,
        json!({ "task": task }),
    )
    .await?;
    events::append(
        &state.db,
        &state.broadcast,
        &task_id,
        "workflow.started",
        &format!("user:{}", user.id),
        None,
        json!({
            "workflowId": wf.id,
            "workflowName": wf.name,
            "stages": stages.len(),
            "variableKeys": inputs.keys().cloned().collect::<Vec<_>>()
        }),
    )
    .await?;
    Ok(HttpResponse::Ok().json(task))
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/workspaces/{id}/workflows", web::get().to(list))
        .route("/workspaces/{id}/workflows", web::post().to(create))
        .route("/workflows/{id}", web::get().to(get))
        .route("/workflows/{id}", web::patch().to(patch))
        .route("/workflows/{id}", web::delete().to(delete))
        .route("/workflows/{id}/run", web::post().to(run));
}
