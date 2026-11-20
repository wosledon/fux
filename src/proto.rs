//! 节点通道协议（runtime ↔ 控制面，出站 WebSocket）。
use crate::runner::RunSpec;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Hello {
        node_id: String,
        approved: bool,
    },
    Approval {
        approved: bool,
    },
    Dispatch {
        spec: RunSpec,
    },
    Cancel {
        run_id: String,
    },
    Permission {
        run_id: String,
        request_id: String,
        decision: String,
    },
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeMsg {
    Heartbeat {
        load: u32,
    },
    Event {
        run_id: String,
        kind: String,
        payload: Value,
    },
    RunState {
        run_id: String,
        status: String,
        stop_reason: String,
        turn_count: u32,
        session_id: String,
    },
    PermissionRequest {
        run_id: String,
        request_id: String,
        request: Value,
    },
    Finish {
        run_id: String,
        status: String,
        stop_reason: String,
        turn_count: u32,
        session_id: String,
        pushed: bool,
        head: Option<String>,
        error: Option<String>,
    },
    Pong,
}

/// 节点能力上报。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub os: String,
    pub clis: Vec<String>,
    pub project_paths: Vec<String>,
    pub load: u32,
}

/// 探测本机已安装的 ACP CLI。
pub fn probe_clis() -> Vec<String> {
    let mut found = vec![];
    for name in [
        "codex-acp",
        "opencode",
        "pi-acp",
        "copilot",
        "fux-mock-agent",
    ] {
        if which::which(name).is_ok() {
            found.push(name.to_string());
        }
    }
    found
}
