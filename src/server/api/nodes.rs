use crate::server::auth::AuthUser;
use crate::server::error::{ApiError, ApiResult};
use crate::server::models::Node;
use crate::server::SharedState;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use serde_json::json;

/// 实例级管理员：至少在一个工作区担任 admin/owner。
async fn require_instance_admin(state: &SharedState, user_id: &str) -> ApiResult<()> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workspace_members WHERE user_id = ? AND role IN ('owner','admin')")
        .bind(user_id).fetch_one(&state.db).await?;
    if n == 0 { return Err(ApiError::forbidden("需要实例管理员权限")); }
    Ok(())
}

async fn list(state: SharedState, user: AuthUser) -> ApiResult<HttpResponse> {
    require_instance_admin(&state, &user.id).await?;
    let rows = sqlx::query_as::<_, Node>("SELECT * FROM nodes ORDER BY created_at").fetch_all(&state.db).await?;
    Ok(HttpResponse::Ok().json(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairCodeReq { expires_hours: Option<i64> }

async fn pairing_code(state: SharedState, user: AuthUser, body: web::Json<PairCodeReq>) -> ApiResult<HttpResponse> {
    require_instance_admin(&state, &user.id).await?;
    let code = hex::encode(rand::random::<[u8; 6]>());
    let expires = body.expires_hours.map(|h| (chrono::Utc::now() + chrono::Duration::hours(h)).format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string());
    sqlx::query("INSERT INTO pairing_tokens (code, expires_at, created_by) VALUES (?,?,?)")
        .bind(&code).bind(&expires).bind(&user.id).execute(&state.db).await?;
    Ok(HttpResponse::Ok().json(json!({
        "code": code,
        "joinCommand": format!("fux-runtime join {} --code {code}", state.api_base),
    })))
}

/// runtime 配对（无需登录，凭一次性配对码）。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairReq {
    pub code: String,
    pub name: String,
    pub capabilities: serde_json::Value,
}

async fn pair(state: SharedState, body: web::Json<PairReq>) -> ApiResult<HttpResponse> {
    let row = sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
        "SELECT code, expires_at, used_by_node FROM pairing_tokens WHERE code = ?",
    ).bind(&body.code).fetch_optional(&state.db).await?
        .ok_or_else(|| ApiError::not_found("配对码不存在"))?;
    if row.2.is_some() { return Err(ApiError::conflict("配对码已使用")); }
    if let Some(exp) = row.1 {
        if exp.as_str() < crate::now().as_str() { return Err(ApiError::conflict("配对码已过期")); }
    }
    let node_id = crate::new_id();
    let token = hex::encode(rand::random::<[u8; 24]>());
    sqlx::query("INSERT INTO nodes (id, name, mode, token, approved, conn_status, capabilities) VALUES (?,?, 'runtime', ?, 0, 'offline', ?)")
        .bind(&node_id).bind(&body.name).bind(&token).bind(body.capabilities.to_string())
        .execute(&state.db).await?;
    sqlx::query("UPDATE pairing_tokens SET used_by_node = ? WHERE code = ?").bind(&node_id).bind(&body.code).execute(&state.db).await?;
    Ok(HttpResponse::Ok().json(json!({ "nodeId": node_id, "token": token })))
}

async fn approve(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    require_instance_admin(&state, &user.id).await?;
    let id = path.into_inner();
    sqlx::query("UPDATE nodes SET approved = 1, approved_by = ? WHERE id = ?").bind(&user.id).bind(&id).execute(&state.db).await?;
    let _ = state.nodes.send(&id, crate::proto::ServerMsg::Approval { approved: true }).await;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

async fn delete(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    require_instance_admin(&state, &user.id).await?;
    let id = path.into_inner();
    if id == "local" { return Err(ApiError::forbidden("本地节点不可删除")); }
    sqlx::query("DELETE FROM nodes WHERE id = ?").bind(&id).execute(&state.db).await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/nodes", web::get().to(list))
        .route("/nodes/pairing-codes", web::post().to(pairing_code))
        .route("/nodes/pair", web::post().to(pair))
        .route("/nodes/{id}/approve", web::post().to(approve))
        .route("/nodes/{id}", web::delete().to(delete))
        .route("/node-channel", web::get().to(crate::server::nodes_ws::channel));
}