use crate::server::auth::{require, AuthUser};
use crate::server::error::{ApiError, ApiResult};
use crate::server::models::{Team, TeamMember};
use crate::server::SharedState;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use serde_json::json;

async fn load(state: &SharedState, id: &str) -> ApiResult<Team> {
    Ok(sqlx::query_as::<_, Team>("SELECT * FROM agent_teams WHERE id = ?").bind(id).fetch_one(&state.db).await?)
}

pub async fn team_members(state: &SharedState, team_id: &str) -> ApiResult<Vec<TeamMember>> {
    Ok(sqlx::query_as::<_, TeamMember>("SELECT * FROM team_members WHERE team_id = ? ORDER BY position").bind(team_id).fetch_all(&state.db).await?)
}

async fn list(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &user.id, "member").await?;
    let teams = sqlx::query_as::<_, Team>("SELECT * FROM agent_teams WHERE workspace_id = ? ORDER BY name").bind(&ws_id).fetch_all(&state.db).await?;
    let mut out = vec![];
    for t in teams {
        let members = team_members(&state, &t.id).await?;
        out.push(json!({ "team": t, "members": members }));
    }
    Ok(HttpResponse::Ok().json(out))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TeamReq {
    name: String,
    mode: Option<String>,
    members: Vec<MemberReq>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MemberReq { agent_id: String, role: Option<String>, position: i64 }

async fn save_members(state: &SharedState, team_id: &str, members: &[MemberReq]) -> ApiResult<()> {
    sqlx::query("DELETE FROM team_members WHERE team_id = ?").bind(team_id).execute(&state.db).await?;
    for m in members {
        sqlx::query("INSERT INTO team_members (team_id, agent_id, role, position) VALUES (?,?,?,?)")
            .bind(team_id).bind(&m.agent_id).bind(m.role.clone().unwrap_or_else(|| "worker".into())).bind(m.position)
            .execute(&state.db).await?;
    }
    Ok(())
}

async fn create(state: SharedState, user: AuthUser, path: web::Path<String>, body: web::Json<TeamReq>) -> ApiResult<HttpResponse> {
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &user.id, "admin").await?;
    let mode = body.mode.clone().unwrap_or_else(|| "pipeline".into());
    if !["pipeline", "supervisor"].contains(&mode.as_str()) {
        return Err(ApiError::bad_request("mode 只能为 pipeline/supervisor"));
    }
    if body.members.is_empty() { return Err(ApiError::bad_request("团队至少需要一个成员")); }
    let id = crate::new_id();
    sqlx::query("INSERT INTO agent_teams (id, workspace_id, name, mode) VALUES (?,?,?,?)")
        .bind(&id).bind(&ws_id).bind(&body.name).bind(&mode).execute(&state.db).await?;
    save_members(&state, &id, &body.members).await?;
    Ok(HttpResponse::Ok().json(json!({ "team": load(&state, &id).await?, "members": team_members(&state, &id).await? })))
}

async fn patch(state: SharedState, user: AuthUser, path: web::Path<String>, body: web::Json<TeamReq>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let t = load(&state, &id).await?;
    require(&state.db, &t.workspace_id, &user.id, "admin").await?;
    let mode = body.mode.clone().unwrap_or(t.mode);
    sqlx::query("UPDATE agent_teams SET name = ?, mode = ? WHERE id = ?").bind(&body.name).bind(&mode).bind(&id).execute(&state.db).await?;
    save_members(&state, &id, &body.members).await?;
    Ok(HttpResponse::Ok().json(json!({ "team": load(&state, &id).await?, "members": team_members(&state, &id).await? })))
}

async fn delete(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let t = load(&state, &id).await?;
    require(&state.db, &t.workspace_id, &user.id, "admin").await?;
    sqlx::query("DELETE FROM agent_teams WHERE id = ?").bind(&id).execute(&state.db).await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/workspaces/{id}/teams", web::get().to(list))
        .route("/workspaces/{id}/teams", web::post().to(create))
        .route("/teams/{id}", web::patch().to(patch))
        .route("/teams/{id}", web::delete().to(delete));
}