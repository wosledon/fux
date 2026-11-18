use crate::server::auth::{require, AuthUser};
use crate::server::error::ApiResult;
use crate::server::models::Notification;
use crate::server::SharedState;
use actix_web::{web, HttpResponse};
use serde_json::json;

async fn list(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &user.id, "member").await?;
    let rows = sqlx::query_as::<_, Notification>("SELECT * FROM notifications WHERE workspace_id = ? ORDER BY created_at DESC LIMIT 100").bind(&ws_id).fetch_all(&state.db).await?;
    let out: Vec<_> = rows.into_iter().map(|n| {
        let read_by: Vec<String> = serde_json::from_str(&n.read_by).unwrap_or_default();
        json!({ "notification": n, "read": read_by.contains(&user.id) })
    }).collect();
    Ok(HttpResponse::Ok().json(out))
}

async fn mark_read(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let n = sqlx::query_as::<_, Notification>("SELECT * FROM notifications WHERE id = ?").bind(&id).fetch_one(&state.db).await?;
    require(&state.db, &n.workspace_id, &user.id, "member").await?;
    let mut read_by: Vec<String> = serde_json::from_str(&n.read_by).unwrap_or_default();
    if !read_by.contains(&user.id) { read_by.push(user.id.clone()); }
    sqlx::query("UPDATE notifications SET read_by = ? WHERE id = ?").bind(serde_json::to_string(&read_by).unwrap()).bind(&id).execute(&state.db).await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/workspaces/{id}/notifications", web::get().to(list))
        .route("/notifications/{id}/read", web::post().to(mark_read));
}