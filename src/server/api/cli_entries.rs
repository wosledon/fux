//! ACP CLI 条目管理、本机可用性探测与模型目录读取。
use crate::acp::AcpClient;
use crate::server::SharedState;
use crate::server::auth::{AuthUser, require};
use crate::server::error::{ApiError, ApiResult};
use crate::server::models::CliEntry;
use crate::server::secrets;
use actix_web::{HttpResponse, web};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntryQuery {
    workspace_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DetectQuery {
    workspace_id: String,
}

async fn list(
    state: SharedState,
    user: AuthUser,
    query: web::Query<EntryQuery>,
) -> ApiResult<HttpResponse> {
    let rows = match &query.workspace_id {
        Some(ws) => {
            require(&state.db, ws, &user.id, "member").await?;
            sqlx::query_as::<_, CliEntry>("SELECT * FROM cli_entries WHERE workspace_id IS NULL OR workspace_id = ? ORDER BY is_default DESC, name").bind(ws).fetch_all(&state.db).await?
        }
        None => {
            sqlx::query_as::<_, CliEntry>(
                "SELECT * FROM cli_entries WHERE workspace_id IS NULL ORDER BY name",
            )
            .fetch_all(&state.db)
            .await?
        }
    };
    // env 模板为密文，不回传密钥明文
    let rows: Vec<CliEntry> = rows
        .into_iter()
        .map(|mut e| {
            if !e.env_template.is_empty() {
                e.env_template = "<encrypted>".into();
            }
            e
        })
        .collect();
    Ok(HttpResponse::Ok().json(rows))
}

/// 批量探测当前工作区可见的 CLI，供接入页打开时自动扫描。
async fn detect(
    state: SharedState,
    user: AuthUser,
    query: web::Query<DetectQuery>,
) -> ApiResult<HttpResponse> {
    require(&state.db, &query.workspace_id, &user.id, "member").await?;
    let entries = sqlx::query_as::<_, CliEntry>(
        "SELECT * FROM cli_entries WHERE workspace_id IS NULL OR workspace_id = ? ORDER BY is_default DESC, name",
    )
    .bind(&query.workspace_id)
    .fetch_all(&state.db)
    .await?;
    let total = entries.len();
    let mut available_count = 0;
    let mut results = Vec::with_capacity(total);
    for entry in entries {
        let path = which::which(&entry.command)
            .ok()
            .map(|p| p.to_string_lossy().to_string());
        if path.is_some() {
            available_count += 1;
        }
        results.push(json!({
            "entryId": entry.id,
            "available": path.is_some(),
            "path": path,
        }));
    }
    Ok(HttpResponse::Ok().json(json!({
        "results": results,
        "availableCount": available_count,
        "total": total,
        "detectedAt": crate::now(),
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntryReq {
    workspace_id: String,
    name: String,
    command: String,
    args: Option<Vec<String>>,
    env_template: Option<serde_json::Value>,
}

async fn create(
    state: SharedState,
    user: AuthUser,
    body: web::Json<EntryReq>,
) -> ApiResult<HttpResponse> {
    require(&state.db, &body.workspace_id, &user.id, "admin").await?;
    let id = crate::new_id();
    // env 模板可能含 API key：加密后入库（与 MCP env 同一套密钥体系）
    let env_enc = secrets::cli_env_encrypt(&body.env_template.clone().unwrap_or(json!({})))?;
    sqlx::query("INSERT INTO cli_entries (id, workspace_id, name, kind, command, args, env_template, is_default) VALUES (?,?,?,?,?,?,?,0)")
        .bind(&id).bind(&body.workspace_id).bind(&body.name).bind("custom").bind(&body.command)
        .bind(serde_json::to_string(&body.args.clone().unwrap_or_default()).unwrap())
        .bind(&env_enc)
        .execute(&state.db).await?;
    let mut e = sqlx::query_as::<_, CliEntry>("SELECT * FROM cli_entries WHERE id = ?")
        .bind(&id)
        .fetch_one(&state.db)
        .await?;
    // 不回传密钥明文
    if !e.env_template.is_empty() {
        e.env_template = "<encrypted>".into();
    }
    Ok(HttpResponse::Ok().json(e))
}

/// 探测 CLI 是否安装（节点上线前 probe）。
async fn probe(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let e = sqlx::query_as::<_, CliEntry>("SELECT * FROM cli_entries WHERE id = ?")
        .bind(&id)
        .fetch_one(&state.db)
        .await?;
    if let Some(ws) = &e.workspace_id {
        require(&state.db, ws, &user.id, "member").await?;
    }
    let _ = user;
    match which::which(&e.command) {
        Ok(p) => {
            Ok(HttpResponse::Ok().json(json!({ "available": true, "path": p.to_string_lossy() })))
        }
        Err(_) => Ok(HttpResponse::Ok().json(json!({ "available": false }))),
    }
}

async fn delete(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let e = sqlx::query_as::<_, CliEntry>("SELECT * FROM cli_entries WHERE id = ?")
        .bind(&id)
        .fetch_one(&state.db)
        .await?;
    if e.is_default == 1 {
        return Err(ApiError::forbidden("默认条目不可删除"));
    }
    if let Some(ws) = &e.workspace_id {
        require(&state.db, ws, &user.id, "admin").await?;
    }
    sqlx::query("DELETE FROM cli_entries WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

/// 通过 ACP 建立一次临时会话，读取 CLI 声明的模型 / 推理强度目录。
async fn probe_models(
    command: &str,
    args: &[String],
    env: &[(String, String)],
    cwd: &std::path::Path,
) -> anyhow::Result<serde_json::Value> {
    let mut client = AcpClient::spawn(command, args, env, cwd).await?;
    let mut updates = client.take_updates();
    client.initialize().await?;
    let mut session = client.new_session_full(cwd, vec![]).await?;
    // 部分 agent 在 session/new 之后才用 config_option_update 下发完整配置项
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(1200);
    loop {
        match tokio::time::timeout_at(deadline, updates.recv()).await {
            Ok(Some(u)) => {
                if u.pointer("/update/sessionUpdate").and_then(|v| v.as_str())
                    == Some("config_option_update")
                {
                    crate::acp::merge_config_options(&mut session, &u);
                }
            }
            _ => break,
        }
    }
    let catalog = crate::acp::session_catalog(&session);
    client.kill().await;
    Ok(catalog)
}

/// 探测 CLI 的 ACP 模型目录（供智能体编辑时下拉选择）。
async fn models(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let e = sqlx::query_as::<_, CliEntry>("SELECT * FROM cli_entries WHERE id = ?")
        .bind(&id)
        .fetch_one(&state.db)
        .await?;
    if let Some(ws) = &e.workspace_id {
        require(&state.db, ws, &user.id, "member").await?;
    }
    let args: Vec<String> = serde_json::from_str(&e.args).unwrap_or_default();
    let mut env: Vec<(String, String)> = secrets::cli_env_plain(&e.env_template)
        .as_object()
        .map(|m| {
            m.into_iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default();
    crate::acp::tweak_env(&e.kind, &mut env);
    let cwd = std::env::temp_dir().join(format!("fux-models-{}", crate::new_id()));
    tokio::fs::create_dir_all(&cwd).await.ok();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(75),
        probe_models(&e.command, &args, &env, &cwd),
    )
    .await;
    let _ = tokio::fs::remove_dir_all(&cwd).await;
    match result {
        Ok(Ok(catalog)) => {
            Ok(HttpResponse::Ok().json(json!({ "available": true, "catalog": catalog })))
        }
        Ok(Err(err)) => {
            Ok(HttpResponse::Ok().json(json!({ "available": false, "error": format!("{err:#}") })))
        }
        Err(_) => {
            Ok(HttpResponse::Ok().json(json!({ "available": false, "error": "探测超时（75s）" })))
        }
    }
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/cli-entries", web::get().to(list))
        .route("/cli-entries", web::post().to(create))
        .route("/cli-entries/detect", web::get().to(detect))
        .route("/cli-entries/{id}/probe", web::post().to(probe))
        .route("/cli-entries/{id}/models", web::post().to(models))
        .route("/cli-entries/{id}", web::delete().to(delete));
}
