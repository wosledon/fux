-- AI 助理多会话：会话表 + 消息挂载会话。
CREATE TABLE assistant_sessions (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  title TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_assistant_sessions_ws ON assistant_sessions(workspace_id, updated_at);

ALTER TABLE assistant_messages ADD COLUMN session_id TEXT;
CREATE INDEX idx_assistant_messages_session ON assistant_messages(session_id);