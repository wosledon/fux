-- 工作流（DAG 多节点编排）+ 多项目交付。
CREATE TABLE workflows (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  max_parallel INTEGER NOT NULL DEFAULT 2,
  stages TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_workflows_ws ON workflows(workspace_id);

ALTER TABLE tasks ADD COLUMN workflow_id TEXT;

CREATE TABLE deliveries (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  project_id TEXT NOT NULL,
  node_key TEXT NOT NULL,
  branch TEXT,
  pr_url TEXT,
  pr_state TEXT,
  merge_commit TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  UNIQUE (task_id, project_id, node_key)
);
CREATE INDEX idx_deliveries_task ON deliveries(task_id);

ALTER TABLE runs ADD COLUMN project_id TEXT;
ALTER TABLE runs ADD COLUMN branch TEXT;
ALTER TABLE runs ADD COLUMN head_sha TEXT;
ALTER TABLE runs ADD COLUMN stage_key TEXT;
ALTER TABLE runs ADD COLUMN lease_expires_at TEXT;
CREATE INDEX idx_runs_task_stage ON runs(task_id, stage_key);