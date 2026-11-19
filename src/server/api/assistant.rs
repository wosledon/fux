//! AI 工作助理（FR-03）：多会话对话，绑定 CLI 的智能体 + 内置 workspace MCP，SSE 流式回复。
use crate::acp::AcpClient;
use crate::server::SharedState;
use crate::server::auth::{AuthUser, issue_token, require};
use crate::server::error::{ApiError, ApiResult};
use crate::server::models::{Agent, CliEntry, Workspace};
use actix_web::{HttpResponse, web};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

/// 旧版本没有会话概念的消息统一归入一个「历史对话」。
async fn backfill_legacy(state: &SharedState, ws_id: &str) -> ApiResult<()> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM assistant_messages WHERE workspace_id = ? AND session_id IS NULL",
    )
    .bind(ws_id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);
    if n == 0 {
        return Ok(());
    }
    let sid = crate::new_id();
    sqlx::query(
        "INSERT INTO assistant_sessions (id, workspace_id, title) VALUES (?,?, '历史对话')",
    )
    .bind(&sid)
    .bind(ws_id)
    .execute(&state.db)
    .await?;
    sqlx::query(
        "UPDATE assistant_messages SET session_id = ? WHERE workspace_id = ? AND session_id IS NULL",
    )
    .bind(&sid)
    .bind(ws_id)
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn list_sessions(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
) -> ApiResult<HttpResponse> {
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &user.id, "member").await?;
    backfill_legacy(&state, &ws_id).await?;
    let rows = sqlx::query_as::<_, (String, String, String, String, i64, Option<String>)>(
        "SELECT s.id, s.title, s.created_at, s.updated_at,
                (SELECT COUNT(*) FROM assistant_messages m WHERE m.session_id = s.id),
                (SELECT content FROM assistant_messages m WHERE m.session_id = s.id ORDER BY m.id DESC LIMIT 1)
         FROM assistant_sessions s WHERE s.workspace_id = ? ORDER BY s.updated_at DESC",
    )
    .bind(&ws_id)
    .fetch_all(&state.db)
    .await?;
    let out: Vec<Value> = rows
        .into_iter()
        .map(|(id, title, created_at, updated_at, count, preview)| {
            json!({
                "id": id, "title": title, "createdAt": created_at, "updatedAt": updated_at,
                "messageCount": count,
                "preview": preview.map(|p| p.chars().take(60).collect::<String>()),
            })
        })
        .collect();
    Ok(HttpResponse::Ok().json(out))
}

async fn session_messages(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
) -> ApiResult<HttpResponse> {
    let sid = path.into_inner();
    let ws_id: String =
        sqlx::query_scalar("SELECT workspace_id FROM assistant_sessions WHERE id = ?")
            .bind(&sid)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(|| ApiError::not_found("会话不存在"))?;
    require(&state.db, &ws_id, &user.id, "member").await?;
    let rows = sqlx::query_as::<_, (i64, String, String, String)>(
        "SELECT id, role, content, created_at FROM assistant_messages WHERE session_id = ? ORDER BY id",
    )
    .bind(&sid)
    .fetch_all(&state.db)
    .await?;
    let out: Vec<Value> = rows
        .into_iter()
        .map(|(id, role, content, created_at)| {
            json!({ "id": id, "role": role, "content": content, "createdAt": created_at })
        })
        .collect();
    Ok(HttpResponse::Ok().json(out))
}

#[derive(Deserialize)]
struct RenameReq {
    title: String,
}

async fn rename_session(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
    body: web::Json<RenameReq>,
) -> ApiResult<HttpResponse> {
    let sid = path.into_inner();
    let ws_id: String =
        sqlx::query_scalar("SELECT workspace_id FROM assistant_sessions WHERE id = ?")
            .bind(&sid)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(|| ApiError::not_found("会话不存在"))?;
    require(&state.db, &ws_id, &user.id, "member").await?;
    let title = body.title.trim();
    if title.is_empty() {
        return Err(ApiError::bad_request("标题不能为空"));
    }
    sqlx::query("UPDATE assistant_sessions SET title = ?, updated_at = ? WHERE id = ?")
        .bind(title.chars().take(60).collect::<String>())
        .bind(crate::now())
        .bind(&sid)
        .execute(&state.db)
        .await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

async fn delete_session(
    state: SharedState,
    user: AuthUser,
    path: web::Path<String>,
) -> ApiResult<HttpResponse> {
    let sid = path.into_inner();
    let ws_id: String =
        sqlx::query_scalar("SELECT workspace_id FROM assistant_sessions WHERE id = ?")
            .bind(&sid)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(|| ApiError::not_found("会话不存在"))?;
    require(&state.db, &ws_id, &user.id, "member").await?;
    sqlx::query("DELETE FROM assistant_messages WHERE session_id = ?")
        .bind(&sid)
        .execute(&state.db)
        .await?;
    sqlx::query("DELETE FROM assistant_sessions WHERE id = ?")
        .bind(&sid)
        .execute(&state.db)
        .await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatReq {
    workspace_id: String,
    message: String,
    session_id: Option<String>,
}

fn session_title(message: &str) -> String {
    let one_line = message.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut t: String = one_line.chars().take(24).collect();
    if one_line.chars().count() > 24 {
        t.push('…');
    }
    if t.is_empty() { "新对话".into() } else { t }
}

/// 工作区上下文：统计 + 重点任务 + 项目/智能体，供助理回答与调用 MCP 时参考。
async fn build_context(state: &SharedState, ws: &Workspace) -> String {
    let stats = sqlx::query_as::<_, (String, i64)>(
        "SELECT status, COUNT(*) FROM tasks WHERE workspace_id = ? GROUP BY status",
    )
    .bind(&ws.id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    let stat_map: BTreeMap<String, i64> = stats.into_iter().collect();

    let tasks = sqlx::query_as::<_, (String, String, String, i64)>(
        "SELECT title, status, COALESCE(project_id, ''), priority FROM tasks
         WHERE workspace_id = ? AND status NOT IN ('archived','cancelled')
         ORDER BY CASE status WHEN 'blocked' THEN 0 WHEN 'review' THEN 1 WHEN 'in_progress' THEN 2 ELSE 3 END,
                  priority DESC, updated_at DESC LIMIT 30",
    )
    .bind(&ws.id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let projects = sqlx::query_as::<_, (String, String)>(
        "SELECT name, kind FROM projects WHERE workspace_id = ? ORDER BY name",
    )
    .bind(&ws.id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let agents = sqlx::query_as::<_, (String, String)>(
        "SELECT name, model FROM agents WHERE workspace_id = ? ORDER BY name",
    )
    .bind(&ws.id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let mut ctx = format!(
        "当前时间: {}\n工作区: {}\n任务统计: {:?}\n",
        crate::now(),
        ws.name,
        stat_map
    );
    if !projects.is_empty() {
        ctx.push_str(&format!(
            "项目: {}\n",
            projects
                .iter()
                .map(|(n, k)| format!("{n}({k})"))
                .collect::<Vec<_>>()
                .join("、")
        ));
    }
    if !agents.is_empty() {
        ctx.push_str(&format!(
            "智能体: {}\n",
            agents
                .iter()
                .map(|(n, m)| if m.is_empty() {
                    n.clone()
                } else {
                    format!("{n}[{m}]")
                })
                .collect::<Vec<_>>()
                .join("、")
        ));
    }
    ctx.push_str("任务（优先展示阻塞/审核/进行中）:\n");
    for (title, status, project_id, priority) in tasks {
        let proj = if project_id.is_empty() {
            String::new()
        } else {
            format!(" project={project_id}")
        };
        ctx.push_str(&format!("- [{status}] {title} (P{priority}{proj})\n"));
    }
    ctx
}

async fn chat(
    state: SharedState,
    user: AuthUser,
    body: web::Json<ChatReq>,
) -> ApiResult<HttpResponse> {
    let ws_id = body.workspace_id.clone();
    require(&state.db, &ws_id, &user.id, "member").await?;
    let ws = sqlx::query_as::<_, Workspace>("SELECT * FROM workspaces WHERE id = ?")
        .bind(&ws_id)
        .fetch_one(&state.db)
        .await?;
    let message = body.message.trim().to_string();
    if message.is_empty() {
        return Err(ApiError::bad_request("消息不能为空"));
    }

    // 助理智能体：工作区设置优先，否则第一个智能体
    let settings: Value = serde_json::from_str(&ws.settings).unwrap_or(json!({}));
    let agent = match settings.get("assistantAgentId").and_then(|v| v.as_str()) {
        Some(id) => {
            sqlx::query_as::<_, Agent>("SELECT * FROM agents WHERE id = ?")
                .bind(id)
                .fetch_optional(&state.db)
                .await?
        }
        None => {
            sqlx::query_as::<_, Agent>(
                "SELECT * FROM agents WHERE workspace_id = ? ORDER BY name LIMIT 1",
            )
            .bind(&ws_id)
            .fetch_optional(&state.db)
            .await?
        }
    };
    let agent =
        agent.ok_or_else(|| ApiError::bad_request("请先创建智能体（绑定 CLI）作为工作助理"))?;
    let cli = sqlx::query_as::<_, CliEntry>("SELECT * FROM cli_entries WHERE id = ?")
        .bind(&agent.cli_entry_id)
        .fetch_one(&state.db)
        .await?;

    // 会话：未指定则创建，标题取首条消息
    let (session_id, title) = match &body.session_id {
        Some(sid) => {
            let row: Option<(String, String)> =
                sqlx::query_as("SELECT workspace_id, title FROM assistant_sessions WHERE id = ?")
                    .bind(sid)
                    .fetch_optional(&state.db)
                    .await?;
            let (owner, title) = row.ok_or_else(|| ApiError::not_found("会话不存在"))?;
            if owner != ws_id {
                return Err(ApiError::forbidden("会话不属于该工作区"));
            }
            (sid.clone(), title)
        }
        None => {
            let sid = crate::new_id();
            let title = session_title(&message);
            sqlx::query("INSERT INTO assistant_sessions (id, workspace_id, title) VALUES (?,?,?)")
                .bind(&sid)
                .bind(&ws_id)
                .bind(&title)
                .execute(&state.db)
                .await?;
            (sid, title)
        }
    };

    // 历史（仅当前会话），先取历史再落库当前消息
    let history = sqlx::query_as::<_, (String, String)>(
        "SELECT role, content FROM assistant_messages WHERE session_id = ? ORDER BY id DESC LIMIT 20",
    )
    .bind(&session_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    // 先落用户消息，断线也不丢
    sqlx::query("INSERT INTO assistant_messages (workspace_id, session_id, role, content) VALUES (?,?, 'user', ?)")
        .bind(&ws_id)
        .bind(&session_id)
        .bind(&message)
        .execute(&state.db)
        .await?;
    sqlx::query("UPDATE assistant_sessions SET updated_at = ? WHERE id = ?")
        .bind(crate::now())
        .bind(&session_id)
        .execute(&state.db)
        .await?;

    let context = build_context(&state, &ws).await;
    let mut prompt = format!(
        "你是 Fux 工作区助理，用简体中文回答，尽量简洁并使用 Markdown。\n\
         你可以通过 fux-workspace MCP 工具查询/创建/移动任务与评论。执行写操作前先说明将要做什么。\n\n\
         ## 工作区上下文\n{context}\n"
    );
    for (role, content) in history.iter().rev() {
        prompt.push_str(&format!("{role}: {content}\n"));
    }
    prompt.push_str(&format!("user: {message}\nassistant:"));

    let cwd = state
        .data_dir
        .join("assistant")
        .join(&ws_id)
        .join(&session_id);
    tokio::fs::create_dir_all(&cwd).await.ok();

    let mut mcp = vec![];
    if which::which("fux-mcp").is_ok() {
        let token = issue_token(&state.jwt_secret, &user.id, &user.username)?;
        mcp.push(json!({
            "name": "fux-workspace", "command": "fux-mcp", "args": [],
            "env": [
                { "name": "FUX_API", "value": state.api_base },
                { "name": "FUX_TOKEN", "value": token },
                { "name": "FUX_WORKSPACE", "value": ws_id },
            ]
        }));
    }

    let mut args: Vec<String> = serde_json::from_str(&cli.args).unwrap_or_default();
    if cli.kind == "copilot" {
        args.push(format!(
            "--effort={}",
            crate::runner::map_effort(&agent.reasoning_effort)
        ));
    }
    let env: Vec<(String, String)> =
        serde_json::from_str::<serde_json::Map<String, Value>>(&cli.env_template)
            .map(|m| {
                m.into_iter()
                    .map(|(k, v)| (k, v.as_str().unwrap_or("").to_string()))
                    .collect()
            })
            .unwrap_or_default();
    let full_prompt = if agent.system_prompt.is_empty() {
        prompt
    } else {
        format!("{}\n\n{prompt}", agent.system_prompt)
    };

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let _ = tx.send(format!(
        "data: {}\n\n",
        json!({ "type": "session", "sessionId": session_id, "title": title })
    ));

    let db = state.db.clone();
    let sid2 = session_id.clone();
    let ws_id2 = ws_id.clone();
    tokio::spawn(async move {
        let answer = Arc::new(Mutex::new(String::new()));
        let result: anyhow::Result<()> = async {
            let mut client = AcpClient::spawn(&cli.command, &args, &env, &cwd).await?;
            let mut updates = client.take_updates();
            let mut requests = client.take_requests();
            client.initialize().await?;
            let acp_sid = client.new_session(&cwd, mcp).await?;
            let client = Arc::new(client);
            let pump = {
                let client = client.clone();
                tokio::spawn(async move {
                    loop {
                        tokio::select! {
                            Some(req) = requests.recv() => {
                                if req.method == "session/request_permission" {
                                    let opt = req.params.get("options").and_then(|o| o.as_array())
                                        .and_then(|arr| arr.first())
                                        .and_then(|o| o.get("optionId")).and_then(|v| v.as_str()).unwrap_or("");
                                    let _ = client.respond(req.id, json!({ "outcome": { "outcome": "selected", "optionId": opt } })).await;
                                } else {
                                    let _ = client.respond(req.id, json!({})).await;
                                }
                            }
                            else => break,
                        }
                    }
                })
            };
            let handle_chunk = |u: &Value, tx: &mpsc::UnboundedSender<String>, answer: &Arc<Mutex<String>>| {
                if u.pointer("/update/sessionUpdate").and_then(|k| k.as_str()) == Some("agent_message_chunk") {
                    if let Some(text) = u.pointer("/update/content/text").and_then(|t| t.as_str()) {
                        let text = text.to_string();
                        if let Ok(mut guard) = answer.try_lock() { guard.push_str(&text); }
                        let _ = tx.send(format!("data: {}\n\n", json!({ "type": "chunk", "text": text })));
                    }
                }
            };
            let resp = {
                let fut = client.prompt(&acp_sid, &full_prompt);
                tokio::pin!(fut);
                loop {
                    tokio::select! {
                        r = &mut fut => break r,
                        Some(u) = updates.recv() => handle_chunk(&u, &tx, &answer),
                    }
                }
            };
            while let Ok(u) = updates.try_recv() { handle_chunk(&u, &tx, &answer); }
            client.kill().await;
            pump.abort();
            resp?;
            Ok(())
        }.await;
        let answer_text = answer.lock().await.clone();
        if !answer_text.is_empty() {
            let _ = sqlx::query("INSERT INTO assistant_messages (workspace_id, session_id, role, content) VALUES (?,?, 'assistant', ?)")
                .bind(&ws_id2)
                .bind(&sid2)
                .bind(&answer_text)
                .execute(&db).await;
        }
        match result {
            Ok(_) => {
                let _ = tx.send(format!("data: {}\n\n", json!({ "type": "done" })));
            }
            Err(e) => {
                let _ = tx.send(format!(
                    "data: {}\n\n",
                    json!({ "type": "error", "message": format!("{e:#}") })
                ));
            }
        }
    });

    let stream = async_stream::stream! {
        while let Some(line) = rx.recv().await {
            yield Ok::<_, actix_web::Error>(actix_web::web::Bytes::from(line));
        }
    };
    Ok(HttpResponse::Ok()
        .insert_header(("Content-Type", "text/event-stream"))
        .insert_header(("Cache-Control", "no-cache"))
        .streaming(stream))
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/workspaces/{id}/assistant/sessions",
        web::get().to(list_sessions),
    )
    .route(
        "/assistant/sessions/{id}/messages",
        web::get().to(session_messages),
    )
    .route("/assistant/sessions/{id}", web::patch().to(rename_session))
    .route("/assistant/sessions/{id}", web::delete().to(delete_session))
    .route("/assistant/chat", web::post().to(chat));
}
