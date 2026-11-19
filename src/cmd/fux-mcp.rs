//! fux-mcp — Fux 内置 workspace MCP Server（stdio）。
//! 供 AI 助理/智能体管理工作台：任务、项目、工作流、SKILL、智能体、MCP/CLI、成员等。
//! 环境变量：FUX_API（控制面地址）、FUX_TOKEN（JWT）、FUX_WORKSPACE（工作区 id）。
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};

mod fux_mcp_tools;

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
        "PATCH" => client.patch(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        _ => client.get(&url),
    };
    req = req.bearer_auth(env("FUX_TOKEN"));
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req.send().await?;
    let status = resp.status();
    let raw = resp.text().await.unwrap_or_default();
    let v: Value = serde_json::from_str(&raw).unwrap_or_else(|_| json!({ "raw": raw }));
    if !status.is_success() {
        let msg = v
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("请求失败")
            .to_string();
        anyhow::bail!(
            "Fux API {} {} -> HTTP {}: {msg}",
            method,
            path,
            status.as_u16()
        );
    }
    Ok(v)
}

async fn board_id() -> anyhow::Result<String> {
    let v = api(
        "GET",
        &format!("/api/v1/workspaces/{}/board", env("FUX_WORKSPACE")),
        None,
    )
    .await?;
    Ok(v["board"]["id"].as_str().unwrap_or_default().to_string())
}

async fn call_tool(name: &str, args: Value) -> anyhow::Result<Value> {
    fux_mcp_tools::call_tool(name, args).await
}

fn tools() -> Value {
    fux_mcp_tools::tools()
}

#[tokio::main]
async fn main() {
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
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
            "ping" => {
                if let Some(id) = id {
                    send(json!({ "jsonrpc": "2.0", "id": id, "result": {} }));
                }
            }
            "tools/list" => {
                if let Some(id) = id {
                    send(json!({ "jsonrpc": "2.0", "id": id, "result": { "tools": tools() } }));
                }
            }
            "tools/call" => {
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let result = match call_tool(name, args).await {
                    Ok(v) => {
                        json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }] })
                    }
                    Err(e) => {
                        json!({ "content": [{ "type": "text", "text": format!("错误: {e:#}") }], "isError": true })
                    }
                };
                if let Some(id) = id {
                    send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
                }
            }
            _ => {
                if let Some(id) = id {
                    send(
                        json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "method not found" } }),
                    );
                }
            }
        }
    }
}
