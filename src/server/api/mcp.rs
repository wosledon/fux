use crate::server::auth::{require, AuthUser};
use crate::server::error::{ApiError, ApiResult};
use crate::server::models::McpServer;
use crate::server::secrets;
use crate::server::SharedState;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use serde_json::json;

async fn load(state: &SharedState, id: &str) -> ApiResult<McpServer> {
    Ok(sqlx::query_as::<_, McpServer>("SELECT * FROM mcp_servers WHERE id = ?").bind(id).fetch_one(&state.db).await?)
}

/// env/headers 解密后的 JSON（供运行时透传给 CLI）。
pub fn env_plain(m: &McpServer) -> serde_json::Value {
    if m.env_json.is_empty() { return json!({}); }
    match secrets::decrypt(&m.env_json) {
        Ok(s) => serde_json::from_str(&s).unwrap_or(json!({})),
        Err(_) => json!({}),
    }
}

async fn list(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &user.id, "member").await?;
    let rows = sqlx::query_as::<_, McpServer>("SELECT * FROM mcp_servers WHERE workspace_id = ? ORDER BY name").bind(&ws_id).fetch_all(&state.db).await?;
    // 不回传密钥明文
    let out: Vec<_> = rows.into_iter().map(|mut m| { m.env_json = if m.env_json.is_empty() { "{}".into() } else { "<encrypted>".into() }; m }).collect();
    Ok(HttpResponse::Ok().json(out))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpReq {
    name: String,
    transport: String,
    command: Option<String>,
    args: Option<Vec<String>>,
    url: Option<String>,
    env: Option<serde_json::Value>,
}

async fn create(state: SharedState, user: AuthUser, path: web::Path<String>, body: web::Json<McpReq>) -> ApiResult<HttpResponse> {
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &user.id, "admin").await?;
    if !["stdio", "sse", "http"].contains(&body.transport.as_str()) {
        return Err(ApiError::bad_request("transport 只能为 stdio/sse/http"));
    }
    if body.transport == "stdio" && body.command.as_deref().unwrap_or("").is_empty() {
        return Err(ApiError::bad_request("stdio 需要 command"));
    }
    if body.transport != "stdio" && body.url.as_deref().unwrap_or("").is_empty() {
        return Err(ApiError::bad_request("sse/http 需要 url"));
    }
    let env_enc = match &body.env {
        Some(v) if !v.as_object().map(|o| o.is_empty()).unwrap_or(true) => secrets::encrypt(&v.to_string())?,
        _ => String::new(),
    };
    let id = crate::new_id();
    sqlx::query("INSERT INTO mcp_servers (id, workspace_id, name, transport, command, args, url, env_json) VALUES (?,?,?,?,?,?,?,?)")
        .bind(&id).bind(&ws_id).bind(&body.name).bind(&body.transport)
        .bind(&body.command)
        .bind(serde_json::to_string(&body.args.clone().unwrap_or_default()).unwrap())
        .bind(&body.url).bind(&env_enc)
        .execute(&state.db).await?;
    let mut m = load(&state, &id).await?;
    m.env_json = "<encrypted>".into();
    Ok(HttpResponse::Ok().json(m))
}

async fn probe(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let m = load(&state, &id).await?;
    require(&state.db, &m.workspace_id, &user.id, "member").await?;
    let (ok, msg) = if m.transport == "stdio" {
        match which::which(m.command.clone().unwrap_or_default()) {
            Ok(p) => (true, format!("命令可用: {}", p.display())),
            Err(_) => (false, "命令未安装或不在 PATH".to_string()),
        }
    } else {
        match reqwest::Url::parse(m.url.as_deref().unwrap_or("")) {
            Ok(_) => (true, "URL 格式有效（连通性请在运行时验证）".to_string()),
            Err(e) => (false, format!("URL 无效: {e}")),
        }
    };
    let status = if ok { "ok" } else { "unavailable" };
    sqlx::query("UPDATE mcp_servers SET status = ? WHERE id = ?").bind(status).bind(&id).execute(&state.db).await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": ok, "message": msg, "status": status })))
}

async fn delete(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let m = load(&state, &id).await?;
    require(&state.db, &m.workspace_id, &user.id, "admin").await?;
    sqlx::query("DELETE FROM mcp_servers WHERE id = ?").bind(&id).execute(&state.db).await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/workspaces/{id}/mcp-servers", web::get().to(list))
        .route("/workspaces/{id}/mcp-servers", web::post().to(create))
        .route("/mcp-servers/{id}/probe", web::post().to(probe))
        .route("/mcp-servers/{id}", web::delete().to(delete));
}