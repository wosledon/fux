-- 工作流发起变量与节点级重试策略。
-- 变量定义仍随工作流模板 JSON 持久化；任务保存本次发起实际填写的值。
ALTER TABLE workflows ADD COLUMN variables TEXT NOT NULL DEFAULT '[]';
ALTER TABLE tasks ADD COLUMN workflow_inputs TEXT NOT NULL DEFAULT '{}';
