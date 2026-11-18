use crate::server::auth::{require, AuthUser};
use crate::server::error::{ApiError, ApiResult};
use crate::server::models::Agent;
use crate::server::SharedState;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use serde_json::{json, Value};

pub async fn load_agent(state: &SharedState, id: &str) -> ApiResult<Agent> {
    Ok(sqlx::query_as::<_, Agent>("SELECT * FROM agents WHERE id = ?").bind(id).fetch_one(&state.db).await?)
}

async fn agent_json(state: &SharedState, a: &Agent) -> ApiResult<Value> {
    let skill_ids: Vec<String> = sqlx::query_scalar("SELECT skill_id FROM agent_skills WHERE agent_id = ?").bind(&a.id).fetch_all(&state.db).await?;
    let mcp: Vec<(String, String)> = sqlx::query_as("SELECT mcp_server_id, tool_whitelist FROM agent_mcp_servers WHERE agent_id = ?").bind(&a.id).fetch_all(&state.db).await?;
    let mut v = serde_json::to_value(a).unwrap();
    v["skillIds"] = json!(skill_ids);
    v["mcpServers"] = json!(mcp.iter().map(|(id, wl)| json!({ "id": id, "toolWhitelist": serde_json::from_str::<Value>(wl).unwrap_or(json!([])) })).collect::<Vec<_>>());
    Ok(v)
}

async fn list(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &user.id, "member").await?;
    let rows = sqlx::query_as::<_, Agent>("SELECT * FROM agents WHERE workspace_id = ? ORDER BY name").bind(&ws_id).fetch_all(&state.db).await?;
    let mut out = vec![];
    for a in rows { out.push(agent_json(&state, &a).await?); }
    Ok(HttpResponse::Ok().json(out))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentReq {
    name: String,
    cli_entry_id: String,
    model: Option<String>,
    reasoning_effort: Option<String>,
    system_prompt: Option<String>,
    policy: Option<String>,
    gates: Option<Value>,
    max_concurrent: Option<i64>,
    match_tags: Option<Vec<String>>,
    skill_ids: Option<Vec<String>>,
    mcp_servers: Option<Vec<McpGrant>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpGrant { id: String, tool_whitelist: Option<Vec<String>> }

async fn save_grants(state: &SharedState, agent_id: &str, body: &AgentReq) -> ApiResult<()> {
    if let Some(skills) = &body.skill_ids {
        sqlx::query("DELETE FROM agent_skills WHERE agent_id = ?").bind(agent_id).execute(&state.db).await?;
        for s in skills {
            sqlx::query("INSERT OR IGNORE INTO agent_skills (agent_id, skill_id) VALUES (?,?)").bind(agent_id).bind(s).execute(&state.db).await?;
        }
    }
    if let Some(mcps) = &body.mcp_servers {
        sqlx::query("DELETE FROM agent_mcp_servers WHERE agent_id = ?").bind(agent_id).execute(&state.db).await?;
        for m in mcps {
            sqlx::query("INSERT OR IGNORE INTO agent_mcp_servers (agent_id, mcp_server_id, tool_whitelist) VALUES (?,?,?)")
                .bind(agent_id).bind(&m.id)
                .bind(serde_json::to_string(&m.tool_whitelist.clone().unwrap_or_default()).unwrap())
                .execute(&state.db).await?;
        }
    }
    Ok(())
}

async fn create(state: SharedState, user: AuthUser, path: web::Path<String>, body: web::Json<AgentReq>) -> ApiResult<HttpResponse> {
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &user.id, "admin").await?;
    let cli = sqlx::query_scalar::<_, String>("SELECT id FROM cli_entries WHERE id = ?").bind(&body.cli_entry_id).fetch_optional(&state.db).await?;
    if cli.is_none() { return Err(ApiError::bad_request("CLI 条目不存在")); }
    let id = crate::new_id();
    sqlx::query("INSERT INTO agents (id, workspace_id, name, cli_entry_id, model, reasoning_effort, system_prompt, policy, gates, max_concurrent, match_tags) VALUES (?,?,?,?,?,?,?,?,?,?,?)")
        .bind(&id).bind(&ws_id).bind(&body.name).bind(&body.cli_entry_id)
        .bind(body.model.clone().unwrap_or_default())
        .bind(body.reasoning_effort.clone().unwrap_or_else(|| "medium".into()))
        .bind(body.system_prompt.clone().unwrap_or_default())
        .bind(body.policy.clone().unwrap_or_else(|| "write_ask".into()))
        .bind(body.gates.clone().unwrap_or(json!({})).to_string())
        .bind(body.max_concurrent.unwrap_or(1))
        .bind(serde_json::to_string(&body.match_tags.clone().unwrap_or_default()).unwrap())
        .execute(&state.db).await?;
    save_grants(&state, &id, &body).await?;
    Ok(HttpResponse::Ok().json(agent_json(&state, &load_agent(&state, &id).await?).await?))
}

async fn patch(state: SharedState, user: AuthUser, path: web::Path<String>, body: web::Json<AgentReq>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let a = load_agent(&state, &id).await?;
    require(&state.db, &a.workspace_id, &user.id, "admin").await?;
    sqlx::query("UPDATE agents SET name=?, cli_entry_id=?, model=?, reasoning_effort=?, system_prompt=?, policy=?, gates=?, max_concurrent=?, match_tags=? WHERE id=?")
        .bind(&body.name).bind(&body.cli_entry_id)
        .bind(body.model.clone().unwrap_or(a.model))
        .bind(body.reasoning_effort.clone().unwrap_or(a.reasoning_effort))
        .bind(body.system_prompt.clone().unwrap_or(a.system_prompt))
        .bind(body.policy.clone().unwrap_or(a.policy))
        .bind(body.gates.clone().map(|v| v.to_string()).unwrap_or(a.gates))
        .bind(body.max_concurrent.unwrap_or(a.max_concurrent))
        .bind(body.match_tags.clone().map(|t| serde_json::to_string(&t).unwrap()).unwrap_or(a.match_tags))
        .bind(&id).execute(&state.db).await?;
    save_grants(&state, &id, &body).await?;
    Ok(HttpResponse::Ok().json(agent_json(&state, &load_agent(&state, &id).await?).await?))
}

#[derive(Deserialize)]
struct PauseReq { paused: bool }

async fn pause(state: SharedState, user: AuthUser, path: web::Path<String>, body: web::Json<PauseReq>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let a = load_agent(&state, &id).await?;
    require(&state.db, &a.workspace_id, &user.id, "admin").await?;
    sqlx::query("UPDATE agents SET paused = ? WHERE id = ?").bind(body.paused as i64).bind(&id).execute(&state.db).await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

async fn delete(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let a = load_agent(&state, &id).await?;
    require(&state.db, &a.workspace_id, &user.id, "admin").await?;
    sqlx::query("DELETE FROM agents WHERE id = ?").bind(&id).execute(&state.db).await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/workspaces/{id}/agents", web::get().to(list))
        .route("/workspaces/{id}/agents", web::post().to(create))
        .route("/agents/{id}", web::patch().to(patch))
        .route("/agents/{id}", web::delete().to(delete))
        .route("/agents/{id}/pause", web::post().to(pause));
}