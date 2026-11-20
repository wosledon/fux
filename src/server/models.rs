//! 数据行模型（sqlx FromRow + camelCase JSON）。
use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub username: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub display_name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub settings: String,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberView {
    pub user_id: String,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub joined_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Invitation {
    pub code: String,
    pub workspace_id: String,
    pub role: String,
    pub expires_at: Option<String>,
    pub one_time: i64,
    pub created_by: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub kind: String,
    pub remote_url: Option<String>,
    pub local_path: Option<String>,
    pub default_branch: String,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    pub id: String,
    pub workspace_id: String,
    pub config: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lane {
    pub id: String,
    pub board_id: String,
    pub kind: String,
    pub name: String,
    pub position: i64,
    pub wip_limit: Option<i64>,
    pub stage_role: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskType {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub icon: String,
    pub title_template: String,
    pub content_template: String,
    pub variables_schema: String,
    pub defaults: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub board_id: String,
    pub workspace_id: String,
    pub type_id: Option<String>,
    pub project_id: Option<String>,
    pub title: String,
    pub content: String,
    pub status: String,
    pub blocked_from: Option<String>,
    pub block_reason: Option<String>,
    pub lane_id: Option<String>,
    pub priority: i64,
    pub tags: String,
    pub assignee_agent_id: Option<String>,
    pub workflow_id: Option<String>,
    pub workflow_inputs: String,
    pub branch: Option<String>,
    pub pr_url: Option<String>,
    pub pr_state: Option<String>,
    pub merge_commit: Option<String>,
    pub retry_count: i64,
    pub lease_run_id: Option<String>,
    pub lease_expires_at: Option<String>,
    pub version: i64,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// 派生字段（仅任务列表/详情填充，不落库）
    #[sqlx(default)]
    pub workflow_name: Option<String>,
    #[sqlx(default)]
    pub stage_total: Option<i64>,
    #[sqlx(default)]
    pub stage_done: Option<i64>,
    #[sqlx(default)]
    pub comment_count: Option<i64>,
    #[sqlx(default)]
    pub run_count: Option<i64>,
    #[sqlx(default)]
    pub last_run_status: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvent {
    pub id: i64,
    pub task_id: String,
    pub seq: i64,
    pub kind: String,
    pub actor: String,
    pub run_id: Option<String>,
    pub payload: String,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliEntry {
    pub id: String,
    pub workspace_id: Option<String>,
    pub name: String,
    pub kind: String,
    pub command: String,
    pub args: String,
    pub env_template: String,
    pub is_default: i64,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub cli_entry_id: String,
    pub model: String,
    pub reasoning_effort: String,
    pub system_prompt: String,
    pub policy: String,
    pub gates: String,
    pub max_concurrent: i64,
    pub paused: i64,
    pub match_tags: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workflow {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub description: String,
    pub variables: String,
    pub stages: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Delivery {
    pub id: String,
    pub task_id: String,
    pub project_id: String,
    pub node_key: String,
    pub branch: Option<String>,
    pub pr_url: Option<String>,
    pub pr_state: Option<String>,
    pub merge_commit: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub description: String,
    pub source: String,
    pub version: String,
    pub content: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    pub args: String,
    pub url: Option<String>,
    pub env_json: String,
    pub status: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub task_id: String,
    pub agent_id: String,
    pub node_id: String,
    pub stage: i64,
    pub stage_key: Option<String>,
    pub project_id: Option<String>,
    pub branch: Option<String>,
    pub head_sha: Option<String>,
    pub lease_expires_at: Option<String>,
    pub retry_count: i64,
    pub retry_of: Option<String>,
    pub acp_session_id: Option<String>,
    pub status: String,
    pub stop_reason: Option<String>,
    pub turn_count: i64,
    pub last_event_at: Option<String>,
    pub usage_tokens: i64,
    pub started_at: String,
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    pub name: String,
    pub mode: String,
    #[serde(skip_serializing)]
    pub token: String,
    pub approved: i64,
    pub conn_status: String,
    pub capabilities: String,
    pub last_heartbeat: Option<String>,
    pub approved_by: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub id: String,
    pub run_id: String,
    pub task_id: String,
    pub request: String,
    pub status: String,
    pub decided_by: Option<String>,
    pub decided_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: String,
    pub workspace_id: String,
    pub kind: String,
    pub task_id: Option<String>,
    pub message: String,
    pub read_by: String,
    pub created_at: String,
}
