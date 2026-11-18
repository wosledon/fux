//! ACP (Agent Client Protocol) client：JSON-RPC 2.0 over stdio，NDJSON  framing。
//! Fux 作为 client，拉起 CLI agent 子进程（Codex/OpenCode/Pi/Copilot 等）。
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

/// agent → client 的请求（如 session/request_permission）
pub struct IncomingRequest {
    pub id: i64,
    pub method: String,
    pub params: Value,
}

pub struct AcpClient {
    stdin: Arc<Mutex<ChildStdin>>,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<std::result::Result<Value, String>>>>>,
    next_id: Arc<AtomicI64>,
    closed: Arc<AtomicBool>,
    child: Arc<Mutex<Child>>,
    reader_handle: tokio::task::JoinHandle<()>,
    updates_rx: Option<mpsc::UnboundedReceiver<Value>>,
    requests_rx: Option<mpsc::UnboundedReceiver<IncomingRequest>>,
}

impl AcpClient {
    pub async fn spawn(
        command: &str,
        args: &[String],
        envs: &[(String, String)],
        cwd: &Path,
    ) -> Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let mut child = cmd
            .spawn()
            .with_context(|| format!("无法启动 CLI: {command}（请确认已安装并在 PATH 中）"))?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
        let stderr = child.stderr.take();

        // stderr 单独走日志，不污染协议流
        if let Some(stderr) = stderr {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(target: "acp_stderr", "{line}");
                }
            });
        }

        let pending: Arc<Mutex<HashMap<i64, oneshot::Sender<std::result::Result<Value, String>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let (updates_tx, updates_rx) = mpsc::unbounded_channel();
        let (requests_tx, requests_rx) = mpsc::unbounded_channel();

        let reader_handle = {
            let pending = pending.clone();
            let closed = closed.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                loop {
                    match lines.next_line().await {
                        Ok(Some(line)) => {
                            if line.trim().is_empty() {
                                continue;
                            }
                            let msg: Value = match serde_json::from_str(&line) {
                                Ok(v) => v,
                                Err(e) => {
                                    tracing::warn!("ACP 非法输出行: {e}: {line}");
                                    continue;
                                }
                            };
                            if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
                                if let Some(id) = msg.get("id").and_then(|i| i.as_i64()) {
                                    let _ = requests_tx.send(IncomingRequest {
                                        id,
                                        method: method.to_string(),
                                        params: msg.get("params").cloned().unwrap_or(json!({})),
                                    });
                                } else if method == "session/update" {
                                    let _ = updates_tx
                                        .send(msg.get("params").cloned().unwrap_or(json!({})));
                                }
                            } else if let Some(id) = msg.get("id").and_then(|i| i.as_i64()) {
                                let mut map = pending.lock().await;
                                if let Some(tx) = map.remove(&id) {
                                    if let Some(err) = msg.get("error") {
                                        let _ = tx.send(Err(err
                                            .get("message")
                                            .and_then(|m| m.as_str())
                                            .unwrap_or("unknown error")
                                            .to_string()));
                                    } else {
                                        let _ =
                                            tx.send(Ok(msg.get("result").cloned().unwrap_or(json!({}))));
                                    }
                                }
                            }
                        }
                        _ => break,
                    }
                }
                closed.store(true, Ordering::SeqCst);
                // 唤醒所有等待中的请求
                let mut map = pending.lock().await;
                for (_, tx) in map.drain() {
                    let _ = tx.send(Err("ACP 进程输出流已关闭（进程退出或崩溃）".into()));
                }
            })
        };

        Ok(Self {
            stdin: Arc::new(Mutex::new(stdin)),
            pending,
            next_id: Arc::new(AtomicI64::new(1)),
            closed,
            child: Arc::new(Mutex::new(child)),
            reader_handle,
            updates_rx: Some(updates_rx),
            requests_rx: Some(requests_rx),
        })
    }

    pub fn take_updates(&mut self) -> mpsc::UnboundedReceiver<Value> {
        self.updates_rx.take().expect("updates already taken")
    }

    pub fn take_requests(&mut self) -> mpsc::UnboundedReceiver<IncomingRequest> {
        self.requests_rx.take().expect("requests already taken")
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    async fn write(&self, mut msg: Value) -> Result<()> {
        msg["jsonrpc"] = json!("2.0");
        let line = format!("{}\n", serde_json::to_string(&msg)?);
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(line.as_bytes())
            .await
            .context("写入 ACP stdin 失败")?;
        stdin.flush().await.ok();
        Ok(())
    }

    /// 发送请求并等待响应（无超时，适合 session/prompt 这类长任务）。
    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        if self.is_closed() {
            return Err(anyhow!("ACP 进程已退出"));
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        if let Err(e) = self
            .write(json!({"id": id, "method": method, "params": params}))
            .await
        {
            self.pending.lock().await.remove(&id);
            return Err(e);
        }
        match rx.await {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => Err(anyhow!("{method} 失败: {e}")),
            Err(_) => Err(anyhow!("{method} 响应通道关闭")),
        }
    }

    pub async fn request_timeout(
        &self,
        method: &str,
        params: Value,
        secs: u64,
    ) -> Result<Value> {
        tokio::time::timeout(std::time::Duration::from_secs(secs), self.request(method, params))
            .await
            .map_err(|_| anyhow!("{method} 超时（{secs}s）"))?
    }

    /// 回应 agent → client 的请求（如权限审批）。
    pub async fn respond(&self, id: i64, result: Value) -> Result<()> {
        self.write(json!({"id": id, "result": result})).await
    }

    pub async fn initialize(&self) -> Result<Value> {
        self.request_timeout(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {
                    "fs": { "readTextFile": true, "writeTextFile": true },
                    "terminal": true,
                    "auth": { "terminal": true }
                }
            }),
            30,
        )
        .await
    }

    pub async fn new_session(&self, cwd: &Path, mcp_servers: Vec<Value>) -> Result<String> {
        let cwd_abs = std::path::absolute(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        let res = self
            .request_timeout(
                "session/new",
                json!({ "cwd": cwd_abs.to_string_lossy(), "mcpServers": mcp_servers }),
                60,
            )
            .await?;
        res.get("sessionId")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("session/new 未返回 sessionId"))
    }

    pub async fn prompt(&self, session_id: &str, text: &str) -> Result<Value> {
        self.request(
            "session/prompt",
            json!({ "sessionId": session_id, "prompt": [{ "type": "text", "text": text }] }),
        )
        .await
    }

    pub async fn cancel(&self, session_id: &str) -> Result<()> {
        let _ = self
            .request_timeout("session/cancel", json!({ "sessionId": session_id }), 10)
            .await;
        Ok(())
    }

    pub async fn set_config_option(&self, session_id: &str, config_id: &str, value: &str) {
        let _ = self
            .request_timeout(
                "session/set_config_option",
                json!({ "sessionId": session_id, "configId": config_id, "value": value }),
                15,
            )
            .await;
    }

    pub async fn kill(&self) {
        let mut child = self.child.lock().await;
        let _ = child.kill().await;
    }
}

impl Drop for AcpClient {
    fn drop(&mut self) {
        self.reader_handle.abort();
    }
}