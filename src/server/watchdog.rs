//! 质量阀（FR-13）：循环抽搐 / 停滞 / 突然中断 / 硬上限 → 熔断阻塞。
use crate::server::SharedState;
use crate::server::domain::{self, MoveOpts};
use crate::server::events;
use crate::server::models::{Agent, Run, Workspace};
use serde_json::{Value, json};

struct Gates {
    loop_window: usize,
    silence_secs: i64,
    no_progress_secs: i64,
    max_turns: i64,
    max_wall_secs: i64,
}

impl Default for Gates {
    fn default() -> Self {
        Self {
            loop_window: 5,
            silence_secs: 120,
            no_progress_secs: 600,
            max_turns: 200,
            max_wall_secs: 3600,
        }
    }
}

fn parse_ts(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc))
}

fn gates_for(agent: &Agent, ws: &Workspace) -> Gates {
    let mut g = Gates::default();
    let mut apply = |v: &Value| {
        if let Some(x) = v.get("loopWindow").and_then(|x| x.as_u64()) {
            g.loop_window = x as usize;
        }
        if let Some(x) = v.get("silenceSecs").and_then(|x| x.as_i64()) {
            g.silence_secs = x;
        }
        if let Some(x) = v.get("noProgressSecs").and_then(|x| x.as_i64()) {
            g.no_progress_secs = x;
        }
        if let Some(x) = v.get("maxTurns").and_then(|x| x.as_i64()) {
            g.max_turns = x;
        }
        if let Some(x) = v.get("maxWallSecs").and_then(|x| x.as_i64()) {
            g.max_wall_secs = x;
        }
    };
    if let Ok(v) = serde_json::from_str::<Value>(&ws.settings) {
        if let Some(gates) = v.get("gates") {
            apply(gates);
        }
    }
    if let Ok(v) = serde_json::from_str::<Value>(&agent.gates) {
        apply(&v);
    }
    g
}

pub fn spawn(state: SharedState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            if let Err(e) = scan(&state).await {
                tracing::warn!("watchdog: {e:#}");
            }
        }
    });
}

/// 单次扫描（测试/手动触发）。
pub async fn scan_once(state: &SharedState) -> anyhow::Result<()> {
    scan(state).await
}

async fn scan(state: &SharedState) -> anyhow::Result<()> {
    let runs = sqlx::query_as::<_, Run>("SELECT * FROM runs WHERE status = 'running'")
        .fetch_all(&state.db)
        .await?;
    let now = chrono::Utc::now();
    for run in runs {
        let task = match domain::load_task(state, &run.task_id).await {
            Ok(t) => t,
            Err(_) => continue,
        };
        if task.status != "in_progress" || task.lease_run_id.as_deref() != Some(run.id.as_str()) {
            continue;
        }
        let agent = match sqlx::query_as::<_, Agent>("SELECT * FROM agents WHERE id = ?")
            .bind(&run.agent_id)
            .fetch_one(&state.db)
            .await
        {
            Ok(a) => a,
            Err(_) => continue,
        };
        let ws = match sqlx::query_as::<_, Workspace>("SELECT * FROM workspaces WHERE id = ?")
            .bind(&task.workspace_id)
            .fetch_one(&state.db)
            .await
        {
            Ok(w) => w,
            Err(_) => continue,
        };
        let g = gates_for(&agent, &ws);

        let last_ts = parse_ts(run.last_event_at.as_deref().unwrap_or(&run.started_at));
        let start_ts = parse_ts(&run.started_at);
        let silence = last_ts.map(|t| (now - t).num_seconds()).unwrap_or(0);
        let wall = start_ts.map(|t| (now - t).num_seconds()).unwrap_or(0);

        let mut trip: Option<(&str, String)> = None;
        // 硬上限
        if run.turn_count >= g.max_turns {
            trip = Some((
                "hard_limit",
                format!("轮次超限: {} >= {}", run.turn_count, g.max_turns),
            ));
        } else if wall > g.max_wall_secs {
            trip = Some((
                "hard_limit",
                format!("墙钟超限: {wall}s > {}s", g.max_wall_secs),
            ));
        } else if silence > g.silence_secs * 2 {
            // 静默熔断（提醒注入受 ACP 单 prompt 限制，在 turn 间尽力而为）
            trip = Some((
                "stalled",
                format!("事件静默 {silence}s（阈值 {}s）", g.silence_secs * 2),
            ));
        } else {
            // 循环检测：最近 N 次工具调用完全相同
            let calls = sqlx::query_as::<_, crate::server::models::TaskEvent>(
                "SELECT * FROM task_events WHERE run_id = ? AND kind = 'tool.call' ORDER BY seq DESC LIMIT ?",
            ).bind(&run.id).bind(g.loop_window as i64).fetch_all(&state.db).await.unwrap_or_default();
            if calls.len() >= g.loop_window && g.loop_window >= 3 {
                let hashes: Vec<String> = calls
                    .iter()
                    .map(|c| {
                        let v: Value = serde_json::from_str(&c.payload).unwrap_or(json!({}));
                        format!(
                            "{}|{}",
                            v.get("title").and_then(|t| t.as_str()).unwrap_or(""),
                            v.get("rawInput").map(|r| r.to_string()).unwrap_or_default()
                        )
                    })
                    .collect();
                if hashes.windows(2).all(|w| w[0] == w[1]) {
                    trip = Some((
                        "loop",
                        format!("连续 {} 次相同工具调用（模型抽搐）", g.loop_window),
                    ));
                }
            }
            // 无进展：长时间只有读类操作，无编辑/执行
            if trip.is_none() && wall > g.no_progress_secs {
                let all_calls = sqlx::query_as::<_, crate::server::models::TaskEvent>(
                    "SELECT * FROM task_events WHERE run_id = ? AND kind = 'tool.call'",
                )
                .bind(&run.id)
                .fetch_all(&state.db)
                .await
                .unwrap_or_default();
                let has_progress = all_calls.iter().any(|c| {
                    let v: Value = serde_json::from_str(&c.payload).unwrap_or(json!({}));
                    matches!(
                        v.get("kind").and_then(|k| k.as_str()),
                        Some("edit" | "write" | "execute" | "terminal" | "delete" | "move")
                    )
                });
                if !has_progress && !all_calls.is_empty() {
                    trip = Some(("no_progress", format!("{wall}s 内无编辑/执行类进展")));
                }
            }
        }

        if let Some((reason, diag)) = trip {
            tracing::warn!("质量阀熔断 run={} reason={reason}: {diag}", run.id);
            let _ = events::append(
                &state.db,
                &state.broadcast,
                &task.id,
                &format!("watchdog.{reason}"),
                "system",
                Some(&run.id),
                json!({ "diagnostics": diag, "turnCount": run.turn_count, "silenceSecs": silence }),
            )
            .await;
            crate::server::scheduler::cancel_run(state, &run).await;
            let _ = sqlx::query(
                "UPDATE runs SET status = 'cancelled', stop_reason = ?, ended_at = ? WHERE id = ?",
            )
            .bind(format!("watchdog.{reason}"))
            .bind(crate::now())
            .bind(&run.id)
            .execute(&state.db)
            .await;
            let _ = domain::move_task(
                state,
                &task.id,
                "blocked",
                "system",
                MoveOpts {
                    reason: Some(format!("质量阀熔断 [{reason}]: {diag}")),
                    ..Default::default()
                },
            )
            .await;
            crate::server::api::workspaces::notify(
                state,
                &task.workspace_id,
                "watchdog",
                Some(&task.id),
                &format!("任务「{}」触发质量阀 [{reason}]，已熔断阻塞", task.title),
            )
            .await;
        }
    }
    Ok(())
}
