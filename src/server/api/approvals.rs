use crate::server::auth::{require, AuthUser};
use crate::server::error::{ApiError, ApiResult};
use crate::server::events;
use crate::server::models::{Approval, Run};
use crate::server::SharedState;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListQuery { task_id: Option<String>, status: Option<String> }

async fn list(state: SharedState, user: AuthUser, query: web::Query<ListQuery>) -> ApiResult<HttpResponse> {
    let rows = match (&query.task_id, &query.status) {
        (Some(t), Some(s)) => sqlx::query_as::<_, Approval>("SELECT * FROM approvals WHERE task_id = ? AND status = ? ORDER BY created_at DESC").bind(t).bind(s).fetch_all(&state.db).await?,
        (Some(t), None) => sqlx::query_as::<_, Approval>("SELECT * FROM approvals WHERE task_id = ? ORDER BY created_at DESC").bind(t).fetch_all(&state.db).await?,
        (None, Some(s)) => sqlx::query_as::<_, Approval>("SELECT * FROM approvals WHERE status = ? ORDER BY created_at DESC LIMIT 100").bind(s).fetch_all(&state.db).await?,
        _ => sqlx::query_as::<_, Approval>("SELECT * FROM approvals ORDER BY created_at DESC LIMIT 100").fetch_all(&state.db).await?,
    };
    // 权限过滤：仅保留用户所在工作区的审批
    let mut out = vec![];
    for a in rows {
        let ws: Option<String> = sqlx::query_scalar("SELECT workspace_id FROM tasks WHERE id = ?").bind(&a.task_id).fetch_optional(&state.db).await?;
        if let Some(ws) = ws {
            if require(&state.db, &ws, &user.id, "member").await.is_ok() { out.push(a); }
        }
    }
    Ok(HttpResponse::Ok().json(out))
}

#[derive(Deserialize)]
struct DecideReq { decision: String }

async fn decide(state: SharedState, user: AuthUser, path: web::Path<String>, body: web::Json<DecideReq>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let a = sqlx::query_as::<_, Approval>("SELECT * FROM approvals WHERE id = ?").bind(&id).fetch_one(&state.db).await?;
    let ws: String = sqlx::query_scalar("SELECT workspace_id FROM tasks WHERE id = ?").bind(&a.task_id).fetch_one(&state.db).await?;
    require(&state.db, &ws, &user.id, "member").await?;
    if a.status != "pending" { return Err(ApiError::conflict("该审批已处理")); }
    if !["approve", "reject"].contains(&body.decision.as_str()) {
        return Err(ApiError::bad_request("decision 只能为 approve/reject"));
    }
    let status = if body.decision == "approve" { "approved" } else { "rejected" };
    sqlx::query("UPDATE approvals SET status = ?, decided_by = ?, decided_at = ? WHERE id = ?")
        .bind(status).bind(&user.id).bind(crate::now()).bind(&id).execute(&state.db).await?;
    events::append(&state.db, &state.broadcast, &a.task_id, "permission.resolved",
        &format!("user:{}", user.id), Some(&a.run_id),
        json!({ "approvalId": id, "decision": body.decision })).await?;
    // 远程节点：转发决策；本地运行器轮询 DB 自取
    let run = sqlx::query_as::<_, Run>("SELECT * FROM runs WHERE id = ?").bind(&a.run_id).fetch_optional(&state.db).await?;
    if let Some(run) = run {
        if run.node_id != "local" {
            let decision = if body.decision == "approve" { "allow_once" } else { "reject" };
            let _ = state.nodes.send(&run.node_id, crate::proto::ServerMsg::Permission {
                run_id: run.id.clone(), request_id: id.clone(), decision: decision.into(),
            }).await;
        }
    }
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/approvals", web::get().to(list))
        .route("/approvals/{id}/decide", web::post().to(decide));
}