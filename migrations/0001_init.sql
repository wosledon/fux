-- Fux initial schema (SQLite; keep PG-compatible where practical)

CREATE TABLE users (
  id TEXT PRIMARY KEY,
  username TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,
  display_name TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE workspaces (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  settings TEXT NOT NULL DEFAULT '{}',
  created_by TEXT NOT NULL REFERENCES users(id),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE workspace_members (
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  role TEXT NOT NULL CHECK (role IN ('owner','admin','member')),
  joined_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (workspace_id, user_id)
);

CREATE TABLE invitations (
  code TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  role TEXT NOT NULL DEFAULT 'member',
  expires_at TEXT,
  one_time INTEGER NOT NULL DEFAULT 1,
  created_by TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE projects (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('git','local')),
  remote_url TEXT,
  local_path TEXT,
  default_branch TEXT NOT NULL DEFAULT 'main',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE boards (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL UNIQUE REFERENCES workspaces(id) ON DELETE CASCADE,
  config TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE lanes (
  id TEXT PRIMARY KEY,
  board_id TEXT NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('planning','ready','work','review','blocked','done','archived')),
  name TEXT NOT NULL,
  position INTEGER NOT NULL,
  wip_limit INTEGER,
  stage_role TEXT
);

CREATE TABLE task_types (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  icon TEXT NOT NULL DEFAULT '📋',
  title_template TEXT NOT NULL DEFAULT '{{title}}',
  content_template TEXT NOT NULL DEFAULT '',
  variables_schema TEXT NOT NULL DEFAULT '[]',
  defaults TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE tasks (
  id TEXT PRIMARY KEY,
  board_id TEXT NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
  workspace_id TEXT NOT NULL,
  type_id TEXT REFERENCES task_types(id) ON DELETE SET NULL,
  project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
  title TEXT NOT NULL,
  content TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'planning' CHECK (status IN ('planning','ready','in_progress','review','blocked','done','archived','cancelled')),
  blocked_from TEXT,
  block_reason TEXT,
  lane_id TEXT REFERENCES lanes(id) ON DELETE SET NULL,
  priority INTEGER NOT NULL DEFAULT 0,
  tags TEXT NOT NULL DEFAULT '[]',
  assignee_agent_id TEXT,
  assignee_team_id TEXT,
  branch TEXT,
  pr_url TEXT,
  pr_state TEXT,
  merge_commit TEXT,
  retry_count INTEGER NOT NULL DEFAULT 0,
  lease_run_id TEXT,
  lease_expires_at TEXT,
  version INTEGER NOT NULL DEFAULT 0,
  created_by TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_tasks_board_status ON tasks(board_id, status);
CREATE INDEX idx_tasks_status ON tasks(status);

CREATE TABLE task_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  kind TEXT NOT NULL,
  actor TEXT NOT NULL,
  run_id TEXT,
  payload TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  UNIQUE (task_id, seq)
);

CREATE TABLE cli_entries (
  id TEXT PRIMARY KEY,
  workspace_id TEXT,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  command TEXT NOT NULL,
  args TEXT NOT NULL DEFAULT '[]',
  env_template TEXT NOT NULL DEFAULT '{}',
  is_default INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE agents (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  cli_entry_id TEXT NOT NULL REFERENCES cli_entries(id),
  model TEXT NOT NULL DEFAULT '',
  reasoning_effort TEXT NOT NULL DEFAULT 'medium',
  system_prompt TEXT NOT NULL DEFAULT '',
  policy TEXT NOT NULL DEFAULT 'write_ask',
  gates TEXT NOT NULL DEFAULT '{}',
  max_concurrent INTEGER NOT NULL DEFAULT 1,
  paused INTEGER NOT NULL DEFAULT 0,
  match_tags TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE agent_teams (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  mode TEXT NOT NULL DEFAULT 'pipeline' CHECK (mode IN ('pipeline','supervisor'))
);

CREATE TABLE team_members (
  team_id TEXT NOT NULL REFERENCES agent_teams(id) ON DELETE CASCADE,
  agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  role TEXT NOT NULL DEFAULT 'worker',
  position INTEGER NOT NULL,
  PRIMARY KEY (team_id, agent_id)
);

CREATE TABLE skills (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  source TEXT NOT NULL DEFAULT '',
  version TEXT NOT NULL DEFAULT '1.0.0',
  content TEXT NOT NULL DEFAULT ''
);

CREATE TABLE agent_skills (
  agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  skill_id TEXT NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
  PRIMARY KEY (agent_id, skill_id)
);

CREATE TABLE mcp_servers (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  transport TEXT NOT NULL CHECK (transport IN ('stdio','sse','http')),
  command TEXT,
  args TEXT NOT NULL DEFAULT '[]',
  url TEXT,
  env_json TEXT NOT NULL DEFAULT '{}',
  status TEXT NOT NULL DEFAULT 'unknown'
);

CREATE TABLE agent_mcp_servers (
  agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  mcp_server_id TEXT NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
  tool_whitelist TEXT NOT NULL DEFAULT '[]',
  PRIMARY KEY (agent_id, mcp_server_id)
);

CREATE TABLE runs (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  agent_id TEXT NOT NULL,
  team_id TEXT,
  node_id TEXT NOT NULL,
  stage INTEGER NOT NULL DEFAULT 0,
  retry_count INTEGER NOT NULL DEFAULT 0,
  retry_of TEXT,
  acp_session_id TEXT,
  status TEXT NOT NULL DEFAULT 'running',
  stop_reason TEXT,
  turn_count INTEGER NOT NULL DEFAULT 0,
  last_event_at TEXT,
  usage_tokens INTEGER NOT NULL DEFAULT 0,
  started_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  ended_at TEXT
);
CREATE INDEX idx_runs_task ON runs(task_id);
CREATE INDEX idx_runs_status ON runs(status);

CREATE TABLE nodes (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  mode TEXT NOT NULL DEFAULT 'runtime',
  token TEXT NOT NULL,
  approved INTEGER NOT NULL DEFAULT 0,
  conn_status TEXT NOT NULL DEFAULT 'offline',
  capabilities TEXT NOT NULL DEFAULT '{}',
  last_heartbeat TEXT,
  approved_by TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE pairing_tokens (
  code TEXT PRIMARY KEY,
  expires_at TEXT,
  one_time INTEGER NOT NULL DEFAULT 1,
  created_by TEXT NOT NULL,
  used_by_node TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE secrets (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  key TEXT NOT NULL,
  encrypted_value TEXT NOT NULL
);

CREATE TABLE approvals (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  request TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',
  decided_by TEXT,
  decided_at TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE notifications (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  task_id TEXT,
  message TEXT NOT NULL,
  read_by TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE assistant_messages (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  workspace_id TEXT NOT NULL,
  role TEXT NOT NULL,
  content TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);