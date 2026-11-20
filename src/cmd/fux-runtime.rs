//! fux-runtime — 节点执行器：安装即注册（一次性配对码 + 出站 WS，免复杂配置）。
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use fux::git;
use fux::proto::{Capabilities, RuntimeMsg, ServerMsg, probe_clis};
use fux::runner::{RunOutcome, RunSink, RunSpec};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc, oneshot};

#[derive(Parser)]
#[command(name = "fux-runtime", about = "Fux 节点执行器：安装即注册")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Subcommand)]
enum Cmd {
    /// 配对并加入控制面
    Join {
        /// 控制面地址，如 http://192.168.1.10:8080
        server: String,
        /// 一次性配对码（控制面节点页生成）
        #[arg(long)]
        code: String,
        #[arg(long, default_value = "fux-node")]
        name: String,
        #[arg(long, default_value = "fux-data")]
        data_dir: String,
    },
}

type Perms = Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>;
type Cancels = Arc<Mutex<HashMap<String, mpsc::UnboundedSender<()>>>>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let Cmd::Join {
        server,
        code,
        name,
        data_dir,
    } = Cli::parse().cmd;
    let server = server.trim_end_matches('/').to_string();
    let data_dir = PathBuf::from(data_dir);
    tokio::fs::create_dir_all(&data_dir).await?;

    let caps = Capabilities {
        os: std::env::consts::OS.to_string(),
        clis: probe_clis(),
        project_paths: vec![],
        load: 0,
    };
    println!("探测到 CLI: {:?}", caps.clis);

    // 1. 配对（REST，一次性码）
    let resp = reqwest::Client::new()
        .post(format!("{server}/api/v1/nodes/pair"))
        .json(&json!({ "code": code, "name": name, "capabilities": caps }))
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("配对失败: {}", resp.text().await?);
    }
    let v: Value = resp.json().await?;
    let token = v["token"].as_str().unwrap_or_default().to_string();
    let node_id = v["nodeId"].as_str().unwrap_or_default().to_string();
    println!("配对成功，节点 {node_id}，等待 admin 批准…");

    let mut reconnect_attempt = 0u32;
    loop {
        match run_connection(&server, &token, &data_dir).await {
            Ok(true) => reconnect_attempt = 0,
            Ok(false) => {
                println!("节点令牌已失效，runtime 退出；如需重新加入请使用新配对码。");
                return Ok(());
            }
            Err(e) => eprintln!("连接控制面失败: {e:#}"),
        }
        let delay = reconnect_delay(reconnect_attempt);
        println!("{delay} 秒后自动重连…");
        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
        reconnect_attempt = reconnect_attempt.saturating_add(1);
    }
}

fn reconnect_delay(attempt: u32) -> u64 {
    30.min(1u64 << attempt.min(5))
}

/// 维持一次节点通道；返回 false 表示节点已删除，不应继续重连。
async fn run_connection(server: &str, token: &str, data_dir: &PathBuf) -> anyhow::Result<bool> {
    // 2. 出站 WS 长连接
    let ws_url = format!(
        "{}/api/v1/node-channel?token={token}",
        server
            .replace("http://", "ws://")
            .replace("https://", "wss://")
    );
    let (ws, _) = tokio_tungstenite::connect_async(&ws_url).await?;
    let (mut ws_sink, mut ws_stream) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<RuntimeMsg>();

    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let text = match serde_json::to_string(&msg) {
                Ok(text) => text,
                Err(_) => continue,
            };
            if ws_sink
                .send(tokio_tungstenite::tungstenite::Message::Text(text.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    let perms: Perms = Arc::new(Mutex::new(HashMap::new()));
    let cancels: Cancels = Arc::new(Mutex::new(HashMap::new()));
    let active = Arc::new(std::sync::atomic::AtomicU32::new(0));

    // 心跳每 10 秒续约；能力在连接建立时及此后每分钟重新探测。
    let heartbeat = {
        let tx = tx.clone();
        let active = active.clone();
        tokio::spawn(async move {
            let mut tick = 0u64;
            loop {
                let load = active.load(std::sync::atomic::Ordering::Relaxed);
                let _ = tx.send(RuntimeMsg::Heartbeat { load });
                if tick % 6 == 0 {
                    let capabilities = Capabilities {
                        os: std::env::consts::OS.to_string(),
                        clis: probe_clis(),
                        project_paths: vec![],
                        load,
                    };
                    let _ = tx.send(RuntimeMsg::Capabilities { capabilities });
                }
                tick = tick.wrapping_add(1);
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        })
    };

    println!("已连接控制面，等待派发…");
    let mut removed = false;
    while let Some(Ok(msg)) = ws_stream.next().await {
        let text = match msg {
            tokio_tungstenite::tungstenite::Message::Text(t) => t.to_string(),
            tokio_tungstenite::tungstenite::Message::Ping(b) => {
                let _ = tx.send(RuntimeMsg::Pong);
                let _ = b;
                continue;
            }
            tokio_tungstenite::tungstenite::Message::Close(_) => break,
            _ => continue,
        };
        let msg: ServerMsg = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(_) => continue,
        };
        match msg {
            ServerMsg::Hello { approved, .. } => {
                if approved {
                    println!("节点已批准，可调度。");
                }
            }
            ServerMsg::Approval { approved } => {
                if approved {
                    println!("admin 已批准，节点上线。");
                } else {
                    println!("节点已被控制面移除，停止当前连接。");
                    removed = true;
                    break;
                }
            }
            ServerMsg::Dispatch { spec } => {
                let tx = tx.clone();
                let perms = perms.clone();
                let cancels = cancels.clone();
                let data_dir = data_dir.clone();
                let active = active.clone();
                active.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                tokio::spawn(async move {
                    execute_run(spec, tx, perms, cancels, data_dir).await;
                    active.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                });
            }
            ServerMsg::Cancel { run_id } => {
                let map = cancels.lock().await;
                if let Some(c) = map.get(&run_id) {
                    let _ = c.send(());
                }
            }
            ServerMsg::Permission {
                request_id,
                decision,
                ..
            } => {
                let mut map = perms.lock().await;
                if let Some(p) = map.remove(&request_id) {
                    let _ = p.send(decision);
                }
            }
            ServerMsg::Ping => {
                let _ = tx.send(RuntimeMsg::Pong);
            }
        }
    }
    writer.abort();
    heartbeat.abort();
    perms.lock().await.clear();
    let mut running = cancels.lock().await;
    for (_, cancel) in running.drain() {
        let _ = cancel.send(());
    }
    println!("与控制面断开。");
    Ok(!removed)
}

async fn prepare_workdir(data_dir: &PathBuf, spec: &RunSpec) -> anyhow::Result<PathBuf> {
    let wt = if spec.stage_key.is_empty() {
        git::worktree_dir(data_dir, &spec.task_id)
    } else {
        git::stage_worktree_dir(data_dir, &spec.task_id, &spec.stage_key)
    };
    let base = spec
        .base_branch
        .clone()
        .or_else(|| spec.project.as_ref().map(|p| p.default_branch.clone()))
        .unwrap_or_else(|| "main".into());
    match &spec.project {
        Some(p) if p.kind == "git" => {
            let repo = git::checkout_dir(data_dir, &p.project_id);
            git::ensure_clone(p.remote_url.as_deref().unwrap_or(""), &repo).await?;
            git::prepare_worktree(&repo, &wt, &spec.branch, &base).await?;
        }
        Some(p) => {
            let lp = std::path::Path::new(p.local_path.as_deref().unwrap_or(""));
            if git::is_repo(lp).await {
                git::prepare_worktree(lp, &wt, &spec.branch, &base).await?;
            } else if !wt.exists() {
                git::copy_dir_recursive(lp, &wt)?;
                git::init_repo(&wt).await?;
                git::commit_all(&wt, "fux: baseline").await.ok();
            }
        }
        None => {
            tokio::fs::create_dir_all(&wt).await?;
            if !git::is_repo(&wt).await {
                git::init_repo(&wt).await?;
            }
        }
    }
    git::materialize_upstreams(&wt, &spec.upstream).await.ok();
    Ok(wt)
}

struct RemoteSink {
    tx: mpsc::UnboundedSender<RuntimeMsg>,
    run_id: String,
    perms: Perms,
    /// 当前 ACP 会话 id：session_started 时写入，心跳/结束时上报
    session: tokio::sync::Mutex<String>,
}

#[async_trait::async_trait]
impl RunSink for RemoteSink {
    async fn emit(&self, kind: &str, payload: Value) {
        let _ = self.tx.send(RuntimeMsg::Event {
            run_id: self.run_id.clone(),
            kind: kind.to_string(),
            payload,
        });
    }
    async fn permission(&self, request: Value) -> String {
        let request_id = fux::new_id();
        let (otx, orx) = oneshot::channel();
        self.perms.lock().await.insert(request_id.clone(), otx);
        let _ = self.tx.send(RuntimeMsg::PermissionRequest {
            run_id: self.run_id.clone(),
            request_id: request_id.clone(),
            request,
        });
        match tokio::time::timeout(std::time::Duration::from_secs(130), orx).await {
            Ok(Ok(d)) => d,
            _ => {
                self.perms.lock().await.remove(&request_id);
                "reject".into()
            }
        }
    }
    async fn heartbeat(&self, turn_count: u32) {
        let _ = self.tx.send(RuntimeMsg::RunState {
            run_id: self.run_id.clone(),
            status: "running".into(),
            stop_reason: String::new(),
            turn_count,
            session_id: self.session.lock().await.clone(),
        });
    }

    /// 会话建立即上报，控制面据此把 acp_session_id 写进 runs（重试时可续接）。
    async fn session_started(&self, session_id: &str) {
        *self.session.lock().await = session_id.to_string();
        let _ = self.tx.send(RuntimeMsg::RunState {
            run_id: self.run_id.clone(),
            status: "running".into(),
            stop_reason: String::new(),
            turn_count: 0,
            session_id: session_id.to_string(),
        });
    }
    async fn pending_guidance(&self) -> Vec<String> {
        // 远程运行的引导评论在派发时注入（v1）
        vec![]
    }
}

async fn execute_run(
    spec: RunSpec,
    tx: mpsc::UnboundedSender<RuntimeMsg>,
    perms: Perms,
    cancels: Cancels,
    data_dir: PathBuf,
) {
    let run_id = spec.run_id.clone();
    let workdir = match prepare_workdir(&data_dir, &spec).await {
        Ok(w) => w,
        Err(e) => {
            let _ = tx.send(RuntimeMsg::Finish {
                run_id,
                status: "failed".into(),
                stop_reason: "interrupted".into(),
                turn_count: 0,
                session_id: String::new(),
                pushed: false,
                head: None,
                error: Some(format!("准备工作区失败: {e:#}")),
            });
            return;
        }
    };
    let mut spec = spec;
    spec.workdir = workdir.clone();
    let (cancel_tx, cancel_rx) = mpsc::unbounded_channel();
    cancels.lock().await.insert(run_id.clone(), cancel_tx);
    let sink = Arc::new(RemoteSink {
        tx: tx.clone(),
        run_id: run_id.clone(),
        perms,
        session: tokio::sync::Mutex::new(String::new()),
    });
    let outcome: RunOutcome = fux::runner::execute(spec.clone(), sink, cancel_rx).await;
    cancels.lock().await.remove(&run_id);

    // 完成 → commit + push（git 原生交付）
    let mut pushed = false;
    let mut head = None;
    let mut error = outcome.error.clone();
    if outcome.status == "finished" {
        match git::commit_all(&workdir, &format!("fux(task): {}", spec.title)).await {
            Ok(h) => {
                head = h;
                let has_remote = spec
                    .project
                    .as_ref()
                    .and_then(|p| p.remote_url.as_deref())
                    .map(|r| !r.is_empty())
                    .unwrap_or(false)
                    || git::has_remote(&workdir).await;
                if has_remote {
                    match git::push(&workdir, &spec.branch).await {
                        Ok(_) => pushed = true,
                        Err(e) => {
                            error = Some(format!("git push 失败: {e:#}"));
                        }
                    }
                }
            }
            Err(e) => error = Some(format!("git commit 失败: {e:#}")),
        }
        // 无新提交时仍上报分支 HEAD，供下游节点引用产物
        if head.is_none() && error.is_none() {
            head = git::git(&workdir, &["rev-parse", "HEAD"]).await.ok();
        }
    }
    let status = if outcome.status == "finished" && error.is_some() {
        "failed".to_string()
    } else {
        outcome.status.clone()
    };
    let stop_reason = if status == "failed" && outcome.status == "finished" {
        "interrupted".to_string()
    } else {
        outcome.stop_reason.clone()
    };
    let _ = tx.send(RuntimeMsg::Finish {
        run_id,
        status,
        stop_reason,
        turn_count: outcome.turn_count,
        session_id: outcome.session_id.clone(),
        pushed,
        head,
        error,
    });
}
