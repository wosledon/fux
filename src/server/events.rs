//! 事件溯源时间线：TaskEvent 为唯一事实来源；SSE 通过广播 + 序号续传。
use crate::server::error::ApiResult;
use crate::server::models::TaskEvent;
use serde::Serialize;
use serde_json::Value;
use sqlx::SqlitePool;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventEnvelope {
    pub workspace_id: String,
    pub task_id: String,
    pub seq: i64,
    pub kind: String,
    pub actor: String,
    pub run_id: Option<String>,
    pub payload: Value,
    pub created_at: String,
}

pub async fn append(
    db: &SqlitePool,
    tx_bus: &broadcast::Sender<EventEnvelope>,
    task_id: &str,
    kind: &str,
    actor: &str,
    run_id: Option<&str>,
    payload: Value,
) -> ApiResult<EventEnvelope> {
    let mut tx = db.begin().await?;
    let workspace_id: String =
        sqlx::query_scalar("SELECT workspace_id FROM tasks WHERE id = ?")
            .bind(task_id)
            .fetch_one(&mut *tx)
            .await?;
    let seq: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(seq), 0) + 1 FROM task_events WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(&mut *tx)
            .await?;
    let created_at = crate::now();
    sqlx::query(
        "INSERT INTO task_events (task_id, seq, kind, actor, run_id, payload, created_at) VALUES (?,?,?,?,?,?,?)",
    )
    .bind(task_id)
    .bind(seq)
    .bind(kind)
    .bind(actor)
    .bind(run_id)
    .bind(payload.to_string())
    .bind(&created_at)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE tasks SET updated_at = ? WHERE id = ?")
        .bind(&created_at)
        .bind(task_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let env = EventEnvelope {
        workspace_id,
        task_id: task_id.to_string(),
        seq,
        kind: kind.to_string(),
        actor: actor.to_string(),
        run_id: run_id.map(String::from),
        payload,
        created_at,
    };
    let _ = tx_bus.send(env.clone());
    Ok(env)
}

pub async fn list_after(db: &SqlitePool, task_id: &str, after: i64) -> ApiResult<Vec<TaskEvent>> {
    Ok(sqlx::query_as::<_, TaskEvent>(
        "SELECT * FROM task_events WHERE task_id = ? AND seq > ? ORDER BY seq ASC",
    )
    .bind(task_id)
    .bind(after)
    .fetch_all(db)
    .await?)
}

pub fn to_envelope(db_row: &TaskEvent, workspace_id: &str) -> EventEnvelope {
    EventEnvelope {
        workspace_id: workspace_id.to_string(),
        task_id: db_row.task_id.clone(),
        seq: db_row.seq,
        kind: db_row.kind.clone(),
        actor: db_row.actor.clone(),
        run_id: db_row.run_id.clone(),
        payload: serde_json::from_str(&db_row.payload).unwrap_or(Value::Null),
        created_at: db_row.created_at.clone(),
    }
}