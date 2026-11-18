use crate::server::SharedState;
use crate::server::auth::{AuthUser, require, role_of};
use crate::server::error::{ApiError, ApiResult};
use crate::server::events;
use crate::server::models::{Invitation, MemberView, Workspace};
use actix_web::{HttpResponse, web};
use serde::Deserialize;
use serde_json::json;

const DEFAULT_LANES: [(&str, &str); 7] = [
    ("planning", "规划"),
    ("ready", "待办"),
    ("work", "进行中"),
    ("review", "审核中"),
    ("blocked", "已阻塞"),
    ("done", "已完成"),
    ("archived", "已归档"),
];

const DEFAULT_TASK_TYPES: [(&str, &str, &str, &str); 5] = [
    (
        "Bug 修复",
        "🐛",
        "修复: {{title}}",
        "## 现象\n{{problem}}\n\n## 期望\n{{expected}}\n\n请定位根因并修复，补充必要的测试。",
    ),
    (
        "功能开发",
        "✨",
        " feat: {{title}}",
        "## 需求\n{{requirement}}\n\n## 验收标准\n{{acceptance}}",
    ),
    (
        "代码评审",
        "🔍",
        "评审: {{title}}",
        "请评审 {{target}} 的实现，关注正确性、安全性与可维护性，输出评审意见。",
    ),
    (
        "文档撰写",
        "📝",
        "文档: {{title}}",
        "请为 {{target}} 撰写文档，包含 {{outline}}。",
    ),
    (
        "调研",
        "🔬",
        "调研: {{title}}",
        "## 背景\n{{background}}\n\n## 问题\n{{question}}\n\n请输出结论与建议。",
    ),
];

pub async fn create_workspace(
    state: &SharedState,
    name: &str,
    user_id: &str,
) -> ApiResult<Workspace> {
    let ws_id = crate::new_id();
    let mut tx = state.db.begin().await?;
    sqlx::query("INSERT INTO workspaces (id, name, created_by) VALUES (?,?,?)")
        .bind(&ws_id)
        .bind(name)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES (?,?, 'owner')",
    )
    .bind(&ws_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    let board_id = crate::new_id();
    sqlx::query("INSERT INTO boards (id, workspace_id) VALUES (?,?)")
        .bind(&board_id)
        .bind(&ws_id)
        .execute(&mut *tx)
        .await?;
    for (i, (kind, label)) in DEFAULT_LANES.iter().enumerate() {
        sqlx::query("INSERT INTO lanes (id, board_id, kind, name, position) VALUES (?,?,?,?,?)")
            .bind(crate::new_id())
            .bind(&board_id)
            .bind(kind)
            .bind(label)
            .bind(i as i64)
            .execute(&mut *tx)
            .await?;
    }
    for (name, icon, title_t, content_t) in DEFAULT_TASK_TYPES {
        sqlx::query("INSERT INTO task_types (id, workspace_id, name, icon, title_template, content_template) VALUES (?,?,?,?,?,?)")
            .bind(crate::new_id()).bind(&ws_id).bind(name).bind(icon).bind(title_t).bind(content_t)
            .execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(
        sqlx::query_as::<_, Workspace>("SELECT * FROM workspaces WHERE id = ?")
            .bind(&ws_id)
            .fetch_one(&state.db)
            .await?,
    )
}

#[derive(Deserialize)]
struct CreateWs {
    name: String,
}

async fn list(state: SharedState, user: AuthUser) -> ApiResult<HttpResponse> {
    let rows = sqlx::query_as::<_, Workspace>(
        "SELECT w.* FROM workspaces w JOIN workspace_members m ON m.workspace_id = w.id WHERE m.user_id = ? ORDER BY w.created_at",
    ).bind(&user.id).fetch_all(&state.db).await?;
    let mut out = vec![];
    for w in rows {
        let role = role_of(&state.db, &w.id, &user.id)
            .await
            .unwrap_or_default();
        out.push(json!({ "workspace": w, "role": role }));
    }
    Ok(HttpResponse::Ok().json(out))
}

async fn create(
    state: SharedState,
    user: AuthUser,
    body: web::Json<CreateWs>,
) -> ApiResult<HttpResponse> {
    if body.name.trim().is_empty() {
        return Err(ApiError::bad_request("名称不能为空"));
    }
    let ws = create_workspace(&state, body.name.trim(), &user.id).await?;
    Ok(HttpResponse::Ok().json(ws))
}

async fn get(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let role = require(&state.db, &id, &user.id, "member").await?;
    let ws = sqlx::query_as::<_, Workspace>("SELECT * FROM workspaces WHERE id = ?")
        .bind(&id)
        .fetch_one(&state.db)
        .await?;
    Ok(HttpResponse::Ok().json(json!({ "workspace": ws, "role": role })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchWs {
    name: Option<String>,
    settings: Option<serde_json::Value>,
}

async fn patch(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
    body: web::Json<PatchWs>,
) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    require(&state.db, &id, &user.id, "admin").await?;
    if let Some(name) = &body.name {
        sqlx::query("UPDATE workspaces SET name = ? WHERE id = ?")
            .bind(name)
            .bind(&id)
            .execute(&state.db)
            .await?;
    }
    if let Some(settings) = &body.settings {
        sqlx::query("UPDATE workspaces SET settings = ? WHERE id = ?")
            .bind(settings.to_string())
            .bind(&id)
            .execute(&state.db)
            .await?;
    }
    let ws = sqlx::query_as::<_, Workspace>("SELECT * FROM workspaces WHERE id = ?")
        .bind(&id)
        .fetch_one(&state.db)
        .await?;
    Ok(HttpResponse::Ok().json(ws))
}

async fn delete(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    require(&state.db, &id, &user.id, "owner").await?;
    sqlx::query("DELETE FROM workspaces WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

async fn members(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    require(&state.db, &id, &user.id, "member").await?;
    let rows = sqlx::query_as::<_, MemberView>(
        "SELECT m.user_id, u.username, u.display_name, m.role, m.joined_at FROM workspace_members m JOIN users u ON u.id = m.user_id WHERE m.workspace_id = ? ORDER BY m.joined_at",
    ).bind(&id).fetch_all(&state.db).await?;
    Ok(HttpResponse::Ok().json(rows))
}

#[derive(Deserialize)]
struct RoleReq {
    role: String,
}

async fn set_role(
    state: SharedState,
    user: AuthUser,
    path: web::Path<(String, String)>,
    body: web::Json<RoleReq>,
) -> ApiResult<HttpResponse> {
    let (ws_id, target) = path.into_inner();
    let my_role = require(&state.db, &ws_id, &user.id, "admin").await?;
    if !["admin", "member"].contains(&body.role.as_str()) {
        return Err(ApiError::bad_request("角色只能为 admin/member"));
    }
    let target_role = role_of(&state.db, &ws_id, &target)
        .await
        .ok_or_else(|| ApiError::not_found("成员不存在"))?;
    if target_role == "owner" || (my_role != "owner" && target_role == "admin") {
        return Err(ApiError::forbidden("不能变更该成员角色"));
    }
    sqlx::query("UPDATE workspace_members SET role = ? WHERE workspace_id = ? AND user_id = ?")
        .bind(&body.role)
        .bind(&ws_id)
        .bind(&target)
        .execute(&state.db)
        .await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

async fn remove_member(
    state: SharedState,
    user: AuthUser,
    path: web::Path<(String, String)>,
) -> ApiResult<HttpResponse> {
    let (ws_id, target) = path.into_inner();
    require(&state.db, &ws_id, &user.id, "admin").await?;
    let target_role = role_of(&state.db, &ws_id, &target)
        .await
        .ok_or_else(|| ApiError::not_found("成员不存在"))?;
    if target_role == "owner" {
        return Err(ApiError::forbidden("不能移除 owner"));
    }
    sqlx::query("DELETE FROM workspace_members WHERE workspace_id = ? AND user_id = ?")
        .bind(&ws_id)
        .bind(&target)
        .execute(&state.db)
        .await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InviteReq {
    role: Option<String>,
    expires_hours: Option<i64>,
    one_time: Option<bool>,
}

async fn invite(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
    body: web::Json<InviteReq>,
) -> ApiResult<HttpResponse> {
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &user.id, "admin").await?;
    let role = body.role.clone().unwrap_or_else(|| "member".into());
    if !["admin", "member"].contains(&role.as_str()) {
        return Err(ApiError::bad_request("角色只能为 admin/member"));
    }
    let code = hex::encode(rand::random::<[u8; 8]>());
    let expires = body.expires_hours.map(|h| {
        (chrono::Utc::now() + chrono::Duration::hours(h))
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string()
    });
    sqlx::query("INSERT INTO invitations (code, workspace_id, role, expires_at, one_time, created_by) VALUES (?,?,?,?,?,?)")
        .bind(&code).bind(&ws_id).bind(&role).bind(&expires).bind(body.one_time.unwrap_or(true) as i64).bind(&user.id)
        .execute(&state.db).await?;
    let inv = sqlx::query_as::<_, Invitation>("SELECT * FROM invitations WHERE code = ?")
        .bind(&code)
        .fetch_one(&state.db)
        .await?;
    Ok(HttpResponse::Ok().json(inv))
}

async fn invitations(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
) -> ApiResult<HttpResponse> {
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &user.id, "admin").await?;
    let rows = sqlx::query_as::<_, Invitation>(
        "SELECT * FROM invitations WHERE workspace_id = ? ORDER BY created_at DESC",
    )
    .bind(&ws_id)
    .fetch_all(&state.db)
    .await?;
    Ok(HttpResponse::Ok().json(rows))
}

async fn accept(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
) -> ApiResult<HttpResponse> {
    let code = path.into_inner();
    let inv = sqlx::query_as::<_, Invitation>("SELECT * FROM invitations WHERE code = ?")
        .bind(&code)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::not_found("邀请不存在"))?;
    if inv.status != "active" {
        return Err(ApiError::conflict("邀请已失效"));
    }
    if let Some(exp) = &inv.expires_at {
        if exp.as_str() < crate::now().as_str() {
            return Err(ApiError::conflict("邀请已过期"));
        }
    }
    if role_of(&state.db, &inv.workspace_id, &user.id)
        .await
        .is_none()
    {
        sqlx::query("INSERT INTO workspace_members (workspace_id, user_id, role) VALUES (?,?,?)")
            .bind(&inv.workspace_id)
            .bind(&user.id)
            .bind(&inv.role)
            .execute(&state.db)
            .await?;
    }
    if inv.one_time == 1 {
        sqlx::query("UPDATE invitations SET status = 'used' WHERE code = ?")
            .bind(&code)
            .execute(&state.db)
            .await?;
    }
    let ws = sqlx::query_as::<_, Workspace>("SELECT * FROM workspaces WHERE id = ?")
        .bind(&inv.workspace_id)
        .fetch_one(&state.db)
        .await?;
    Ok(HttpResponse::Ok().json(ws))
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/workspaces", web::get().to(list))
        .route("/workspaces", web::post().to(create))
        .route("/workspaces/{id}", web::get().to(get))
        .route("/workspaces/{id}", web::patch().to(patch))
        .route("/workspaces/{id}", web::delete().to(delete))
        .route("/workspaces/{id}/members", web::get().to(members))
        .route(
            "/workspaces/{id}/members/{user_id}/role",
            web::post().to(set_role),
        )
        .route(
            "/workspaces/{id}/members/{user_id}",
            web::delete().to(remove_member),
        )
        .route("/workspaces/{id}/invitations", web::get().to(invitations))
        .route("/workspaces/{id}/invitations", web::post().to(invite))
        .route("/invitations/{code}/accept", web::post().to(accept));
}

/// 供其他模块使用的通知写入。
pub async fn notify(
    state: &SharedState,
    workspace_id: &str,
    kind: &str,
    task_id: Option<&str>,
    message: &str,
) {
    let _ = sqlx::query(
        "INSERT INTO notifications (id, workspace_id, kind, task_id, message) VALUES (?,?,?,?,?)",
    )
    .bind(crate::new_id())
    .bind(workspace_id)
    .bind(kind)
    .bind(task_id)
    .bind(message)
    .execute(&state.db)
    .await;
    if let Some(tid) = task_id {
        let _ = events::append(
            &state.db,
            &state.broadcast,
            tid,
            "notification",
            "system",
            None,
            json!({ "kind": kind, "message": message }),
        )
        .await;
    }
}
