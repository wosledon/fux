//! AI 工作助理（FR-03）：多会话对话，绑定 CLI 的智能体 + 内置 workspace MCP，SSE 流式回复。
use crate::acp::AcpClient;
use crate::server::SharedState;
use crate::server::api::agents;
use crate::server::auth::{AuthUser, issue_token, require};
use crate::server::error::{ApiError, ApiResult};
use crate::server::models::{Agent, CliEntry, Workspace};
use actix_web::{HttpResponse, web};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, broadcast, mpsc};

/// 助理常驻连接上限：每个连接都是一个真实 CLI 进程，需防止会话无限增多拖垮机器。
const MAX_CONNS: usize = 8;
/// 单轮 prompt 的兜底超时：CLI 僵死（不退出也不响应）时释放连接与排队中的后续消息。
const PROMPT_TIMEOUT_SECS: u64 = 600;
/// 连接空闲回收阈值（reaper 每 60s 扫描一次）。
const IDLE_EVICT_SECS: u64 = 600;
/// 「生成中」标记的过期阈值：任务异常退出时最多残留这么久，防止状态永久卡住。
const GENERATING_STALE_SECS: u64 = 600;

/// SSE 断开守卫：流被丢弃（客户端断线/关页）时通知生成任务取消，避免后台空跑烧额度。
struct DisconnectGuard(Option<tokio::sync::oneshot::Sender<()>>);

impl Drop for DisconnectGuard {
    fn drop(&mut self) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(());
        }
    }
}

/// 上下文指纹：用于判断复用会话时工作区上下文是否有变化（无需引入哈希依赖）。
fn context_hash(context: &str) -> String {
    let mut h: u64 = 1469598103934665603;
    for b in context.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    format!("{h:x}")
}

impl Default for AssistantPool {
    fn default() -> Self {
        Self {
            conns: Mutex::new(HashMap::new()),
            generating: std::sync::Mutex::new(HashMap::new()),
            max_conns: MAX_CONNS,
        }
    }
}

/// 助理常驻 ACP 连接池：同一工作区会话复用进程与 ACP session，避免每条消息都冷启动。
pub struct AssistantPool {
    conns: Mutex<HashMap<String, Arc<AssistantConn>>>,
    /// 正在生成的会话（用于页面刷新/离开后回来继续等待结果）
    generating: std::sync::Mutex<HashMap<String, Instant>>,
    /// 最大常驻连接数（每个连接 ≈ 一个 CLI 进程），超出按最近使用淘汰
    max_conns: usize,
}

struct AssistantConn {
    /// 智能体 / CLI / 模型 / 授权配置指纹；变化时重建连接
    fingerprint: String,
    /// 创建时注入的工作区上下文指纹；变化时复用消息附带最新上下文
    context_hash: String,
    client: Arc<AcpClient>,
    acp_session_id: String,
    /// 把 session/update 广播给当前请求（同一时刻只允许一个 prompt）
    updates: broadcast::Sender<Value>,
    last_used: std::sync::Mutex<Instant>,
    prompt_lock: Mutex<()>,
}

/// 单轮对话的执行过程（思考 + 工具调用），结束后作为 role=process 落库。
#[derive(Default)]
struct ProcessState {
    thoughts: String,
    tools: Vec<Value>,
}

fn upsert_tool(
    tools: &mut Vec<Value>,
    id: &str,
    title: Option<&str>,
    status: Option<&str>,
    kind: Option<&str>,
    detail: Option<&str>,
) {
    if id.is_empty() {
        return;
    }
    if let Some(t) = tools
        .iter_mut()
        .find(|t| t.get("id").and_then(|v| v.as_str()) == Some(id))
    {
        if let Some(title) = title {
            t["title"] = json!(title);
        }
        if let Some(status) = status {
            t["status"] = json!(status);
        }
        if let Some(kind) = kind {
            t["kind"] = json!(kind);
        }
        if let Some(detail) = detail {
            t["detail"] = json!(detail);
        }
    } else {
        tools.push(json!({
            "id": id,
            "title": title.unwrap_or("工具"),
            "status": status.unwrap_or("running"),
            "kind": kind.unwrap_or("other"),
            "detail": detail,
        }));
    }
}

/// 从 rawInput 提取可读的工具操作摘要（命令 / 路径 / 搜索词等）。
fn tool_detail(u: &Value) -> Option<String> {
    let raw = u.pointer("/update/rawInput")?;
    for key in [
        "command",
        "path",
        "pattern",
        "query",
        "description",
        "url",
        "name",
    ] {
        if let Some(s) = raw.get(key).and_then(|v| v.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                return Some(s.chars().take(200).collect());
            }
        }
    }
    None
}

impl AssistantPool {
    fn mark_generating(&self, key: &str, on: bool) {
        let mut map = self.generating.lock().unwrap_or_else(|e| e.into_inner());
        if on {
            map.insert(key.to_string(), Instant::now());
        } else {
            map.remove(key);
        }
    }

    /// 生成中或最近 10 分钟内仍在生成（防止进程异常导致状态卡住）。
    fn is_generating(&self, key: &str) -> bool {
        let map = self.generating.lock().unwrap_or_else(|e| e.into_inner());
        map.get(key)
            .map(|t| t.elapsed() < Duration::from_secs(GENERATING_STALE_SECS))
            .unwrap_or(false)
    }

    async fn get(&self, key: &str, fingerprint: &str) -> Option<Arc<AssistantConn>> {
        let map = self.conns.lock().await;
        map.get(key)
            .filter(|c| c.fingerprint == fingerprint && !c.client.is_closed())
            .cloned()
    }

    async fn put(&self, key: String, conn: Arc<AssistantConn>) {
        let mut map = self.conns.lock().await;
        map.insert(key.clone(), conn);
        // 超出容量上限：按最近使用淘汰最久未用的连接（刚插入的 key 除外）
        while map.len() > self.max_conns {
            let lru = map
                .iter()
                .filter(|(k, _)| **k != key)
                .min_by_key(|(_, c)| *c.last_used.lock().unwrap_or_else(|e| e.into_inner()))
                .map(|(k, _)| k.clone());
            let Some(lru) = lru else { break };
            if let Some(old) = map.remove(&lru) {
                tracing::debug!("助理连接池已满，淘汰最久未用连接 {lru}");
                // kill 是异步的，但持有锁时也不能阻塞：丢给后台任务
                let client = old.client.clone();
                tokio::spawn(async move { client.kill().await });
            }
        }
    }

    async fn remove(&self, key: &str) {
        let old = self.conns.lock().await.remove(key);
        if let Some(old) = old {
            old.client.kill().await;
        }
    }

    /// 回收长时间未使用的连接，释放 CLI 进程。
    async fn evict_idle(&self, max_idle: Duration) {
        let mut map = self.conns.lock().await;
        let now = Instant::now();
        let stale: Vec<String> = map
            .iter()
            .filter(|(_, c)| {
                now.duration_since(*c.last_used.lock().unwrap_or_else(|e| e.into_inner()))
                    > max_idle
            })
            .map(|(k, _)| k.clone())
            .collect();
        let conns: Vec<_> = stale.iter().filter_map(|k| map.remove(k)).collect();
        drop(map);
        for c in conns {
            c.client.kill().await;
        }
    }
}

/// 后台回收空闲助理连接。
pub fn spawn_reaper(state: SharedState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            state
                .assistant_pool
                .evict_idle(Duration::from_secs(IDLE_EVICT_SECS))
                .await;
        }
    });
}

fn conn_fingerprint(
    agent: &Agent,
    cli: &CliEntry,
    model: &str,
    effort: &str,
    skills: &[crate::server::models::Skill],
    mcp: &[Value],
) -> String {
    let skill_ids: Vec<&str> = skills.iter().map(|s| s.id.as_str()).collect();
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        agent.id,
        agent.system_prompt,
        cli.id,
        cli.args,
        cli.env_template,
        model,
        effort,
        skill_ids.join(","),
        mcp.len()
    )
}

/// 建立一条常驻助理连接：启动 CLI、初始化、创建 ACP session，并挂上事件泵。
async fn create_conn(
    tx: &mpsc::UnboundedSender<String>,
    cli: &CliEntry,
    args: &[String],
    env: &[(String, String)],
    cwd: &std::path::Path,
    mcp: Vec<Value>,
    model: &str,
    effort: &str,
    policy: &str,
    fingerprint: String,
    context_hash: String,
) -> anyhow::Result<Arc<AssistantConn>> {
    let mut client = AcpClient::spawn(&cli.command, args, env, cwd).await?;
    let mut updates = client.take_updates();
    let mut requests = client.take_requests();
    sse_status(tx, &format!("已启动 {}，正在初始化 ACP…", cli.name));
    client.initialize().await?;
    sse_status(tx, "正在创建会话…");
    let session = client.new_session_full(cwd, mcp).await?;
    let acp_session_id = session
        .get("sessionId")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("session/new 未返回 sessionId"))?;
    crate::acp::apply_model_effort(&client, &session, &acp_session_id, model, effort, &cli.kind)
        .await;
    sse_status(tx, "会话已创建，正在等待模型响应…");

    let client = Arc::new(client);
    let (updates_tx, _) = broadcast::channel(1024);
    let btx = updates_tx.clone();
    tokio::spawn(async move {
        while let Some(u) = updates.recv().await {
            let _ = btx.send(u);
        }
    });

    let req_client = Arc::downgrade(&client);
    // 通知走会话广播通道：不能长期持有 SSE 发送端，否则响应流永远等不到关闭
    let notice_tx = updates_tx.clone();
    let notice_session = acp_session_id.clone();
    let policy = policy.to_string();
    tokio::spawn(async move {
        while let Some(req) = requests.recv().await {
            // 弱引用：连接被回收时请求泵自然结束，不阻止 AcpClient Drop 杀掉子进程
            let Some(req_client) = req_client.upgrade() else {
                break;
            };
            if req.method == "session/request_permission" {
                // 与 run 路径同一套策略：full_auto 全放行，只读自动放行，
                // 写操作按智能体审批策略处理（助理无审批通道 → 拒绝并提示）
                let decision = crate::runner::decide_permission(&policy, &req.params);
                if decision == "reject" {
                    let title = req
                        .params
                        .pointer("/toolCall/title")
                        .and_then(|t| t.as_str())
                        .unwrap_or("该操作");
                    let _ = notice_tx.send(json!({
                        "sessionId": notice_session,
                        "update": {
                            "sessionUpdate": "notice",
                            "text": format!("已阻止「{title}」：当前智能体审批策略不自动放行写操作，如需执行请先在智能体设置调整")
                        }
                    }));
                }
                let result = match crate::runner::pick_option(&req.params, &decision) {
                    Some(id) => json!({ "outcome": { "outcome": "selected", "optionId": id } }),
                    None => json!({ "outcome": { "outcome": "cancelled" } }),
                };
                let _ = req_client.respond(req.id, result).await;
            } else {
                let _ = req_client.respond(req.id, json!({})).await;
            }
        }
    });

    // Copilot 模型经 /model 命令设置（每个 ACP 会话一次）
    if cli.kind == "copilot" && !model.is_empty() {
        let _ = client
            .prompt(&acp_session_id, &format!("/model {model}"))
            .await;
    }

    Ok(Arc::new(AssistantConn {
        fingerprint,
        context_hash,
        client,
        acp_session_id,
        updates: updates_tx,
        last_used: std::sync::Mutex::new(Instant::now()),
        prompt_lock: Mutex::new(()),
    }))
}

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

/// 查询会话是否仍在生成（页面刷新/离开后回来继续等待结果）。
async fn session_status(
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
    let key = format!("{ws_id}:{sid}");
    Ok(HttpResponse::Ok().json(json!({ "generating": state.assistant_pool.is_generating(&key) })))
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
    // 同时回收该会话的常驻连接，避免 CLI 进程空转到空闲超时
    state.assistant_pool.remove(&format!("{ws_id}:{sid}")).await;
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

/// 向 SSE 客户端发送一条进度状态（只用于展示，不写入历史）。
fn sse_status(tx: &mpsc::UnboundedSender<String>, text: &str) {
    let _ = tx.send(format!(
        "data: {}\n\n",
        json!({ "type": "status", "text": text })
    ));
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
    let skills = agents::skills_for_agent(&state, &agent.id).await?;
    let mut prompt = format!(
        "你是 Fux 工作区助理，用简体中文回答，尽量简洁并使用 Markdown。\n\
         你可以通过 fux-workspace MCP 管理任务、项目、工作流、SKILL、智能体、任务类型、MCP、CLI 与成员；需要各类 ID 时先调用 workspace_overview，查看任务运行与节点状态用 get_task。\n\
         执行写操作前先说明将要做什么；删除、覆盖、发起工作流、邀请成员等影响较大的操作必须先征得用户确认。所有操作以当前用户身份执行，并受工作区权限与任务状态机约束。\n\n\
         ## 工作区上下文\n{context}\n"
    );
    if !skills.is_empty() {
        prompt.push_str("\n可用 SKILL（位于 .fux/skills/，按需阅读）:\n");
        for s in &skills {
            prompt.push_str(&format!("- {}\n", s.name));
        }
    }
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
    for s in &skills {
        let dir = cwd.join(".fux").join("skills").join(&s.name);
        tokio::fs::create_dir_all(&dir).await.ok();
        tokio::fs::write(dir.join("SKILL.md"), &s.content)
            .await
            .ok();
    }
    let model = agent.model.clone();
    let effort = agent.reasoning_effort.clone();
    let cli_name = cli.name.clone();

    let mut mcp = agents::mcp_configs(&state, &agent.id).await?;
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
    let mut env: Vec<(String, String)> = crate::server::secrets::cli_env_plain(&cli.env_template)
        .as_object()
        .map(|m| {
            m.into_iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default();
    crate::acp::tweak_env(&cli.kind, &mut env);
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
    let state2 = state.clone();
    let fingerprint = conn_fingerprint(&agent, &cli, &model, &effort, &skills, &mcp);
    let pool_key = format!("{ws_id}:{session_id}");
    let short_prompt = message.clone();
    let policy = agent.policy.clone();
    let ctx_hash = context_hash(&context);
    // 客户端断线信号：SSE 流被丢弃时经 DisconnectGuard 送达
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
    state.assistant_pool.mark_generating(&pool_key, true);
    tokio::spawn(async move {
        let answer = Arc::new(Mutex::new(String::new()));
        let process = Arc::new(std::sync::Mutex::new(ProcessState::default()));
        let handle_update =
            |u: &Value,
             tx: &mpsc::UnboundedSender<String>,
             answer: &Arc<Mutex<String>>,
             process: &Arc<std::sync::Mutex<ProcessState>>| {
                match u
                    .pointer("/update/sessionUpdate")
                    .and_then(|k| k.as_str())
                    .unwrap_or("")
                {
                    "agent_message_chunk" => {
                        if let Some(text) =
                            u.pointer("/update/content/text").and_then(|t| t.as_str())
                        {
                            let text = text.to_string();
                            if let Ok(mut guard) = answer.try_lock() {
                                guard.push_str(&text);
                            }
                            let _ = tx.send(format!(
                                "data: {}\n\n",
                                json!({ "type": "chunk", "text": text })
                            ));
                        }
                    }
                    "agent_thought_chunk" => {
                        if let Some(text) =
                            u.pointer("/update/content/text").and_then(|t| t.as_str())
                        {
                            if let Ok(mut p) = process.lock() {
                                p.thoughts.push_str(text);
                            }
                            let _ = tx.send(format!(
                                "data: {}\n\n",
                                json!({ "type": "thought", "text": text })
                            ));
                        }
                    }
                    "tool_call" => {
                        let id = u
                            .pointer("/update/toolCallId")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let title = u
                            .pointer("/update/title")
                            .and_then(|t| t.as_str())
                            .unwrap_or("工具");
                        let kind = u
                            .pointer("/update/kind")
                            .and_then(|v| v.as_str())
                            .unwrap_or("other");
                        let status = u
                            .pointer("/update/status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("running");
                        let detail = tool_detail(u);
                        if let Ok(mut p) = process.lock() {
                            upsert_tool(
                                &mut p.tools,
                                id,
                                Some(title),
                                Some(status),
                                Some(kind),
                                detail.as_deref(),
                            );
                        }
                        let _ = tx.send(format!(
                        "data: {}\n\n",
                        json!({ "type": "tool", "id": id, "title": title, "status": status, "kind": kind, "detail": detail })
                    ));
                    }
                    "tool_call_update" => {
                        let id = u
                            .pointer("/update/toolCallId")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let title = u.pointer("/update/title").and_then(|t| t.as_str());
                        let kind = u.pointer("/update/kind").and_then(|v| v.as_str());
                        let status = u.pointer("/update/status").and_then(|v| v.as_str());
                        let detail = tool_detail(u);
                        if let Ok(mut p) = process.lock() {
                            upsert_tool(&mut p.tools, id, title, status, kind, detail.as_deref());
                        }
                        let _ = tx.send(format!(
                        "data: {}\n\n",
                        json!({ "type": "tool", "id": id, "title": title, "status": status, "kind": kind, "detail": detail })
                    ));
                    }
                    "plan" => sse_status(tx, "已更新执行计划"),
                    // 权限被策略阻止时的提示（由连接权限泵经广播通道送入）
                    "notice" => {
                        if let Some(text) = u.pointer("/update/text").and_then(|t| t.as_str()) {
                            sse_status(tx, text);
                        }
                    }
                    _ => {}
                }
            };
        let result: anyhow::Result<()> = async {
            let (conn, is_new) = match state2.assistant_pool.get(&pool_key, &fingerprint).await {
                Some(c) => (c, false),
                None => {
                    let c = create_conn(
                        &tx, &cli, &args, &env, &cwd, mcp, &model, &effort, &policy,
                        fingerprint.clone(), ctx_hash.clone(),
                    )
                    .await?;
                    state2.assistant_pool.put(pool_key.clone(), c.clone()).await;
                    (c, true)
                }
            };
            let _guard = conn.prompt_lock.lock().await;
            let mut sub = conn.updates.subscribe();
            if !is_new {
                sse_status(&tx, "复用已有会话，正在等待模型响应…");
            }
            // 复用时若工作区上下文有更新，把最新上下文随消息补入（新会话已含在 full_prompt 里）
            let prompt_text: String = if is_new {
                full_prompt.clone()
            } else if conn.context_hash != ctx_hash {
                format!("{context}\n\n{short_prompt}")
            } else {
                short_prompt.clone()
            };
            let mut cancel_rx = cancel_rx;
            let resp = tokio::time::timeout(
                std::time::Duration::from_secs(PROMPT_TIMEOUT_SECS),
                async {
                    let fut = conn.client.prompt(&conn.acp_session_id, &prompt_text);
                    tokio::pin!(fut);
                    let mut waited = 0u64;
                    let mut tick = tokio::time::interval_at(
                        tokio::time::Instant::now() + std::time::Duration::from_secs(10),
                        std::time::Duration::from_secs(10),
                    );
                    loop {
                        tokio::select! {
                            r = &mut fut => break r,
                            recv = sub.recv() => match recv {
                                Ok(u) => {
                                    if u.get("sessionId").and_then(|v| v.as_str()) == Some(conn.acp_session_id.as_str()) {
                                        handle_update(&u, &tx, &answer, &process);
                                    }
                                }
                                Err(broadcast::error::RecvError::Lagged(_)) => {}
                                Err(broadcast::error::RecvError::Closed) => {}
                            },
                            _ = tick.tick() => {
                                waited += 10;
                                sse_status(&tx, &format!("等待 {cli_name} 响应（已 {waited}s）…"));
                            }
                            // 客户端断开：取消会话并作废连接（ACP 会话被中途取消，状态不可信）
                            _ = &mut cancel_rx => {
                                sse_status(&tx, "连接已断开，正在取消本次生成…");
                                let _ = conn.client.cancel(&conn.acp_session_id).await;
                                state2.assistant_pool.remove(&pool_key).await;
                                break Err(anyhow::anyhow!("客户端断开连接，已取消生成"));
                            }
                        }
                    }
                },
            )
            .await;
            let resp = match resp {
                Ok(r) => r,
                Err(_) => {
                    // CLI 僵死（不退出也不响应）：取消并作废连接，避免 prompt_lock 被永久持有
                    sse_status(&tx, &format!("等待超过 {PROMPT_TIMEOUT_SECS}s，已强制取消本次生成"));
                    let _ = conn.client.cancel(&conn.acp_session_id).await;
                    state2.assistant_pool.remove(&pool_key).await;
                    Err(anyhow::anyhow!("生成超时（{PROMPT_TIMEOUT_SECS}s），已取消"))
                }
            };
            while let Ok(u) = sub.try_recv() {
                if u.get("sessionId").and_then(|v| v.as_str()) == Some(conn.acp_session_id.as_str()) {
                    handle_update(&u, &tx, &answer, &process);
                }
            }
            *conn.last_used.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
            if resp.is_err() && conn.client.is_closed() {
                state2.assistant_pool.remove(&pool_key).await;
            }
            resp?;
            Ok(())
        }.await;
        if let Err(e) = &result {
            tracing::warn!("助理生成失败（会话 {sid2}）: {e:#}");
        }
        state2.assistant_pool.mark_generating(&pool_key, false);
        let process_content = process.lock().ok().and_then(|p| {
            if p.thoughts.trim().is_empty() && p.tools.is_empty() {
                None
            } else {
                Some(json!({ "thoughts": p.thoughts, "tools": p.tools }).to_string())
            }
        });
        if let Some(content) = process_content {
            let _ = sqlx::query("INSERT INTO assistant_messages (workspace_id, session_id, role, content) VALUES (?,?, 'process', ?)")
                .bind(&ws_id2)
                .bind(&sid2)
                .bind(&content)
                .execute(&db).await;
        }
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
        // 流随响应返回：客户端断线/关闭页面时本生成器被丢弃，guard 触发取消
        let _disconnect = DisconnectGuard(Some(cancel_tx));
        while let Some(line) = rx.recv().await {
            // 终态事件后主动结束：不依赖发送端全部 drop（连接上还有长期存活的任务）
            let terminal = line.contains("\"type\":\"done\"") || line.contains("\"type\":\"error\"");
            yield Ok::<_, actix_web::Error>(actix_web::web::Bytes::from(line));
            if terminal { break; }
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
    .route(
        "/assistant/sessions/{id}/status",
        web::get().to(session_status),
    )
    .route("/assistant/sessions/{id}", web::patch().to(rename_session))
    .route("/assistant/sessions/{id}", web::delete().to(delete_session))
    .route("/assistant/chat", web::post().to(chat));
}
