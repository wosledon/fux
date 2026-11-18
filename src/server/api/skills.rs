use crate::server::auth::{require, AuthUser};
use crate::server::error::ApiResult;
use crate::server::models::Skill;
use crate::server::SharedState;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use serde_json::json;

async fn load(state: &SharedState, id: &str) -> ApiResult<Skill> {
    Ok(sqlx::query_as::<_, Skill>("SELECT * FROM skills WHERE id = ?").bind(id).fetch_one(&state.db).await?)
}

async fn list(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &user.id, "member").await?;
    let rows = sqlx::query_as::<_, Skill>("SELECT * FROM skills WHERE workspace_id = ? ORDER BY name").bind(&ws_id).fetch_all(&state.db).await?;
    Ok(HttpResponse::Ok().json(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillReq {
    name: String,
    description: Option<String>,
    source: Option<String>,
    version: Option<String>,
    content: Option<String>,
}

async fn create(state: SharedState, user: AuthUser, path: web::Path<String>, body: web::Json<SkillReq>) -> ApiResult<HttpResponse> {
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &user.id, "admin").await?;
    let id = crate::new_id();
    sqlx::query("INSERT INTO skills (id, workspace_id, name, description, source, version, content) VALUES (?,?,?,?,?,?,?)")
        .bind(&id).bind(&ws_id).bind(&body.name)
        .bind(body.description.clone().unwrap_or_default())
        .bind(body.source.clone().unwrap_or_default())
        .bind(body.version.clone().unwrap_or_else(|| "1.0.0".into()))
        .bind(body.content.clone().unwrap_or_default())
        .execute(&state.db).await?;
    Ok(HttpResponse::Ok().json(load(&state, &id).await?))
}

async fn patch(state: SharedState, user: AuthUser, path: web::Path<String>, body: web::Json<SkillReq>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let s = load(&state, &id).await?;
    require(&state.db, &s.workspace_id, &user.id, "admin").await?;
    sqlx::query("UPDATE skills SET name=?, description=?, source=?, version=?, content=? WHERE id=?")
        .bind(&body.name)
        .bind(body.description.clone().unwrap_or(s.description))
        .bind(body.source.clone().unwrap_or(s.source))
        .bind(body.version.clone().unwrap_or(s.version))
        .bind(body.content.clone().unwrap_or(s.content))
        .bind(&id).execute(&state.db).await?;
    Ok(HttpResponse::Ok().json(load(&state, &id).await?))
}

async fn delete(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let s = load(&state, &id).await?;
    require(&state.db, &s.workspace_id, &user.id, "admin").await?;
    sqlx::query("DELETE FROM skills WHERE id = ?").bind(&id).execute(&state.db).await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/workspaces/{id}/skills", web::get().to(list))
        .route("/workspaces/{id}/skills", web::post().to(create))
        .route("/skills/{id}", web::patch().to(patch))
        .route("/skills/{id}", web::delete().to(delete));
}