use crate::server::auth::{require, AuthUser};
use crate::server::error::{ApiError, ApiResult};
use crate::server::models::{Board, Lane};
use crate::server::SharedState;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use serde_json::json;

pub async fn board_of_workspace(state: &SharedState, ws_id: &str) -> ApiResult<Board> {
    Ok(sqlx::query_as::<_, Board>("SELECT * FROM boards WHERE workspace_id = ?").bind(ws_id).fetch_one(&state.db).await?)
}

pub async fn load_board(state: &SharedState, id: &str) -> ApiResult<Board> {
    Ok(sqlx::query_as::<_, Board>("SELECT * FROM boards WHERE id = ?").bind(id).fetch_one(&state.db).await?)
}

async fn get_by_ws(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let ws_id = path.into_inner();
    require(&state.db, &ws_id, &user.id, "member").await?;
    let board = board_of_workspace(&state, &ws_id).await?;
    let lanes = sqlx::query_as::<_, Lane>("SELECT * FROM lanes WHERE board_id = ? ORDER BY position").bind(&board.id).fetch_all(&state.db).await?;
    Ok(HttpResponse::Ok().json(json!({ "board": board, "lanes": lanes })))
}

async fn get(state: SharedState, user: AuthUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let board = load_board(&state, &id).await?;
    require(&state.db, &board.workspace_id, &user.id, "member").await?;
    let lanes = sqlx::query_as::<_, Lane>("SELECT * FROM lanes WHERE board_id = ? ORDER BY position").bind(&id).fetch_all(&state.db).await?;
    Ok(HttpResponse::Ok().json(json!({ "board": board, "lanes": lanes })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LaneReq {
    id: Option<String>,
    kind: String,
    name: String,
    position: i64,
    wip_limit: Option<i64>,
    stage_role: Option<String>,
}

#[derive(Deserialize)]
struct LanesReq { lanes: Vec<LaneReq> }

/// 泳道定制：语义泳道各一，work 子泳道可多个（PRD 3.5.1）。
async fn put_lanes(state: SharedState, user: AuthUser, path: web::Path<String>, body: web::Json<LanesReq>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let board = load_board(&state, &id).await?;
    require(&state.db, &board.workspace_id, &user.id, "admin").await?;
    let required = ["planning", "ready", "review", "blocked", "done", "archived"];
    for k in required {
        let n = body.lanes.iter().filter(|l| l.kind == k).count();
        if n != 1 { return Err(ApiError::bad_request(format!("语义泳道 {k} 必须恰好一个"))); }
    }
    if !body.lanes.iter().any(|l| l.kind == "work") {
        return Err(ApiError::bad_request("至少需要一个工作子泳道"));
    }
    if body.lanes.iter().any(|l| !["planning","ready","work","review","blocked","done","archived"].contains(&l.kind.as_str())) {
        return Err(ApiError::bad_request("非法泳道类型"));
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM lanes WHERE board_id = ?").bind(&id).execute(&mut *tx).await?;
    for l in &body.lanes {
        let lane_id = l.id.clone().unwrap_or_else(crate::new_id);
        sqlx::query("INSERT INTO lanes (id, board_id, kind, name, position, wip_limit, stage_role) VALUES (?,?,?,?,?,?,?)")
            .bind(&lane_id).bind(&id).bind(&l.kind).bind(&l.name).bind(l.position).bind(l.wip_limit).bind(&l.stage_role)
            .execute(&mut *tx).await?;
    }
    // 任务重新挂到对应类型的首个泳道
    sqlx::query(
        "UPDATE tasks SET lane_id = (SELECT l.id FROM lanes l WHERE l.board_id = tasks.board_id AND l.kind = CASE tasks.status WHEN 'planning' THEN 'planning' WHEN 'ready' THEN 'ready' WHEN 'in_progress' THEN 'work' WHEN 'review' THEN 'review' WHEN 'blocked' THEN 'blocked' WHEN 'done' THEN 'done' WHEN 'archived' THEN 'archived' END ORDER BY l.position LIMIT 1) WHERE board_id = ?",
    ).bind(&id).execute(&mut *tx).await?;
    tx.commit().await?;
    let lanes = sqlx::query_as::<_, Lane>("SELECT * FROM lanes WHERE board_id = ? ORDER BY position").bind(&id).fetch_all(&state.db).await?;
    Ok(HttpResponse::Ok().json(lanes))
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/workspaces/{id}/board", web::get().to(get_by_ws))
        .route("/boards/{id}", web::get().to(get))
        .route("/boards/{id}/lanes", web::put().to(put_lanes));
}