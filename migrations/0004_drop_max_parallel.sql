-- 并行度由工作流 DAG 结构决定，不再提供配置项。
ALTER TABLE workflows DROP COLUMN max_parallel;