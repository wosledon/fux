//! ACP (Agent Client Protocol) client：JSON-RPC 2.0 over stdio，NDJSON  framing。
//! Fux 作为 client，拉起 CLI agent 子进程（Codex/OpenCode/Pi/Copilot 等）。
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, mpsc, oneshot};

/// agent → client 的请求（如 session/request_permission）
pub struct IncomingRequest {
    pub id: i64,
    pub method: String,
    pub params: Value,
}

/// 按 CLI 类型补充运行环境变量。
/// OpenCode 启动时会强制拉取 models.dev（不可达时等 10s 超时），
/// 统一注入跳过开关；用户在 CLI env 里显式配置时以用户为准。
pub fn tweak_env(cli_kind: &str, env: &mut Vec<(String, String)>) {
    const KEY: &str = "OPENCODE_DISABLE_MODELS_FETCH";
    if cli_kind == "opencode" && !env.iter().any(|(k, _)| k == KEY) {
        env.push((KEY.to_string(), "1".to_string()));
    }
}

/// 构建 ACP 子进程命令。
/// Windows 上 npm 全局安装的 CLI 是 .cmd/.bat/.ps1 垫片，CreateProcess 不能直接执行，
/// 需要经 cmd.exe / powershell 启动；其余情况优先使用 which 解析到的完整路径。
fn build_command(command: &str, args: &[String]) -> Command {
    #[cfg(windows)]
    {
        if let Ok(resolved) = which::which(command) {
            let ext = resolved
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if ext == "cmd" || ext == "bat" {
                let mut c = Command::new("cmd.exe");
                c.arg("/C").arg(resolved).args(args);
                return c;
            }
            if ext == "ps1" {
                let mut c = Command::new("powershell.exe");
                c.arg("-NoProfile")
                    .arg("-ExecutionPolicy")
                    .arg("Bypass")
                    .arg("-File")
                    .arg(resolved)
                    .args(args);
                return c;
            }
            let mut c = Command::new(resolved);
            c.args(args);
            return c;
        }
    }
    let mut c = Command::new(command);
    c.args(args);
    c
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
        let mut cmd = build_command(command, args);
        cmd.current_dir(cwd)
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
                                        let _ = tx.send(Ok(msg
                                            .get("result")
                                            .cloned()
                                            .unwrap_or(json!({}))));
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

    pub async fn request_timeout(&self, method: &str, params: Value, secs: u64) -> Result<Value> {
        tokio::time::timeout(
            std::time::Duration::from_secs(secs),
            self.request(method, params),
        )
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
        let res = self.new_session_full(cwd, mcp_servers).await?;
        res.get("sessionId")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("session/new 未返回 sessionId"))
    }

    /// 创建会话并返回完整响应（含 configOptions / models，用于模型与推理强度发现）。
    pub async fn new_session_full(&self, cwd: &Path, mcp_servers: Vec<Value>) -> Result<Value> {
        let cwd_abs = std::path::absolute(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        self.request_timeout(
            "session/new",
            json!({ "cwd": cwd_abs.to_string_lossy(), "mcpServers": mcp_servers }),
            60,
        )
        .await
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

/// 取会话响应（或 config_option_update 事件）里的配置项数组。
fn config_options(v: &Value) -> Option<&Vec<Value>> {
    v.get("configOptions")
        .and_then(|x| x.as_array())
        .or_else(|| {
            v.pointer("/update/configOptions")
                .and_then(|x| x.as_array())
        })
}

fn option_matches(opt: &Value, category: &str, ids: &[&str]) -> bool {
    let cat = opt.get("category").and_then(|v| v.as_str()).unwrap_or("");
    let id = opt.get("id").and_then(|v| v.as_str()).unwrap_or("");
    cat == category || ids.iter().any(|x| x.eq_ignore_ascii_case(id))
}

/// 从 session/new 响应中找出模型 / 推理强度的配置项 id。
pub fn session_option_ids(session: &Value) -> (Option<String>, Option<String>) {
    let mut model = None;
    let mut effort = None;
    if let Some(opts) = config_options(session) {
        for o in opts {
            let id = o.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
            if model.is_none() && option_matches(o, "model", &["model"]) {
                model = id.clone();
            }
            if effort.is_none()
                && option_matches(
                    o,
                    "thought_level",
                    &["effort", "reasoning_effort", "reasoningEffort"],
                )
            {
                effort = id;
            }
        }
    }
    (model, effort)
}

fn push_choice(out: &mut Vec<Value>, item: &Value, id_key: &str) {
    let id = item.get(id_key).and_then(|v| v.as_str()).unwrap_or("");
    if id.is_empty() {
        return;
    }
    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or(id);
    out.push(json!({
        "id": id,
        "name": name,
        "description": item.get("description").cloned().unwrap_or(Value::Null),
    }));
}

fn collect_select_values(out: &mut Vec<Value>, opt: &Value) {
    let Some(items) = opt.get("options").and_then(|v| v.as_array()) else {
        return;
    };
    for item in items {
        // options 既可能是扁平的选项数组，也可能是 {group,name,options} 分组
        if let Some(group) = item.get("options").and_then(|v| v.as_array()) {
            for sub in group {
                push_choice(out, sub, "value");
            }
        } else {
            push_choice(out, item, "value");
        }
    }
}

/// 提取 ACP 会话声明的模型与推理强度；兼容新版 configOptions 与旧版 models 字段。
pub fn session_catalog(session: &Value) -> Value {
    let mut models = Vec::new();
    let mut efforts = Vec::new();
    let mut current_model = session
        .pointer("/models/currentModelId")
        .cloned()
        .unwrap_or(Value::Null);
    let mut current_effort = Value::Null;

    if let Some(arr) = session
        .pointer("/models/availableModels")
        .and_then(|v| v.as_array())
    {
        for it in arr {
            push_choice(&mut models, it, "modelId");
        }
    }

    if let Some(opts) = config_options(session) {
        for o in opts {
            if option_matches(o, "model", &["model"]) {
                if models.is_empty() {
                    collect_select_values(&mut models, o);
                }
                if let Some(cur) = o.get("currentValue") {
                    current_model = cur.clone();
                }
            } else if option_matches(
                o,
                "thought_level",
                &["effort", "reasoning_effort", "reasoningEffort"],
            ) {
                if efforts.is_empty() {
                    collect_select_values(&mut efforts, o);
                }
                if let Some(cur) = o.get("currentValue") {
                    current_effort = cur.clone();
                }
            }
        }
    }

    json!({
        "models": models,
        "currentModelId": current_model,
        "efforts": efforts,
        "currentEffortId": current_effort,
    })
}

/// 把 config_option_update 通知里的完整配置项集合合并回会话响应。
pub fn merge_config_options(session: &mut Value, update: &Value) {
    if let Some(opts) = update
        .pointer("/update/configOptions")
        .and_then(|v| v.as_array())
    {
        session["configOptions"] = Value::Array(opts.clone());
    }
}

/// 按会话声明的配置项设置模型 / 推理强度；不声明时回退到 OpenCode 的固定 id。
pub async fn apply_model_effort(
    client: &AcpClient,
    session: &Value,
    session_id: &str,
    model: &str,
    effort: &str,
    cli_kind: &str,
) {
    // Copilot 的推理强度由启动参数控制、模型经 /model prompt 设置
    if cli_kind == "copilot" {
        return;
    }
    let (model_opt, effort_opt) = session_option_ids(session);
    if !model.is_empty() {
        if let Some(id) = model_opt {
            client.set_config_option(session_id, &id, model).await;
        } else if cli_kind == "opencode" {
            client.set_config_option(session_id, "model", model).await;
        }
    }
    let mapped = crate::runner::map_effort(effort);
    if let Some(id) = effort_opt {
        client.set_config_option(session_id, &id, &mapped).await;
    } else if cli_kind == "opencode" {
        client
            .set_config_option(session_id, "effort", &mapped)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_supports_config_options_and_legacy_models() {
        let session = json!({
            "sessionId": "s1",
            "configOptions": [
                {
                    "id": "model", "name": "Model", "category": "model", "type": "select",
                    "currentValue": "a",
                    "options": [
                        { "value": "a", "name": "Model A" },
                        { "value": "b", "name": "Model B" }
                    ]
                },
                {
                    "id": "reasoning", "name": "Reasoning", "category": "thought_level", "type": "select",
                    "currentValue": "high",
                    "options": [
                        { "group": "g1", "name": "G1", "options": [
                            { "value": "low", "name": "Low" },
                            { "value": "high", "name": "High" }
                        ]}
                    ]
                }
            ]
        });
        let cat = session_catalog(&session);
        assert_eq!(cat["models"].as_array().unwrap().len(), 2);
        assert_eq!(cat["currentModelId"], "a");
        assert_eq!(cat["efforts"].as_array().unwrap().len(), 2);
        assert_eq!(
            session_option_ids(&session),
            (Some("model".to_string()), Some("reasoning".to_string()))
        );

        let legacy = json!({
            "models": { "availableModels": [{ "modelId": "x", "name": "X" }], "currentModelId": "x" }
        });
        let cat = session_catalog(&legacy);
        assert_eq!(cat["models"][0]["id"], "x");
        assert_eq!(cat["currentModelId"], "x");
        assert_eq!(session_option_ids(&legacy), (None, None));
    }
}
