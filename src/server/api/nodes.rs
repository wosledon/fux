//! 执行节点配对、审批、删除与运行状态总览。
use crate::server::SharedState;
use crate::server::auth::AuthUser;
use crate::server::error::{ApiError, ApiResult};
use crate::server::models::Node;
use actix_web::{HttpResponse, web};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::FromRow;

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
struct NodeRunView {
    run_id: String,
    task_id: String,
    task_title: String,
    stage_key: Option<String>,
    started_at: String,
}

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
struct NodeErrorView {
    run_id: String,
    task_id: String,
    task_title: String,
    stop_reason: Option<String>,
    ended_at: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeView {
    #[serde(flatten)]
    node: Node,
    running_runs: Vec<NodeRunView>,
    recent_error: Option<NodeErrorView>,
    cli_auth_required: bool,
    cli_auth_message: Option<String>,
}

/// 实例级管理员：至少在一个工作区担任 admin/owner。
async fn require_instance_admin(state: &SharedState, user_id: &str) -> ApiResult<()> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM workspace_members WHERE user_id = ? AND role IN ('owner','admin')",
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;
    if n == 0 {
        return Err(ApiError::forbidden("需要实例管理员权限"));
    }
    Ok(())
}

async fn list(state: SharedState, user: AuthUser) -> ApiResult<HttpResponse> {
    require_instance_admin(&state, &user.id).await?;
    let rows = sqlx::query_as::<_, Node>("SELECT * FROM nodes ORDER BY created_at")
        .fetch_all(&state.db)
        .await?;
    let mut views = Vec::with_capacity(rows.len());
    for node in rows {
        views.push(node_view(&state, node).await?);
    }
    Ok(HttpResponse::Ok().json(views))
}

/// 聚合节点当前 Run、最近失败与尚未恢复的 CLI 认证错误。
async fn node_view(state: &SharedState, node: Node) -> ApiResult<NodeView> {
    let running_runs = sqlx::query_as::<_, NodeRunView>(
        "SELECT r.id AS run_id, r.task_id, t.title AS task_title, r.stage_key, r.started_at FROM runs r JOIN tasks t ON t.id = r.task_id WHERE r.node_id = ? AND r.status = 'running' ORDER BY r.started_at DESC",
    )
    .bind(&node.id)
    .fetch_all(&state.db)
    .await?;
    let recent_error = sqlx::query_as::<_, NodeErrorView>(
        "SELECT r.id AS run_id, r.task_id, t.title AS task_title, r.stop_reason, r.ended_at FROM runs r JOIN tasks t ON t.id = r.task_id WHERE r.node_id = ? AND r.status = 'failed' ORDER BY r.ended_at DESC LIMIT 1",
    )
    .bind(&node.id)
    .fetch_optional(&state.db)
    .await?;
    let auth_event = sqlx::query_as::<_, (String, String)>(
        "SELECT e.created_at, e.payload FROM task_events e JOIN runs r ON r.id = e.run_id WHERE r.node_id = ? AND e.kind = 'auth_required' ORDER BY e.created_at DESC LIMIT 1",
    )
    .bind(&node.id)
    .fetch_optional(&state.db)
    .await?;
    let last_success = sqlx::query_scalar::<_, Option<String>>(
        "SELECT MAX(ended_at) FROM runs WHERE node_id = ? AND status = 'finished'",
    )
    .bind(&node.id)
    .fetch_one(&state.db)
    .await?;
    let cli_auth_required = auth_event.as_ref().is_some_and(|(at, _)| {
        last_success
            .as_deref()
            .map(|success| at.as_str() > success)
            .unwrap_or(true)
    });
    let cli_auth_message = if cli_auth_required {
        auth_event.and_then(|(_, payload)| {
            serde_json::from_str::<serde_json::Value>(&payload)
                .ok()
                .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
        })
    } else {
        None
    };
    Ok(NodeView {
        node,
        running_runs,
        recent_error,
        cli_auth_required,
        cli_auth_message,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairCodeReq {
    expires_hours: Option<i64>,
}

async fn pairing_code(
    state: SharedState,
    user: AuthUser,
    body: web::Json<PairCodeReq>,
) -> ApiResult<HttpResponse> {
    require_instance_admin(&state, &user.id).await?;
    let code = hex::encode(rand::random::<[u8; 6]>());
    let expires = body.expires_hours.map(|h| {
        (chrono::Utc::now() + chrono::Duration::hours(h))
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string()
    });
    sqlx::query("INSERT INTO pairing_tokens (code, expires_at, created_by) VALUES (?,?,?)")
        .bind(&code)
        .bind(&expires)
        .bind(&user.id)
        .execute(&state.db)
        .await?;
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
    )
    .bind(&body.code)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| ApiError::not_found("配对码不存在"))?;
    if row.2.is_some() {
        return Err(ApiError::conflict("配对码已使用"));
    }
    if let Some(exp) = row.1 {
        if exp.as_str() < crate::now().as_str() {
            return Err(ApiError::conflict("配对码已过期"));
        }
    }
    let node_id = crate::new_id();
    let token = hex::encode(rand::random::<[u8; 24]>());
    sqlx::query("INSERT INTO nodes (id, name, mode, token, approved, conn_status, capabilities) VALUES (?,?, 'runtime', ?, 0, 'offline', ?)")
        .bind(&node_id).bind(&body.name).bind(&token).bind(body.capabilities.to_string())
        .execute(&state.db).await?;
    sqlx::query("UPDATE pairing_tokens SET used_by_node = ? WHERE code = ?")
        .bind(&node_id)
        .bind(&body.code)
        .execute(&state.db)
        .await?;
    Ok(HttpResponse::Ok().json(json!({ "nodeId": node_id, "token": token })))
}

async fn approve(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
) -> ApiResult<HttpResponse> {
    require_instance_admin(&state, &user.id).await?;
    let id = path.into_inner();
    let result = sqlx::query("UPDATE nodes SET approved = 1, approved_by = ? WHERE id = ?")
        .bind(&user.id)
        .bind(&id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found("节点不存在"));
    }
    let _ = state
        .nodes
        .send(&id, crate::proto::ServerMsg::Approval { approved: true })
        .await;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

async fn delete(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
) -> ApiResult<HttpResponse> {
    require_instance_admin(&state, &user.id).await?;
    let id = path.into_inner();
    if id == "local" {
        return Err(ApiError::forbidden("本地节点不可删除"));
    }
    let _ = state
        .nodes
        .send(&id, crate::proto::ServerMsg::Approval { approved: false })
        .await;
    let result = sqlx::query("DELETE FROM nodes WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found("节点不存在"));
    }
    state.nodes.unregister(&id).await;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/nodes", web::get().to(list))
        .route("/nodes/pairing-codes", web::post().to(pairing_code))
        .route("/nodes/pair", web::post().to(pair))
        .route("/nodes/{id}/approve", web::post().to(approve))
        .route("/nodes/{id}", web::delete().to(delete))
        .route(
            "/node-channel",
            web::get().to(crate::server::nodes_ws::channel),
        );
}
