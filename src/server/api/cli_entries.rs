use crate::server::auth::{require, AuthUser};
use crate::server::error::{ApiError, ApiResult};
use crate::server::models::CliEntry;
use crate::server::SharedState;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntryQuery { workspace_id: Option<String> }

async fn list(state: SharedState, user: AuthUser, query: web::Query<EntryQuery>) -> ApiResult<HttpResponse> {
    let rows = match &query.workspace_id {
        Some(ws) => {
            require(&state.db, ws, &user.id, "member").await?;
            sqlx::query_as::<_, CliEntry>("SELECT * FROM cli_entries WHERE workspace_id IS NULL OR workspace_id = ? ORDER BY is_default DESC, name").bind(ws).fetch_all(&state.db).await?
        }
        None => sqlx::query_as::<_, CliEntry>("SELECT * FROM cli_entries WHERE workspace_id IS NULL ORDER BY name").fetch_all(&state.db).await?,
    };
    Ok(HttpResponse::Ok().json(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntryReq {
    workspace_id: String,
    name: String,
    command: String,
    args: Option<Vec<String>>,
    env_template: Option<serde_json::Value>,
}

async fn create(state: SharedState, user: AuthUser, body: web::Json<EntryReq>) -> ApiResult<HttpResponse> {
    require(&state.db, &body.workspace_id, &user.id, "admin").await?;
    let id = crate::new_id();
    sqlx::query("INSERT INTO cli_entries (id, workspace_id, name, kind, command, args, env_template, is_default) VALUES (?,?,?,?,?,?,?,0)")
        .bind(&id).bind(&body.workspace_id).bind(&body.name).bind("custom").bind(&body.command)
        .bind(serde_json::to_string(&body.args.clone().unwrap_or_default()).unwrap())
        .bind(body.env_template.clone().unwrap_or(json!({})).to_string())
        .execute(&state.db).await?;
    let e = sqlx::query_as::<_, CliEntry>("SELECT * FROM cli_entries WHERE id = ?").bind(&id).fetch_one(&state.db).await?;
    Ok(HttpResponse::Ok().json(e))
}

/// 探测 CLI 是否安装（节点上线前 probe）。
async fn probe(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let e = sqlx::query_as::<_, CliEntry>("SELECT * FROM cli_entries WHERE id = ?").bind(&id).fetch_one(&state.db).await?;
    if let Some(ws) = &e.workspace_id { require(&state.db, ws, &user.id, "member").await?; }
    let _ = user;
    match which::which(&e.command) {
        Ok(p) => Ok(HttpResponse::Ok().json(json!({ "available": true, "path": p.to_string_lossy() }))),
        Err(_) => Ok(HttpResponse::Ok().json(json!({ "available": false }))),
    }
}

async fn delete(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let e = sqlx::query_as::<_, CliEntry>("SELECT * FROM cli_entries WHERE id = ?").bind(&id).fetch_one(&state.db).await?;
    if e.is_default == 1 { return Err(ApiError::forbidden("默认条目不可删除")); }
    if let Some(ws) = &e.workspace_id { require(&state.db, ws, &user.id, "admin").await?; }
    sqlx::query("DELETE FROM cli_entries WHERE id = ?").bind(&id).execute(&state.db).await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/cli-entries", web::get().to(list))
        .route("/cli-entries", web::post().to(create))
        .route("/cli-entries/{id}/probe", web::post().to(probe))
        .route("/cli-entries/{id}", web::delete().to(delete));
}