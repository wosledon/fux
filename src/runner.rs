//! 通用运行器：驱动一次「智能体 × 任务」的 ACP 会话。
//! 控制面本地执行与远程 runtime 共用此逻辑，差异通过 RunSink 抽象。
use crate::acp::AcpClient;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use tokio::sync::{Notify, mpsc};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSpec {
    pub project_id: String,
    pub kind: String,
    pub remote_url: Option<String>,
    pub local_path: Option<String>,
    pub default_branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamSpec {
    /// 上游节点 key
    pub key: String,
    /// 项目名（展示用）
    pub name: String,
    /// 可 clone 的地址（remote URL 或本地路径）
    pub remote: String,
    pub branch: String,
    pub sha: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSpec {
    pub run_id: String,
    pub task_id: String,
    pub title: String,
    pub content: String,
    pub workdir: PathBuf,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cli_kind: String,
    pub model: String,
    pub effort: String,
    pub system_prompt: String,
    pub skills: Vec<(String, String)>,
    pub mcp_servers: Vec<Value>,
    /// read_auto / write_ask / full_auto
    pub policy: String,
    pub guidance: Vec<String>,
    pub context_summary: String,
    /// 远程节点据此自行检出仓库/准备 worktree
    pub project: Option<ProjectSpec>,
    pub branch: String,
    /// 分支起点（工作流：同项目上游分支）；None 用项目默认分支
    #[serde(default)]
    pub base_branch: Option<String>,
    /// 工作流：当前节点 key / 名称（非工作流为空）
    #[serde(default)]
    pub stage_key: String,
    #[serde(default)]
    pub stage_name: String,
    /// 需要检出到 .fux/upstream/<key> 的上游产物
    #[serde(default)]
    pub upstream: Vec<UpstreamSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunOutcome {
    /// finished / failed / cancelled
    pub status: String,
    pub stop_reason: String,
    pub turn_count: u32,
    pub session_id: String,
    pub error: Option<String>,
}

#[async_trait]
pub trait RunSink: Send + Sync {
    async fn emit(&self, kind: &str, payload: Value);
    /// 权限决策：allow_once / allow_always / reject
    async fn permission(&self, request: Value) -> String;
    async fn heartbeat(&self, turn_count: u32);
    /// turn 结束后待注入的引导评论（决策 #7）
    async fn pending_guidance(&self) -> Vec<String>;
}

/// 推理强度映射（Copilot 启动参数接受 low/medium/high/xhigh/max）。
pub fn map_effort(effort: &str) -> String {
    match effort {
        "minimal" => "low".to_string(),
        "xhigh" | "max" => effort.to_string(),
        other => other.to_string(),
    }
}

/// Run 存续期间维持租约；Drop 时停止后台心跳。
struct HeartbeatGuard(tokio::task::JoinHandle<()>);
impl Drop for HeartbeatGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn pick_option(params: &Value, decision: &str) -> Option<String> {
    let want = match decision {
        "allow_once" => "allow_once",
        "allow_always" => "allow_always",
        _ => "reject_once",
    };
    let options = params.get("options")?.as_array()?;
    // 优先精确匹配 kind；reject 时兼容 reject_once
    for o in options {
        if o.get("kind").and_then(|k| k.as_str()) == Some(want) {
            return o.get("optionId").and_then(|v| v.as_str()).map(String::from);
        }
    }
    if want == "reject_once" {
        for o in options {
            let k = o.get("kind").and_then(|k| k.as_str()).unwrap_or("");
            if k.starts_with("reject") {
                return o.get("optionId").and_then(|v| v.as_str()).map(String::from);
            }
        }
    }
    None
}

async fn map_update(sink: &Arc<dyn RunSink>, u: &Value) {
    // ACP session/update 通知：更新体位于 params.update
    let u = u.get("update").unwrap_or(u);
    let kind = u
        .get("sessionUpdate")
        .and_then(|k| k.as_str())
        .unwrap_or("");
    match kind {
        "agent_message_chunk" => {
            let text = u
                .pointer("/content/text")
                .and_then(|t| t.as_str())
                .unwrap_or("");
            if !text.is_empty() {
                sink.emit("agent.message", json!({ "text": text })).await;
            }
        }
        "agent_thought_chunk" => {
            let text = u
                .pointer("/content/text")
                .and_then(|t| t.as_str())
                .unwrap_or("");
            if !text.is_empty() {
                sink.emit("agent.thought", json!({ "text": text })).await;
            }
        }
        "tool_call" => {
            sink.emit(
                "tool.call",
                json!({
                    "toolCallId": u.get("toolCallId"),
                    "title": u.get("title"),
                    "name": u.get("name"),
                    "kind": u.get("kind"),
                    "rawInput": u.get("rawInput"),
                }),
            )
            .await;
        }
        "tool_call_update" => {
            sink.emit(
                "tool.result",
                json!({
                    "toolCallId": u.get("toolCallId"),
                    "status": u.get("status"),
                    "content": u.get("content"),
                }),
            )
            .await;
        }
        "plan" => {
            sink.emit("plan.updated", json!({ "entries": u.get("entries") }))
                .await;
        }
        "usage_update" => {
            sink.emit(
                "usage.updated",
                json!({ "used": u.get("used"), "size": u.get("size") }),
            )
            .await;
        }
        _ => {}
    }
}

fn build_prompt(spec: &RunSpec, guidance: &[String], summary: &str, first: bool) -> String {
    let mut p = String::new();
    if !spec.system_prompt.is_empty() {
        p.push_str(&spec.system_prompt);
        p.push_str("\n\n");
    }
    if first {
        p.push_str(&format!("# 任务: {}\n\n{}\n", spec.title, spec.content));
        if !spec.skills.is_empty() {
            p.push_str("\n可用 SKILL（位于 .fux/skills/，按需阅读）:\n");
            for (name, _) in &spec.skills {
                p.push_str(&format!("- {name}\n"));
            }
        }
        if !summary.is_empty() {
            p.push_str(&format!("\n上一轮工作摘要:\n{summary}\n"));
        }
    } else {
        p.push_str(&format!("继续任务「{}」。\n", spec.title));
    }
    if !guidance.is_empty() {
        p.push_str("\n用户最新引导评论（请据此调整工作）:\n");
        for g in guidance {
            p.push_str(&format!("- {g}\n"));
        }
    }
    p.push_str("\n完成后请确保变更已提交到当前 git 分支（fux 任务分支）。");
    p
}

pub async fn execute(
    spec: RunSpec,
    sink: Arc<dyn RunSink>,
    cancel_rx: mpsc::UnboundedReceiver<()>,
) -> RunOutcome {
    match execute_inner(spec, sink, cancel_rx).await {
        Ok(o) => o,
        Err(e) => RunOutcome {
            status: "failed".into(),
            stop_reason: "interrupted".into(),
            turn_count: 0,
            session_id: String::new(),
            error: Some(format!("{e:#}")),
        },
    }
}

async fn execute_inner(
    spec: RunSpec,
    sink: Arc<dyn RunSink>,
    mut cancel_rx: mpsc::UnboundedReceiver<()>,
) -> Result<RunOutcome> {
    // SKILL 注入：写入会话工作目录
    for (name, content) in &spec.skills {
        let dir = spec.workdir.join(".fux").join("skills").join(name);
        tokio::fs::create_dir_all(&dir).await.ok();
        tokio::fs::write(dir.join("SKILL.md"), content).await.ok();
    }

    // 独立于 turn 的租约心跳：单次 prompt 可能持续几十秒，不能只在轮次之间续租
    let turn_counter = Arc::new(AtomicU32::new(0));
    let _heartbeat = HeartbeatGuard(tokio::spawn({
        let sink = sink.clone();
        let turn_counter = turn_counter.clone();
        async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(10));
            tick.tick().await; // 消费 interval 的立即首 tick
            loop {
                sink.heartbeat(turn_counter.load(Ordering::Relaxed)).await;
                tick.tick().await;
            }
        }
    }));

    let mut args = spec.args.clone();
    if spec.cli_kind == "copilot" {
        args.push(format!("--effort={}", map_effort(&spec.effort)));
    }

    let mut env = spec.env.clone();
    crate::acp::tweak_env(&spec.cli_kind, &mut env);
    let mut client = AcpClient::spawn(&spec.command, &args, &env, &spec.workdir).await?;
    let mut updates_rx = client.take_updates();
    let mut requests_rx = client.take_requests();
    client.initialize().await?;
    let session = client
        .new_session_full(&spec.workdir, spec.mcp_servers.clone())
        .await?;
    let session_id = session
        .get("sessionId")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("session/new 未返回 sessionId"))?;

    // 模型 / 推理强度：按会话声明的配置项透传（兼容旧版固定 id）
    crate::acp::apply_model_effort(
        &client,
        &session,
        &session_id,
        &spec.model,
        &spec.effort,
        &spec.cli_kind,
    )
    .await;

    let client = Arc::new(client);
    // 取消信号：pump 收到后先发 ACP session/cancel，主循环最多再等 15 秒，
    // 超时直接结束会话（client 丢弃时 kill_on_drop 会终止子进程），防止被取消的 Run 继续烧额度。
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_notify = Arc::new(Notify::new());
    // pump 只处理 agent→client 请求（权限）与取消信号；
    // session/update 事件在等待 prompt 期间内联消费，避免 abort 竞态丢事件。
    let pump = {
        let client = client.clone();
        let sink = sink.clone();
        let session_id = session_id.clone();
        let cancel_flag = cancel_flag.clone();
        let cancel_notify = cancel_notify.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(req) = requests_rx.recv() => {
                        if req.method == "session/request_permission" {
                            let decision = sink.permission(req.params.clone()).await;
                            let result = match pick_option(&req.params, &decision) {
                                Some(id) => json!({ "outcome": { "outcome": "selected", "optionId": id } }),
                                None => json!({ "outcome": { "outcome": "cancelled" } }),
                            };
                            let _ = client.respond(req.id, result).await;
                        } else {
                            let _ = client.respond(req.id, json!({})).await;
                        }
                    }
                    Some(_) = cancel_rx.recv() => {
                        cancel_flag.store(true, Ordering::SeqCst);
                        cancel_notify.notify_waiters();
                        let _ = client.cancel(&session_id).await;
                    }
                    else => break,
                }
            }
        })
    };

    // Copilot 模型经 /model 命令设置（作为 prompt 下发，不触发模型调用）
    if spec.cli_kind == "copilot" && !spec.model.is_empty() {
        let _ = client
            .prompt(&session_id, &format!("/model {}", spec.model))
            .await;
    }

    let mut turn_count = 0u32;
    let mut guidance = spec.guidance.clone();
    let mut first = true;
    let outcome = loop {
        turn_count += 1;
        turn_counter.store(turn_count, Ordering::Relaxed);
        sink.heartbeat(turn_count).await;
        let text = build_prompt(&spec, &guidance, &spec.context_summary, first);
        first = false;
        let mut forced_cancel = false;
        // 等待 prompt 的同时内联消费事件流（保证事件先于响应落库）
        let resp = {
            let fut = client.prompt(&session_id, &text);
            tokio::pin!(fut);
            let mut cancel_deadline: Option<tokio::time::Instant> = None;
            loop {
                if cancel_flag.load(Ordering::Relaxed) && cancel_deadline.is_none() {
                    cancel_deadline =
                        Some(tokio::time::Instant::now() + std::time::Duration::from_secs(15));
                }
                let deadline = cancel_deadline;
                tokio::select! {
                    r = &mut fut => break r,
                    Some(u) = updates_rx.recv() => map_update(&sink, &u).await,
                    // 收到取消通知后回到循环头部设置截止时间
                    _ = cancel_notify.notified() => {}
                    // 宽限期内 agent 未完事，强制结束
                    _ = async {
                        if let Some(d) = deadline {
                            tokio::time::sleep_until(d).await;
                        }
                    }, if deadline.is_some() => {
                        forced_cancel = true;
                        break Err(anyhow!("取消超时，已强制结束会话"));
                    }
                }
            }
        };
        match resp {
            Err(e) => {
                if forced_cancel {
                    break RunOutcome {
                        status: "cancelled".into(),
                        stop_reason: "cancelled".into(),
                        turn_count,
                        session_id: session_id.clone(),
                        error: None,
                    };
                }
                break RunOutcome {
                    status: "failed".into(),
                    stop_reason: "interrupted".into(),
                    turn_count,
                    session_id: session_id.clone(),
                    error: Some(format!("{e:#}")),
                };
            }
            Ok(resp) => {
                if let Some(usage) = resp.get("usage") {
                    sink.emit("usage.updated", json!({ "usage": usage })).await;
                }
                let stop = resp
                    .get("stopReason")
                    .and_then(|s| s.as_str())
                    .unwrap_or("end_turn");
                match stop {
                    "end_turn" => {
                        // 决策 #7：turn 结束后注入排队的引导评论
                        let pending = sink.pending_guidance().await;
                        if !pending.is_empty() {
                            guidance = pending;
                            continue;
                        }
                        break RunOutcome {
                            status: "finished".into(),
                            stop_reason: "end_turn".into(),
                            turn_count,
                            session_id: session_id.clone(),
                            error: None,
                        };
                    }
                    "cancelled" => {
                        break RunOutcome {
                            status: "cancelled".into(),
                            stop_reason: "cancelled".into(),
                            turn_count,
                            session_id: session_id.clone(),
                            error: None,
                        };
                    }
                    other => {
                        break RunOutcome {
                            status: "failed".into(),
                            stop_reason: other.to_string(),
                            turn_count,
                            session_id: session_id.clone(),
                            error: Some(format!("stopReason={other}")),
                        };
                    }
                }
            }
        }
    };

    // 排空响应前已入队但尚未处理的事件（reader 按序解析，通知先于响应）
    while let Ok(u) = updates_rx.try_recv() {
        map_update(&sink, &u).await;
    }
    client.kill().await;
    pump.abort();
    Ok(outcome)
}
