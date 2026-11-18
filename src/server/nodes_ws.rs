//! 节点通道（出站 WS）：runtime 注册/心跳/事件上报/Run 派发。
use crate::proto::{RuntimeMsg, ServerMsg};
use crate::runner::RunOutcome;
use crate::server::error::{ApiError, ApiResult};
use crate::server::events;
use crate::server::gitflow;
use crate::server::models::{Node, Run};
use crate::server::SharedState;
use actix_web::{web, HttpRequest, HttpResponse};
use serde_json::json;
use tokio::sync::mpsc;

pub async fn channel(state: SharedState, req: HttpRequest, stream: web::Payload) -> ApiResult<HttpResponse> {
    let token = req.query_string().split('&').find_map(|kv| kv.strip_prefix("token=")).unwrap_or("").to_string();
    let node = sqlx::query_as::<_, Node>("SELECT * FROM nodes WHERE token = ? AND mode = 'runtime'")
        .bind(&token).fetch_optional(&state.db).await?
        .ok_or_else(|| ApiError::unauthorized("节点令牌无效"))?;

    let (res, session, mut msg_stream) = actix_ws::handle(&req, stream)
        .map_err(|e| ApiError::bad_request(format!("WS 握手失败: {e}")))?;

    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();
    state.nodes.register(&node.id, tx.clone()).await;
    let _ = sqlx::query("UPDATE nodes SET conn_status = 'online', last_heartbeat = ? WHERE id = ?")
        .bind(crate::now()).bind(&node.id).execute(&state.db).await;
    let _ = tx.send(ServerMsg::Hello { node_id: node.id.clone(), approved: node.approved == 1 });

    // 写端
    let mut writer = session.clone();
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if writer.text(serde_json::to_string(&msg).unwrap_or_default()).await.is_err() { break; }
        }
    });

    // 读端
    let mut pinger = session.clone();
    let node_id = node.id.clone();
    while let Some(Ok(msg)) = msg_stream.recv().await {
        match msg {
            actix_ws::Message::Text(t) => {
                match serde_json::from_str::<RuntimeMsg>(&t) {
                    Ok(m) => handle_msg(&state, &node_id, m).await,
                    Err(e) => tracing::warn!("节点消息解析失败: {e}"),
                }
            }
            actix_ws::Message::Ping(b) => { let _ = pinger.pong(&b).await; }
            actix_ws::Message::Close(_) => break,
            _ => {}
        }
    }

    state.nodes.unregister(&node_id).await;
    let _ = sqlx::query("UPDATE nodes SET conn_status = 'offline' WHERE id = ?").bind(&node_id).execute(&state.db).await;
    Ok(res)
}

async fn handle_msg(state: &SharedState, node_id: &str, msg: RuntimeMsg) {
    match msg {
        RuntimeMsg::Heartbeat { load } => {
            let caps: Option<String> = sqlx::query_scalar("SELECT capabilities FROM nodes WHERE id = ?").bind(node_id).fetch_optional(&state.db).await.ok().flatten();
            let mut v: serde_json::Value = caps.and_then(|c| serde_json::from_str(&c).ok()).unwrap_or(json!({}));
            v["load"] = json!(load);
            let _ = sqlx::query("UPDATE nodes SET last_heartbeat = ?, capabilities = ? WHERE id = ?")
                .bind(crate::now()).bind(v.to_string()).bind(node_id).execute(&state.db).await;
        }
        RuntimeMsg::Event { run_id, kind, payload } => {
            if let Ok(run) = sqlx::query_as::<_, Run>("SELECT * FROM runs WHERE id = ?").bind(&run_id).fetch_one(&state.db).await {
                let _ = events::append(&state.db, &state.broadcast, &run.task_id, &kind,
                    &format!("agent:{}", run.agent_id), Some(&run_id), payload).await;
                let _ = sqlx::query("UPDATE runs SET last_event_at = ? WHERE id = ?").bind(crate::now()).bind(&run_id).execute(&state.db).await;
                let _ = sqlx::query("UPDATE tasks SET lease_expires_at = ? WHERE lease_run_id = ?").bind(gitflow::lease_deadline()).bind(&run_id).execute(&state.db).await;
            }
        }
        RuntimeMsg::RunState { run_id, turn_count, session_id, .. } => {
            let _ = sqlx::query("UPDATE runs SET turn_count = ?, acp_session_id = ?, last_event_at = ? WHERE id = ?")
                .bind(turn_count as i64).bind(&session_id).bind(crate::now()).bind(&run_id).execute(&state.db).await;
            let _ = sqlx::query("UPDATE tasks SET lease_expires_at = ? WHERE lease_run_id = ?").bind(gitflow::lease_deadline()).bind(&run_id).execute(&state.db).await;
        }
        RuntimeMsg::PermissionRequest { run_id, request_id, request } => {
            if let Ok(run) = sqlx::query_as::<_, Run>("SELECT * FROM runs WHERE id = ?").bind(&run_id).fetch_one(&state.db).await {
                let _ = sqlx::query("INSERT OR IGNORE INTO approvals (id, run_id, task_id, request) VALUES (?,?,?,?)")
                    .bind(&request_id).bind(&run_id).bind(&run.task_id).bind(request.to_string()).execute(&state.db).await;
                let _ = events::append(&state.db, &state.broadcast, &run.task_id, "permission.requested",
                    &format!("agent:{}", run.agent_id), Some(&run_id),
                    json!({ "approvalId": request_id, "request": request })).await;
            }
        }
        RuntimeMsg::Finish { run_id, status, stop_reason, turn_count, pushed, head, error } => {
            let outcome = RunOutcome { status, stop_reason, turn_count, session_id: String::new(), error };
            gitflow::finish_run(state, &run_id, outcome, Some((pushed, head))).await;
        }
        RuntimeMsg::Pong => {}
    }
}