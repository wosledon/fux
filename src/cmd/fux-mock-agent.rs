//! fux-mock-agent — 演示/测试用 ACP agent（最小协议实现）。
//! 提示词关键字驱动行为：LOOP=循环抽搐 FAIL=突然中断 SLEEP=停滞 ASK=权限请求。
//! 权限请求是「挂起当前 prompt」的：主循环发出请求后记录待续跑的 prompt，
//! 收到 client 的决策响应后再继续执行（不在 prompt 处理里嵌套读 stdin）。
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};

/// 权限请求的固定 id（agent → client 请求）。
const PERM_REQUEST_ID: i64 = 9001;

fn send(v: Value) {
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    let _ = writeln!(lock, "{}", serde_json::to_string(&v).unwrap());
    let _ = lock.flush();
}

fn notify(session: &str, update: Value) {
    send(
        json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": session, "update": update } }),
    );
}

fn chunk(session: &str, text: &str) {
    notify(
        session,
        json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": text } }),
    );
}

/// 会话响应里的配置项（模型 / 推理强度），session/new 与 session/load 共用。
fn session_result(session: &str) -> Value {
    json!({
        "sessionId": session,
        "configOptions": [
            {
                "id": "model", "name": "模型", "category": "model", "type": "select",
                "currentValue": "mock-fast",
                "options": [
                    { "value": "mock-fast", "name": "Mock Fast", "description": "快速模型" },
                    { "value": "mock-smart", "name": "Mock Smart", "description": "高智能模型" }
                ]
            },
            {
                "id": "reasoning", "name": "推理强度", "category": "thought_level", "type": "select",
                "currentValue": "medium",
                "options": [
                    { "value": "low", "name": "Low" },
                    { "value": "medium", "name": "Medium" },
                    { "value": "high", "name": "High" }
                ]
            }
        ]
    })
}

/// 挂起中的 prompt：ASK 已发出权限请求，等待 client 决策。
struct PendingPrompt {
    /// 发起 prompt 的请求 id（决策回来后用它回 stopReason）
    prompt_id: Option<Value>,
    sid: String,
    text: String,
}

/// 执行一次 prompt（权限决策已由 `decided` 给出；None 表示本条 prompt 不涉及权限请求）。
fn run_prompt(cwd: &str, sid: &str, text: &str, prompt_id: Option<Value>, decided: Option<bool>) {
    if text.contains("SLEEP") {
        chunk(sid, "进入长时间静默（测试停滞阀）…\n");
        std::thread::sleep(std::time::Duration::from_secs(600));
    }
    if let Some(decided) = decided {
        chunk(
            sid,
            if decided {
                "权限已处理。\n"
            } else {
                "权限未决。\n"
            },
        );
    }
    if text.contains("LOOP") {
        // 模型抽搐：连续相同工具调用
        for i in 0..6 {
            notify(
                sid,
                json!({ "sessionUpdate": "tool_call", "toolCallId": format!("loop-{i}"), "title": "读取同一文件", "kind": "read", "rawInput": { "path": "same.txt" } }),
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    } else {
        notify(
            sid,
            json!({ "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": "分析任务…" } }),
        );
        chunk(sid, "收到任务，开始工作。\n");
        notify(
            sid,
            json!({ "sessionUpdate": "plan", "entries": [ { "content": "实现变更", "status": "in_progress" } ] }),
        );
        notify(
            sid,
            json!({ "sessionUpdate": "tool_call", "toolCallId": "call-1", "title": "编辑 demo.txt", "kind": "edit", "rawInput": { "path": "demo.txt" } }),
        );
        // 真实产生文件变更，便于 git 交付
        let _ = std::fs::write(
            std::path::Path::new(cwd).join("demo.txt"),
            format!("fux mock output\n{text}\n"),
        );
        notify(
            sid,
            json!({ "sessionUpdate": "tool_call_update", "toolCallId": "call-1", "status": "completed" }),
        );
        chunk(sid, "工作完成，变更已写入 demo.txt。\n");
    }
    if text.contains("FAIL") {
        chunk(sid, "模拟突然中断…\n");
        std::process::exit(1);
    }
    if let Some(id) = prompt_id {
        send(json!({ "jsonrpc": "2.0", "id": id, "result": { "stopReason": "end_turn" } }));
    }
}

fn main() {
    let stdin = io::stdin();
    let mut session = String::from("mock-session");
    let mut cwd = String::from(".");
    let mut pending: Option<PendingPrompt> = None;
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

        // 对 agent → client 请求的响应（无 method）：ASK 的权限决策在此回到主循环
        if method.is_empty() {
            let resp_id = id.as_ref().and_then(|i| i.as_i64());
            if let (Some(resp_id), Some(_)) = (resp_id, pending.as_ref()) {
                if resp_id == PERM_REQUEST_ID {
                    let p = pending.take().expect("pending 已取出");
                    run_prompt(&cwd, &p.sid, &p.text, p.prompt_id, Some(true));
                }
            }
            continue;
        }

        match method {
            "initialize" => {
                if let Some(id) = id {
                    send(json!({ "jsonrpc": "2.0", "id": id, "result": {
                        "protocolVersion": 1,
                        "agentCapabilities": { "loadSession": true, "promptCapabilities": { "image": false } }
                    }}));
                }
            }
            "session/new" => {
                cwd = params
                    .get("cwd")
                    .and_then(|c| c.as_str())
                    .unwrap_or(".")
                    .to_string();
                session = cwd.clone();
                if let Some(id) = id {
                    send(json!({ "jsonrpc": "2.0", "id": id, "result": session_result(&session) }));
                }
            }
            "session/load" => {
                // 恢复会话：回显请求的 sessionId（真实 CLI 会在此还原历史上下文）
                cwd = params
                    .get("cwd")
                    .and_then(|c| c.as_str())
                    .unwrap_or(".")
                    .to_string();
                session = params
                    .get("sessionId")
                    .and_then(|s| s.as_str())
                    .unwrap_or("mock-session")
                    .to_string();
                if let Some(id) = id {
                    send(json!({ "jsonrpc": "2.0", "id": id, "result": session_result(&session) }));
                }
            }
            "session/set_config_option" | "session/cancel" => {
                if let Some(id) = id {
                    send(json!({ "jsonrpc": "2.0", "id": id, "result": {} }));
                }
            }
            "session/prompt" => {
                let text = params
                    .get("prompt")
                    .and_then(|p| p.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                let sid = params
                    .get("sessionId")
                    .and_then(|s| s.as_str())
                    .unwrap_or(&session)
                    .to_string();

                if text.contains("ASK") {
                    // 权限请求：等待 client 决策（决策回来后由主循环续跑本条 prompt）
                    send(
                        json!({ "jsonrpc": "2.0", "id": PERM_REQUEST_ID, "method": "session/request_permission", "params": {
                            "sessionId": sid,
                            "toolCall": { "toolCallId": "call-ask", "title": "写入文件 demo.txt", "kind": "edit" },
                            "options": [
                                { "optionId": "once", "kind": "allow_once", "name": "允许一次" },
                                { "optionId": "always", "kind": "allow_always", "name": "始终允许" },
                                { "optionId": "reject", "kind": "reject_once", "name": "拒绝" }
                            ]
                        }}),
                    );
                    pending = Some(PendingPrompt {
                        prompt_id: id,
                        sid,
                        text,
                    });
                } else {
                    run_prompt(&cwd, &sid, &text, id, None);
                }
            }
            _ => {
                if let Some(id) = id {
                    send(json!({ "jsonrpc": "2.0", "id": id, "result": {} }));
                }
            }
        }
    }
}
