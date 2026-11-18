use crate::server::api::tasks::{create_task, NewTask};
use crate::server::auth::{require, AuthUser};
use crate::server::error::{ApiError, ApiResult};
use crate::server::models::TaskType;
use crate::server::SharedState;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use serde_json::{json, Value};

async fn load(state: &SharedState, id: &str) -> ApiResult<TaskType> {
    Ok(sqlx::query_as::<_, TaskType>("SELECT * FROM task_types WHERE id = ?").bind(id).fetch_one(&state.db).await?)
}

async fn list(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &user.id, "member").await?;
    let rows = sqlx::query_as::<_, TaskType>("SELECT * FROM task_types WHERE workspace_id = ? ORDER BY name").bind(&ws_id).fetch_all(&state.db).await?;
    Ok(HttpResponse::Ok().json(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TypeReq {
    name: String,
    icon: Option<String>,
    title_template: Option<String>,
    content_template: Option<String>,
    variables_schema: Option<Value>,
    defaults: Option<Value>,
}

async fn create(state: SharedState, user: AuthUser, path: web::Path<String>, body: web::Json<TypeReq>) -> ApiResult<HttpResponse> {
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &user.id, "admin").await?;
    let id = crate::new_id();
    sqlx::query("INSERT INTO task_types (id, workspace_id, name, icon, title_template, content_template, variables_schema, defaults) VALUES (?,?,?,?,?,?,?,?)")
        .bind(&id).bind(&ws_id).bind(&body.name)
        .bind(body.icon.clone().unwrap_or_else(|| "📋".into()))
        .bind(body.title_template.clone().unwrap_or_else(|| "{{title}}".into()))
        .bind(body.content_template.clone().unwrap_or_default())
        .bind(body.variables_schema.clone().unwrap_or(json!([])).to_string())
        .bind(body.defaults.clone().unwrap_or(json!({})).to_string())
        .execute(&state.db).await?;
    Ok(HttpResponse::Ok().json(load(&state, &id).await?))
}

async fn patch(state: SharedState, user: AuthUser, path: web::Path<String>, body: web::Json<TypeReq>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let t = load(&state, &id).await?;
    require(&state.db, &t.workspace_id, &user.id, "admin").await?;
    sqlx::query("UPDATE task_types SET name=?, icon=?, title_template=?, content_template=?, variables_schema=?, defaults=? WHERE id=?")
        .bind(&body.name)
        .bind(body.icon.clone().unwrap_or(t.icon))
        .bind(body.title_template.clone().unwrap_or(t.title_template))
        .bind(body.content_template.clone().unwrap_or(t.content_template))
        .bind(body.variables_schema.clone().map(|v| v.to_string()).unwrap_or(t.variables_schema))
        .bind(body.defaults.clone().map(|v| v.to_string()).unwrap_or(t.defaults))
        .bind(&id).execute(&state.db).await?;
    Ok(HttpResponse::Ok().json(load(&state, &id).await?))
}

async fn delete(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let t = load(&state, &id).await?;
    require(&state.db, &t.workspace_id, &user.id, "admin").await?;
    sqlx::query("DELETE FROM task_types WHERE id = ?").bind(&id).execute(&state.db).await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

fn render(template: &str, vars: &serde_json::Map<String, Value>) -> String {
    let mut out = template.to_string();
    for (k, v) in vars {
        let s = match v { Value::String(s) => s.clone(), other => other.to_string() };
        out = out.replace(&format!("{{{{{k}}}}}"), &s);
    }
    out
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstantiateReq {
    variables: Option<Value>,
    project_id: Option<String>,
    priority: Option<i64>,
    assignee_agent_id: Option<String>,
    assignee_team_id: Option<String>,
}

/// 模板实例化：填变量 → 一键建卡（FR-04）。
async fn instantiate(state: SharedState, user: AuthUser, path: web::Path<String>, body: web::Json<InstantiateReq>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let t = load(&state, &id).await?;
    require(&state.db, &t.workspace_id, &user.id, "member").await?;
    let mut vars = body.variables.clone().unwrap_or(json!({})).as_object().cloned().unwrap_or_default();
    // 变量默认值
    if let Ok(schema) = serde_json::from_str::<Vec<Value>>(&t.variables_schema) {
        for v in schema {
            if let (Some(name), Some(default)) = (v.get("name").and_then(|n| n.as_str()), v.get("default")) {
                vars.entry(name.to_string()).or_insert_with(|| default.clone());
            }
        }
    }
    let defaults: Value = serde_json::from_str(&t.defaults).unwrap_or(json!({}));
    let title = render(&t.title_template, &vars);
    let content = render(&t.content_template, &vars);
    if title.trim().is_empty() { return Err(ApiError::bad_request("标题不能为空")); }
    let board = crate::server::api::boards::board_of_workspace(&state, &t.workspace_id).await?;
    let task = create_task(&state, NewTask {
        board_id: board.id,
        workspace_id: t.workspace_id.clone(),
        type_id: Some(t.id.clone()),
        project_id: body.project_id.clone().or_else(|| defaults.get("projectId").and_then(|v| v.as_str()).map(String::from)),
        title,
        content,
        priority: body.priority.or_else(|| defaults.get("priority").and_then(|v| v.as_i64())).unwrap_or(0),
        tags: vec![],
        assignee_agent_id: body.assignee_agent_id.clone(),
        assignee_team_id: body.assignee_team_id.clone(),
        created_by: Some(user.id.clone()),
    }).await?;
    Ok(HttpResponse::Ok().json(task))
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/workspaces/{id}/task-types", web::get().to(list))
        .route("/workspaces/{id}/task-types", web::post().to(create))
        .route("/task-types/{id}", web::patch().to(patch))
        .route("/task-types/{id}", web::delete().to(delete))
        .route("/task-types/{id}/instantiate", web::post().to(instantiate));
}