//! fux-mcp — Fux 内置 workspace MCP Server（stdio）。
//! 供 AI 助理/智能体操作工作台：创建任务、移动、评论、查询。
//! 环境变量：FUX_API（控制面地址）、FUX_TOKEN（JWT）、FUX_WORKSPACE（工作区 id）。
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

fn send(v: Value) {
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    let _ = writeln!(lock, "{}", serde_json::to_string(&v).unwrap());
    let _ = lock.flush();
}

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_default()
}

async fn api(method: &str, path: &str, body: Option<Value>) -> anyhow::Result<Value> {
    let client = reqwest::Client::new();
    let url = format!("{}{}", env("FUX_API").trim_end_matches('/'), path);
    let mut req = match method {
        "POST" => client.post(&url),
        _ => client.get(&url),
    };
    req = req.bearer_auth(env("FUX_TOKEN"));
    if let Some(b) = body { req = req.json(&b); }
    let resp = req.send().await?;
    let v: Value = resp.json().await?;
    Ok(v)
}

async fn board_id() -> anyhow::Result<String> {
    let v = api("GET", &format!("/api/v1/workspaces/{}/board", env("FUX_WORKSPACE")), None).await?;
    Ok(v["board"]["id"].as_str().unwrap_or_default().to_string())
}

async fn call_tool(name: &str, args: Value) -> anyhow::Result<Value> {
    match name {
        "create_task" => {
            let board = board_id().await?;
            let mut body = json!({
                "boardId": board,
                "title": args.get("title").and_then(|t| t.as_str()).unwrap_or("未命名任务"),
                "content": args.get("content").and_then(|t| t.as_str()).unwrap_or(""),
            });
            if let Some(p) = args.get("projectId") { body["projectId"] = p.clone(); }
            if let Some(t) = args.get("typeId") { body["typeId"] = t.clone(); }
            api("POST", "/api/v1/tasks", Some(body)).await
        }
        "list_tasks" => {
            let board = board_id().await?;
            let mut path = format!("/api/v1/tasks?boardId={board}");
            if let Some(s) = args.get("status").and_then(|s| s.as_str()) {
                path.push_str(&format!("&status={s}"));
            }
            api("GET", &path, None).await
        }
        "move_task" => {
            let task_id = args.get("taskId").and_then(|t| t.as_str()).unwrap_or_default();
            let body = json!({
                "to": args.get("to").and_then(|t| t.as_str()).unwrap_or("ready"),
                "reason": args.get("reason").and_then(|t| t.as_str()).unwrap_or("助理操作"),
            });
            api("POST", &format!("/api/v1/tasks/{task_id}/transition"), Some(body)).await
        }
        "add_comment" => {
            let task_id = args.get("taskId").and_then(|t| t.as_str()).unwrap_or_default();
            let body = json!({ "text": args.get("text").and_then(|t| t.as_str()).unwrap_or("") });
            api("POST", &format!("/api/v1/tasks/{task_id}/comments"), Some(body)).await
        }
        _ => anyhow::bail!("未知工具: {name}"),
    }
}

fn tools() -> Value {
    json!([
        { "name": "create_task", "description": "在工作区看板创建任务（进入待规划区）",
          "inputSchema": { "type": "object", "properties": {
              "title": { "type": "string" }, "content": { "type": "string" },
              "projectId": { "type": "string" }, "typeId": { "type": "string" } },
              "required": ["title"] } },
        { "name": "list_tasks", "description": "查询看板任务，可按状态过滤（planning/ready/in_progress/review/blocked/done/archived）",
          "inputSchema": { "type": "object", "properties": { "status": { "type": "string" } } } },
        { "name": "move_task", "description": "移动任务状态（如 planning→ready）",
          "inputSchema": { "type": "object", "properties": {
              "taskId": { "type": "string" }, "to": { "type": "string" }, "reason": { "type": "string" } },
              "required": ["taskId", "to"] } },
        { "name": "add_comment", "description": "给任务评论（进行中任务会引导智能体工作）",
          "inputSchema": { "type": "object", "properties": {
              "taskId": { "type": "string" }, "text": { "type": "string" } },
              "required": ["taskId", "text"] } }
    ])
}

#[tokio::main]
async fn main() {
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line { Ok(l) => l, Err(_) => break };
        if line.trim().is_empty() { continue; }
        let msg: Value = match serde_json::from_str(&line) { Ok(v) => v, Err(_) => continue };
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        match method {
            "initialize" => {
                if let Some(id) = id {
                    send(json!({ "jsonrpc": "2.0", "id": id, "result": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "fux-workspace", "version": env!("CARGO_PKG_VERSION") }
                    }}));
                }
            }
            "notifications/initialized" | "notifications/cancelled" => {}
            "ping" => { if let Some(id) = id { send(json!({ "jsonrpc": "2.0", "id": id, "result": {} })); } }
            "tools/list" => {
                if let Some(id) = id { send(json!({ "jsonrpc": "2.0", "id": id, "result": { "tools": tools() } })); }
            }
            "tools/call" => {
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let result = match call_tool(name, args).await {
                    Ok(v) => json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }] }),
                    Err(e) => json!({ "content": [{ "type": "text", "text": format!("错误: {e:#}") }], "isError": true }),
                };
                if let Some(id) = id { send(json!({ "jsonrpc": "2.0", "id": id, "result": result })); }
            }
            _ => {
                if let Some(id) = id {
                    send(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "method not found" } }));
                }
            }
        }
    }
}