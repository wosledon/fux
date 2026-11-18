//! fux-mock-agent — 演示/测试用 ACP agent（最小协议实现）。
//! 提示词关键字驱动行为：LOOP=循环抽搐 FAIL=突然中断 SLEEP=停滞 ASK=权限请求。
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

fn send(v: Value) {
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    let _ = writeln!(lock, "{}", serde_json::to_string(&v).unwrap());
    let _ = lock.flush();
}

fn notify(session: &str, update: Value) {
    send(json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": session, "update": update } }));
}

fn chunk(session: &str, text: &str) {
    notify(session, json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": text } }));
}

fn main() {
    let stdin = io::stdin();
    let mut session = String::from("mock-session");
    let mut cwd = String::from(".");
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
                        "protocolVersion": 1,
                        "agentCapabilities": { "loadSession": false, "promptCapabilities": { "image": false } }
                    }}));
                }
            }
            "session/new" => {
                cwd = params.get("cwd").and_then(|c| c.as_str()).unwrap_or(".").to_string();
                session = cwd.clone();
                if let Some(id) = id {
                    send(json!({ "jsonrpc": "2.0", "id": id, "result": { "sessionId": session } }));
                }
            }
            "session/set_config_option" | "session/cancel" => {
                if let Some(id) = id { send(json!({ "jsonrpc": "2.0", "id": id, "result": {} })); }
            }
            "session/prompt" => {
                let text = params.get("prompt").and_then(|p| p.as_array())
                    .map(|arr| arr.iter().filter_map(|b| b.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("\n"))
                    .unwrap_or_default();
                let sid = params.get("sessionId").and_then(|s| s.as_str()).unwrap_or(&session).to_string();

                if text.contains("SLEEP") {
                    chunk(&sid, "进入长时间静默（测试停滞阀）…\n");
                    std::thread::sleep(std::time::Duration::from_secs(600));
                }
                if text.contains("ASK") {
                    // 权限请求：等待 client 决策
                    send(json!({ "jsonrpc": "2.0", "id": 9001, "method": "session/request_permission", "params": {
                        "sessionId": sid,
                        "toolCall": { "toolCallId": "call-ask", "title": "写入文件 demo.txt", "kind": "edit" },
                        "options": [
                            { "optionId": "once", "kind": "allow_once", "name": "允许一次" },
                            { "optionId": "always", "kind": "allow_always", "name": "始终允许" },
                            { "optionId": "reject", "kind": "reject_once", "name": "拒绝" }
                        ]
                    }}));
                    let mut decided = false;
                    for resp in io::stdin().lock().lines() {
                        let resp = match resp { Ok(r) => r, Err(_) => break };
                        if let Ok(v) = serde_json::from_str::<Value>(&resp) {
                            if v.get("id").and_then(|i| i.as_i64()) == Some(9001) { decided = true; break; }
                        }
                    }
                    chunk(&sid, if decided { "权限已处理。\n" } else { "权限未决。\n" });
                }
                if text.contains("LOOP") {
                    // 模型抽搐：连续相同工具调用
                    for i in 0..6 {
                        notify(&sid, json!({ "sessionUpdate": "tool_call", "toolCallId": format!("loop-{i}"), "title": "读取同一文件", "kind": "read", "rawInput": { "path": "same.txt" } }));
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                } else {
                    notify(&sid, json!({ "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": "分析任务…" } }));
                    chunk(&sid, "收到任务，开始工作。\n");
                    notify(&sid, json!({ "sessionUpdate": "plan", "entries": [ { "content": "实现变更", "status": "in_progress" } ] }));
                    notify(&sid, json!({ "sessionUpdate": "tool_call", "toolCallId": "call-1", "title": "编辑 demo.txt", "kind": "edit", "rawInput": { "path": "demo.txt" } }));
                    // 真实产生文件变更，便于 git 交付
                    let _ = std::fs::write(std::path::Path::new(&cwd).join("demo.txt"), format!("fux mock output\n{text}\n"));
                    notify(&sid, json!({ "sessionUpdate": "tool_call_update", "toolCallId": "call-1", "status": "completed" }));
                    chunk(&sid, "工作完成，变更已写入 demo.txt。\n");
                }
                if text.contains("FAIL") {
                    chunk(&sid, "模拟突然中断…\n");
                    std::process::exit(1);
                }
                if let Some(id) = id {
                    send(json!({ "jsonrpc": "2.0", "id": id, "result": { "stopReason": "end_turn" } }));
                }
            }
            _ => {
                if let Some(id) = id { send(json!({ "jsonrpc": "2.0", "id": id, "result": {} })); }
            }
        }
    }
}