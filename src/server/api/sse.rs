//! SSE：任务时间线 / 工作看板变更；事件序号单调，支持 Last-Event-ID 续传。
use crate::server::api::claims_from_req;
use crate::server::auth::require;
use crate::server::domain;
use crate::server::error::ApiResult;
use crate::server::events;
use crate::server::SharedState;
use actix_web::{web, HttpRequest, HttpResponse};
use std::time::Duration;
use tokio::sync::broadcast;

fn sse_bytes(env: &events::EventEnvelope) -> actix_web::web::Bytes {
    actix_web::web::Bytes::from(format!(
        "id: {}\nevent: message\ndata: {}\n\n",
        env.seq,
        serde_json::to_string(env).unwrap_or_default()
    ))
}

fn last_id(req: &HttpRequest) -> i64 {
    req.headers()
        .get("Last-Event-ID")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .or_else(|| {
            req.query_string().split('&').find_map(|kv| kv.strip_prefix("after=")).and_then(|s| s.parse().ok())
        })
        .unwrap_or(0)
}

async fn task_events(state: SharedState, req: HttpRequest, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let claims = claims_from_req(&req, &state.jwt_secret)?;
    let task_id = path.into_inner();
    let task = domain::load_task(&state, &task_id).await?;
    require(&state.db, &task.workspace_id, &claims.sub, "member").await?;
    let after = last_id(&req);
    let ws_id = task.workspace_id.clone();
    let db = state.db.clone();
    let mut rx = state.broadcast.subscribe();
    let stream = async_stream::stream! {
        let mut last = after;
        if let Ok(rows) = events::list_after(&db, &task_id, last).await {
            for r in rows {
                let env = events::to_envelope(&r, &ws_id);
                last = env.seq;
                yield Ok::<_, actix_web::Error>(sse_bytes(&env));
            }
        }
        loop {
            match tokio::time::timeout(Duration::from_secs(15), rx.recv()).await {
                Ok(Ok(env)) => {
                    if env.task_id != task_id || env.seq <= last { continue; }
                    last = env.seq;
                    yield Ok(sse_bytes(&env));
                }
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
                    if let Ok(rows) = events::list_after(&db, &task_id, last).await {
                        for r in rows {
                            let env = events::to_envelope(&r, &ws_id);
                            last = env.seq;
                            yield Ok(sse_bytes(&env));
                        }
                    }
                }
                Ok(Err(broadcast::error::RecvError::Closed)) => break,
                Err(_) => yield Ok(actix_web::web::Bytes::from(": ping\n\n")),
            }
        }
    };
    Ok(HttpResponse::Ok()
        .insert_header(("Content-Type", "text/event-stream"))
        .insert_header(("Cache-Control", "no-cache"))
        .insert_header(("X-Accel-Buffering", "no"))
        .streaming(stream))
}

async fn workspace_events(state: SharedState, req: HttpRequest, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let claims = claims_from_req(&req, &state.jwt_secret)?;
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &claims.sub, "member").await?;
    let mut rx = state.broadcast.subscribe();
    let stream = async_stream::stream! {
        yield Ok::<_, actix_web::Error>(actix_web::web::Bytes::from(": connected\n\n"));
        loop {
            match tokio::time::timeout(Duration::from_secs(15), rx.recv()).await {
                Ok(Ok(env)) => {
                    if env.workspace_id != ws_id { continue; }
                    yield Ok(sse_bytes(&env));
                }
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(broadcast::error::RecvError::Closed)) => break,
                Err(_) => yield Ok(actix_web::web::Bytes::from(": ping\n\n")),
            }
        }
    };
    Ok(HttpResponse::Ok()
        .insert_header(("Content-Type", "text/event-stream"))
        .insert_header(("Cache-Control", "no-cache"))
        .insert_header(("X-Accel-Buffering", "no"))
        .streaming(stream))
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/tasks/{id}/events", web::get().to(task_events))
        .route("/workspaces/{id}/events", web::get().to(workspace_events));
}