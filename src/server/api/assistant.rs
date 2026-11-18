//! AI 工作助理（FR-03）：绑定 CLI 的智能体 + 内置 workspace MCP，SSE 流式回复。
use crate::acp::AcpClient;
use crate::server::SharedState;
use crate::server::auth::{AuthUser, issue_token, require};
use crate::server::error::{ApiError, ApiResult};
use crate::server::models::{Agent, CliEntry, Workspace};
use actix_web::{HttpResponse, web};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatReq {
    workspace_id: String,
    message: String,
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

    // 工作区上下文
    let stats = sqlx::query_as::<_, (String, i64)>(
        "SELECT status, COUNT(*) FROM tasks WHERE workspace_id = ? GROUP BY status",
    )
    .bind(&ws_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    let doing = sqlx::query_scalar::<_, String>(
        "SELECT title FROM tasks WHERE workspace_id = ? AND status = 'in_progress' LIMIT 10",
    )
    .bind(&ws_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    let context = format!(
        "工作区「{}」看板统计: {:?}；进行中任务: {:?}",
        ws.name, stats, doing
    );

    let history = sqlx::query_as::<_, (String, String)>("SELECT role, content FROM assistant_messages WHERE workspace_id = ? ORDER BY id DESC LIMIT 16")
        .bind(&ws_id).fetch_all(&state.db).await.unwrap_or_default();
    let mut prompt = format!(
        "你是 Fux 工作区助理。{context}\n\n你可以通过 fux-workspace MCP 工具操作工作台（创建/移动任务、评论、查询）。回答简洁。\n\n"
    );
    for (role, content) in history.iter().rev() {
        prompt.push_str(&format!("{role}: {content}\n"));
    }
    prompt.push_str(&format!("user: {}\n", body.message));

    let cwd = state.data_dir.join("assistant").join(&ws_id);
    tokio::fs::create_dir_all(&cwd).await.ok();

    // 内置 workspace MCP（若 fux-mcp 已构建/安装）
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

    let args: Vec<String> = serde_json::from_str(&cli.args).unwrap_or_default();
    let mut args = args;
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
    let system_prompt = agent.system_prompt.clone();
    let full_prompt = if system_prompt.is_empty() {
        prompt
    } else {
        format!("{system_prompt}\n\n{prompt}")
    };

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let db = state.db.clone();
    let ws_id2 = ws_id.clone();
    let user_msg = body.message.clone();
    tokio::spawn(async move {
        let answer = Arc::new(Mutex::new(String::new()));
        let result: anyhow::Result<()> = async {
            let mut client = AcpClient::spawn(&cli.command, &args, &env, &cwd).await?;
            let mut updates = client.take_updates();
            let mut requests = client.take_requests();
            client.initialize().await?;
            let sid = client.new_session(&cwd, mcp).await?;
            let client = Arc::new(client);
            // pump 只处理权限等 agent→client 请求；消息分片在等待 prompt 期间内联消费
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
                        let answer = answer.clone();
                        let line = format!("data: {}\n\n", json!({ "type": "chunk", "text": text }));
                        let tx = tx.clone();
                        // 同步累积 + 发送（避免异步闭包复杂度）
                        if let Ok(mut guard) = answer.try_lock() { guard.push_str(&text); }
                        let _ = tx.send(line);
                    }
                }
            };
            let resp = {
                let fut = client.prompt(&sid, &full_prompt);
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
        let _ = sqlx::query(
            "INSERT INTO assistant_messages (workspace_id, role, content) VALUES (?, 'user', ?)",
        )
        .bind(&ws_id2)
        .bind(&user_msg)
        .execute(&db)
        .await;
        if !answer_text.is_empty() {
            let _ = sqlx::query("INSERT INTO assistant_messages (workspace_id, role, content) VALUES (?, 'assistant', ?)")
                .bind(&ws_id2).bind(&answer_text).execute(&db).await;
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
    cfg.route("/assistant/chat", web::post().to(chat));
}
