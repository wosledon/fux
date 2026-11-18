use crate::server::auth::{require, AuthUser};
use crate::server::error::{ApiError, ApiResult};
use crate::server::models::Project;
use crate::server::SharedState;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectReq {
    pub name: String,
    pub kind: String,
    pub remote_url: Option<String>,
    pub local_path: Option<String>,
    pub default_branch: Option<String>,
}

pub async fn load_project(state: &SharedState, id: &str) -> ApiResult<Project> {
    Ok(sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = ?").bind(id).fetch_one(&state.db).await?)
}

async fn list(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &user.id, "member").await?;
    let rows = sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE workspace_id = ? ORDER BY created_at").bind(&ws_id).fetch_all(&state.db).await?;
    Ok(HttpResponse::Ok().json(rows))
}

async fn create(state: SharedState, user: AuthUser, path: web::Path<String>, body: web::Json<ProjectReq>) -> ApiResult<HttpResponse> {
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &user.id, "admin").await?;
    if !["git", "local"].contains(&body.kind.as_str()) {
        return Err(ApiError::bad_request("kind 只能为 git/local"));
    }
    if body.kind == "git" && body.remote_url.as_deref().unwrap_or("").is_empty() {
        return Err(ApiError::bad_request("git 项目需要 remoteUrl"));
    }
    if body.kind == "local" && body.local_path.as_deref().unwrap_or("").is_empty() {
        return Err(ApiError::bad_request("本地项目需要 localPath"));
    }
    let id = crate::new_id();
    sqlx::query("INSERT INTO projects (id, workspace_id, name, kind, remote_url, local_path, default_branch) VALUES (?,?,?,?,?,?,?)")
        .bind(&id).bind(&ws_id).bind(&body.name).bind(&body.kind)
        .bind(&body.remote_url).bind(&body.local_path)
        .bind(body.default_branch.clone().unwrap_or_else(|| "main".into()))
        .execute(&state.db).await?;
    Ok(HttpResponse::Ok().json(load_project(&state, &id).await?))
}

/// 连接测试：git ls-remote / 本地路径检查（凭据用户自理）。
async fn test_connection(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let p = load_project(&state, &id).await?;
    require(&state.db, &p.workspace_id, &user.id, "member").await?;
    if p.kind == "git" {
        let url = p.remote_url.clone().unwrap_or_default();
        let out = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            tokio::process::Command::new("git").args(["ls-remote", "--symref", &url, "HEAD"]).output(),
        ).await;
        match out {
            Ok(Ok(o)) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout);
                let branch = text.lines()
                    .find(|l| l.starts_with("ref:"))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|r| r.rsplit('/').next())
                    .unwrap_or("main").to_string();
                Ok(HttpResponse::Ok().json(json!({ "ok": true, "defaultBranch": branch })))
            }
            Ok(Ok(o)) => Ok(HttpResponse::Ok().json(json!({ "ok": false, "message": format!("git 认证/网络失败: {}", String::from_utf8_lossy(&o.stderr).trim()) }))),
            Ok(Err(e)) => Ok(HttpResponse::Ok().json(json!({ "ok": false, "message": format!("无法执行 git: {e}") }))),
            Err(_) => Ok(HttpResponse::Ok().json(json!({ "ok": false, "message": "连接超时（30s）" }))),
        }
    } else {
        let path_str = p.local_path.clone().unwrap_or_default();
        let path = std::path::Path::new(&path_str);
        if !path.exists() {
            return Ok(HttpResponse::Ok().json(json!({ "ok": false, "message": "路径不存在" })));
        }
        let is_repo = crate::git::is_repo(path).await;
        Ok(HttpResponse::Ok().json(json!({ "ok": true, "isGitRepo": is_repo })))
    }
}

async fn delete(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let p = load_project(&state, &id).await?;
    require(&state.db, &p.workspace_id, &user.id, "admin").await?;
    sqlx::query("DELETE FROM projects WHERE id = ?").bind(&id).execute(&state.db).await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/workspaces/{id}/projects", web::get().to(list))
        .route("/workspaces/{id}/projects", web::post().to(create))
        .route("/projects/{id}/test-connection", web::post().to(test_connection))
        .route("/projects/{id}", web::delete().to(delete));
}