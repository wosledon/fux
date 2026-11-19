-- 移除「智能体团队」设计（PRD v0.4）：团队已由「工作流」替代。
DROP TABLE IF EXISTS team_members;
DROP TABLE IF EXISTS agent_teams;
ALTER TABLE tasks DROP COLUMN assignee_team_id;
ALTER TABLE runs DROP COLUMN team_id;