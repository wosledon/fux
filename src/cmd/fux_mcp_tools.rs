//! fux-mcp 的工具集：以当前用户身份代理 Fux REST API。
//! 权限、状态机与审计仍由控制面负责；这里只做参数收敛和错误透传。
use super::{api, board_id, env};
use anyhow::{Result, bail};
use serde_json::{Value, json};

fn ws() -> String {
    env("FUX_WORKSPACE")
}

fn ws_path(suffix: &str) -> String {
    format!("/api/v1/workspaces/{}{suffix}", ws())
}

fn text(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn require(args: &Value, key: &str) -> Result<String> {
    let value = text(args, key);
    if value.is_empty() {
        bail!("缺少参数 {key}");
    }
    Ok(value)
}

fn copy(args: &Value, body: &mut Value, fields: &[&str]) {
    for field in fields {
        if let Some(value) = args.get(*field) {
            if !value.is_null() {
                body[*field] = value.clone();
            }
        }
    }
}

fn p_str(desc: &str) -> Value {
    json!({ "type": "string", "description": desc })
}

fn p_int(desc: &str) -> Value {
    json!({ "type": "integer", "description": desc })
}

fn p_bool(desc: &str) -> Value {
    json!({ "type": "boolean", "description": desc })
}

fn p_obj(desc: &str) -> Value {
    json!({ "type": "object", "description": desc })
}

fn p_str_arr(desc: &str) -> Value {
    json!({ "type": "array", "items": { "type": "string" }, "description": desc })
}

fn tool(name: &str, desc: &str, props: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": desc,
        "inputSchema": {
            "type": "object",
            "properties": props,
            "required": required,
        }
    })
}

pub fn tools() -> Value {
    let variables_schema = json!({
        "type": "array",
        "description": "发起变量；可在任务标题、内容、标签和节点追加提示词中用 {{key}} 引用",
        "items": {
            "type": "object",
            "properties": {
                "key": p_str("变量 key，字母/数字/下划线"),
                "label": p_str("显示名称"),
                "description": p_str("补充说明"),
                "required": json!({ "type": "boolean", "description": "是否必填" }),
                "default": json!({ "description": "默认值（字符串）" }),
                "options": p_str_arr("可选值列表"),
            },
            "required": ["key", "label"],
        }
    });
    let stages_schema = json!({
        "type": "array",
        "description": "节点数组。每个节点绑定智能体，可选执行项目、追加提示词、依赖、跳过条件与重试次数；依赖满足即并行",
        "items": {
            "type": "object",
            "properties": {
                "key": p_str("节点唯一 key，如 design"),
                "name": p_str("节点显示名"),
                "agentId": p_str("绑定智能体 ID"),
                "projectId": p_str("执行项目 ID；省略则继承任务主项目"),
                "promptAppend": p_str("追加到该节点的提示词"),
                "dependsOn": p_str_arr("依赖的上游节点 key 列表"),
                "when": p_str("跳过条件，如 tag:ui / path:docs/ / var:mode=fast"),
                "maxRetries": p_int("突然中断重试上限，0-5，缺省 2"),
                "x": json!({ "type": "number", "description": "画布 x 坐标（可选）" }),
                "y": json!({ "type": "number", "description": "画布 y 坐标（可选）" }),
            },
            "required": ["key", "name", "agentId"],
        }
    });

    json!([
        tool(
            "workspace_overview",
            "查看当前工作区全貌：工作区、看板泳道、项目、智能体、工作流、SKILL、任务类型、MCP、CLI 与成员。需要 ID 时先调用它。",
            json!({}),
            &[]
        ),
        tool(
            "create_task",
            "在看板创建任务（进入待规划）。",
            json!({
                "title": p_str("任务标题（必填）"),
                "content": p_str("任务内容/提示词"),
                "projectId": p_str("关联项目 ID"),
                "typeId": p_str("任务类型 ID"),
                "priority": p_int("优先级，0 普通，1-3 越高越优先"),
                "tags": p_str_arr("标签列表"),
                "assigneeAgentId": p_str("指派智能体 ID；留空自动匹配"),
            }),
            &["title"]
        ),
        tool(
            "list_tasks",
            "查询看板任务，可按状态或项目过滤。",
            json!({
                "status": p_str("状态：planning/ready/in_progress/review/blocked/done/archived"),
                "projectId": p_str("项目 ID"),
            }),
            &[]
        ),
        tool(
            "get_task",
            "查看任务详情，含运行记录、工作流定义与交付项。",
            json!({ "taskId": p_str("任务 ID") }),
            &["taskId"]
        ),
        tool(
            "update_task",
            "编辑任务基础字段（标题、内容、优先级、标签、项目、指派、泳道）。",
            json!({
                "taskId": p_str("任务 ID"),
                "title": p_str("新标题"),
                "content": p_str("新内容/提示词"),
                "priority": p_int("新优先级"),
                "tags": p_str_arr("新标签列表"),
                "projectId": p_str("新项目 ID"),
                "assigneeAgentId": p_str("新指派智能体 ID"),
                "laneId": p_str("目标泳道 ID（同状态内移动）"),
            }),
            &["taskId"]
        ),
        tool(
            "move_task",
            "移动任务状态（受状态机与当前用户权限约束）。",
            json!({
                "taskId": p_str("任务 ID"),
                "to": p_str("目标状态：ready/done/archived/cancelled 等"),
                "reason": p_str("变更原因"),
                "expectedVersion": p_int("期望版本（可选，用于乐观锁）"),
            }),
            &["taskId", "to"]
        ),
        tool(
            "add_comment",
            "给任务评论；进行中会引导智能体，已完成会触发返工，阻塞会解答解阻，审核中作为评审意见。",
            json!({
                "taskId": p_str("任务 ID"), "text": p_str("评论内容"),
            }),
            &["taskId", "text"]
        ),
        tool(
            "delete_task",
            "删除任务（管理员）。不可恢复，执行前必须向用户确认。",
            json!({ "taskId": p_str("任务 ID") }),
            &["taskId"]
        ),
        tool(
            "check_merge",
            "检测任务/交付项是否已合并。",
            json!({ "taskId": p_str("任务 ID") }),
            &["taskId"]
        ),
        tool(
            "set_task_pr",
            "为普通（非工作流）任务回填 PR/MR 链接。",
            json!({
                "taskId": p_str("任务 ID"), "prUrl": p_str("PR/MR 链接"),
            }),
            &["taskId", "prUrl"]
        ),
        tool(
            "set_delivery",
            "为工作流任务的某个交付项回填 PR/MR 链接。",
            json!({
                "taskId": p_str("任务 ID"), "deliveryId": p_str("交付项 ID"), "prUrl": p_str("PR/MR 链接"),
            }),
            &["taskId", "deliveryId", "prUrl"]
        ),
        tool(
            "rewind_workflow",
            "从指定工作流节点开始返工（管理员）。",
            json!({
                "taskId": p_str("任务 ID"), "fromStage": p_str("起始节点 key"),
            }),
            &["taskId", "fromStage"]
        ),
        tool(
            "skip_workflow_stage",
            "跳过工作流中的指定节点（管理员）。",
            json!({
                "taskId": p_str("任务 ID"), "stageKey": p_str("节点 key"),
            }),
            &["taskId", "stageKey"]
        ),
        tool("list_projects", "列出工作区项目。", json!({}), &[]),
        tool(
            "create_project",
            "创建项目。kind=git 需要 remoteUrl，kind=local 需要 localPath。",
            json!({
                "name": p_str("项目名称"),
                "kind": json!({ "type": "string", "enum": ["git", "local"], "description": "项目类型" }),
                "remoteUrl": p_str("git remote 地址"),
                "localPath": p_str("本地目录绝对路径"),
                "defaultBranch": p_str("默认分支，默认 main"),
            }),
            &["name", "kind"]
        ),
        tool(
            "delete_project",
            "删除项目（管理员）。执行前必须向用户确认。",
            json!({ "projectId": p_str("项目 ID") }),
            &["projectId"]
        ),
        tool(
            "test_project_connection",
            "测试项目连接：git ls-remote 或检查本地路径。",
            json!({ "projectId": p_str("项目 ID") }),
            &["projectId"]
        ),
        tool("list_workflows", "列出工作流模板。", json!({}), &[]),
        tool(
            "get_workflow",
            "查看工作流模板详情（含节点 DAG）。",
            json!({ "workflowId": p_str("工作流 ID") }),
            &["workflowId"]
        ),
        tool(
            "create_workflow",
            "创建工作流 DAG 模板（管理员）。",
            json!({
                "name": p_str("工作流名称"),
                "description": p_str("说明"),
                "variables": variables_schema.clone(),
                "stages": stages_schema.clone(),
            }),
            &["name", "stages"]
        ),
        tool(
            "update_workflow",
            "更新工作流模板（管理员）。需要提交完整 name + stages。",
            json!({
                "workflowId": p_str("工作流 ID"),
                "name": p_str("工作流名称"),
                "description": p_str("说明"),
                "variables": variables_schema,
                "stages": stages_schema,
            }),
            &["workflowId", "name", "stages"]
        ),
        tool(
            "delete_workflow",
            "删除工作流模板（管理员）。执行前必须向用户确认。",
            json!({ "workflowId": p_str("工作流 ID") }),
            &["workflowId"]
        ),
        tool(
            "run_workflow",
            "用一句话发起工作流：创建 ready 任务并按 DAG 自动推进。",
            json!({
                "workflowId": p_str("工作流 ID"),
                "title": p_str("任务标题"),
                "content": p_str("任务内容/提示词"),
                "projectId": p_str("主项目 ID"),
                "typeId": p_str("任务类型 ID"),
                "priority": p_int("优先级"),
                "tags": p_str_arr("标签列表"),
                "variables": json!({ "type": "object", "description": "工作流发起变量键值" }),
            }),
            &["workflowId", "title"]
        ),
        tool("list_skills", "列出 SKILL 库。", json!({}), &[]),
        tool(
            "create_skill",
            "创建 SKILL（管理员）。content 是 SKILL.md 全文。",
            json!({
                "name": p_str("SKILL 名称"),
                "description": p_str("简介"),
                "source": p_str("来源"),
                "version": p_str("版本，默认 1.0.0"),
                "content": p_str("SKILL.md 内容"),
            }),
            &["name"]
        ),
        tool(
            "update_skill",
            "更新 SKILL（管理员）。未提供的字段保持原值。",
            json!({
                "skillId": p_str("SKILL ID"),
                "name": p_str("名称"),
                "description": p_str("简介"),
                "source": p_str("来源"),
                "version": p_str("版本"),
                "content": p_str("SKILL.md 内容"),
            }),
            &["skillId", "name"]
        ),
        tool(
            "delete_skill",
            "删除 SKILL（管理员）。执行前必须向用户确认。",
            json!({ "skillId": p_str("SKILL ID") }),
            &["skillId"]
        ),
        tool(
            "list_agents",
            "列出工作区智能体，含绑定的 CLI、模型、SKILL 与 MCP。",
            json!({}),
            &[]
        ),
        tool(
            "create_agent",
            "创建智能体（管理员）。需要绑定一个已存在的 CLI 条目。",
            json!({
                "name": p_str("智能体名称"),
                "cliEntryId": p_str("CLI 条目 ID"),
                "model": p_str("模型名"),
                "reasoningEffort": p_str("推理强度：minimal/low/medium/high"),
                "systemPrompt": p_str("系统提示词"),
                "policy": p_str("审批策略：write_ask/full_auto 等"),
                "gates": p_obj("质量阀配置"),
                "maxConcurrent": p_int("最大并发"),
                "matchTags": p_str_arr("自动匹配的任务标签"),
                "skillIds": p_str_arr("授权的 SKILL ID 列表"),
                "mcpServers": json!({ "type": "array", "description": "授权的 MCP 列表", "items": { "type": "object", "properties": { "id": p_str("MCP ID"), "toolWhitelist": p_str_arr("工具白名单") }, "required": ["id"] } }),
            }),
            &["name", "cliEntryId"]
        ),
        tool(
            "update_agent",
            "更新智能体（管理员）。需要提交完整 name + cliEntryId。",
            json!({
                "agentId": p_str("智能体 ID"),
                "name": p_str("智能体名称"),
                "cliEntryId": p_str("CLI 条目 ID"),
                "model": p_str("模型名"),
                "reasoningEffort": p_str("推理强度"),
                "systemPrompt": p_str("系统提示词"),
                "policy": p_str("审批策略"),
                "gates": p_obj("质量阀配置"),
                "maxConcurrent": p_int("最大并发"),
                "matchTags": p_str_arr("任务标签"),
                "skillIds": p_str_arr("SKILL ID 列表"),
                "mcpServers": json!({ "type": "array", "description": "MCP 授权列表", "items": { "type": "object", "properties": { "id": p_str("MCP ID"), "toolWhitelist": p_str_arr("工具白名单") }, "required": ["id"] } }),
            }),
            &["agentId", "name", "cliEntryId"]
        ),
        tool(
            "pause_agent",
            "暂停或恢复智能体。默认暂停。",
            json!({
                "agentId": p_str("智能体 ID"), "paused": p_bool("true 暂停，false 恢复"),
            }),
            &["agentId"]
        ),
        tool(
            "delete_agent",
            "删除智能体（管理员）。执行前必须向用户确认。",
            json!({ "agentId": p_str("智能体 ID") }),
            &["agentId"]
        ),
        tool("list_task_types", "列出任务类型模板。", json!({}), &[]),
        tool(
            "instantiate_task_type",
            "用任务类型模板建卡：填变量并一键创建任务。",
            json!({
                "typeId": p_str("任务类型 ID"),
                "variables": p_obj("模板变量，如 {title, problem, expected}"),
                "projectId": p_str("项目 ID"),
                "priority": p_int("优先级"),
                "assigneeAgentId": p_str("指派智能体 ID"),
            }),
            &["typeId"]
        ),
        tool("list_mcp_servers", "列出工作区 MCP 服务。", json!({}), &[]),
        tool(
            "create_mcp_server",
            "注册 MCP 服务（管理员）。env 会加密存储。",
            json!({
                "name": p_str("服务名称"),
                "transport": json!({ "type": "string", "enum": ["stdio", "sse", "http"], "description": "传输方式" }),
                "command": p_str("stdio 命令"),
                "args": p_str_arr("stdio 参数"),
                "url": p_str("SSE/HTTP URL"),
                "env": p_obj("环境变量或 headers"),
            }),
            &["name", "transport"]
        ),
        tool(
            "delete_mcp_server",
            "删除 MCP 服务（管理员）。执行前必须向用户确认。",
            json!({ "mcpServerId": p_str("MCP 服务 ID") }),
            &["mcpServerId"]
        ),
        tool(
            "probe_mcp_server",
            "探测 MCP 服务可用性。",
            json!({ "mcpServerId": p_str("MCP 服务 ID") }),
            &["mcpServerId"]
        ),
        tool(
            "list_cli_entries",
            "列出可用 CLI 条目（含全局默认条目）。",
            json!({}),
            &[]
        ),
        tool(
            "create_cli_entry",
            "注册自定义 ACP CLI 条目（管理员）。",
            json!({
                "name": p_str("条目名称"),
                "command": p_str("启动命令"),
                "args": p_str_arr("启动参数"),
                "envTemplate": p_obj("环境变量模板"),
            }),
            &["name", "command"]
        ),
        tool(
            "delete_cli_entry",
            "删除自定义 CLI 条目（管理员）。执行前必须向用户确认。",
            json!({ "cliEntryId": p_str("CLI 条目 ID") }),
            &["cliEntryId"]
        ),
        tool(
            "probe_cli_entry",
            "探测 CLI 是否安装。",
            json!({ "cliEntryId": p_str("CLI 条目 ID") }),
            &["cliEntryId"]
        ),
        tool("list_members", "列出工作区成员与角色。", json!({}), &[]),
        tool(
            "invite_member",
            "生成工作区邀请码（管理员）。执行前先向用户确认角色与有效期。",
            json!({
                "role": json!({ "type": "string", "enum": ["admin", "member"], "description": "邀请角色" }),
                "expiresHours": p_int("有效期小时数"),
                "oneTime": p_bool("是否一次性邀请"),
            }),
            &[]
        ),
        tool(
            "fux_api",
            "通用 REST 兜底：当前用户身份调用任意 /api/v1 接口，服务端仍校验 RBAC 与状态机。仅当没有专用工具时使用；写操作前先向用户确认。常用：GET/POST /api/v1/workspaces/{workspaceId}/task-types，PATCH/DELETE /api/v1/task-types/{id}，GET /api/v1/nodes，POST /api/v1/nodes/pairing-codes，GET/POST /api/v1/workspaces/{workspaceId}/invitations，GET /api/v1/approvals，POST /api/v1/approvals/{id}/decide，GET /api/v1/workspaces/{workspaceId}/notifications。",
            json!({
                "method": json!({ "type": "string", "enum": ["GET", "POST", "PATCH", "PUT", "DELETE"], "description": "HTTP 方法" }),
                "path": p_str("以 /api/v1/ 开头的接口路径"),
                "body": p_obj("请求体（GET/DELETE 可省略）"),
            }),
            &["method", "path"]
        ),
    ])
}

pub async fn call_tool(name: &str, args: Value) -> Result<Value> {
    match name {
        "workspace_overview" => {
            let ws = ws();
            let workspace = api("GET", &format!("/api/v1/workspaces/{ws}"), None).await?;
            let board = api("GET", &format!("/api/v1/workspaces/{ws}/board"), None).await?;
            let projects = api("GET", &ws_path("/projects"), None).await?;
            let agents = api("GET", &ws_path("/agents"), None).await?;
            let workflows = api("GET", &ws_path("/workflows"), None).await?;
            let skills = api("GET", &ws_path("/skills"), None).await?;
            let task_types = api("GET", &ws_path("/task-types"), None).await?;
            let mcp_servers = api("GET", &ws_path("/mcp-servers"), None).await?;
            let cli_entries = api(
                "GET",
                &format!("/api/v1/cli-entries?workspaceId={ws}"),
                None,
            )
            .await?;
            let members = api("GET", &ws_path("/members"), None).await?;
            // 概览只返回配置索引，避免把 SKILL 全文 / 系统提示词全部塞进上下文
            let compact_agents: Vec<Value> = agents.as_array().into_iter().flatten().map(|a| json!({
                "id": a["id"], "name": a["name"], "cliEntryId": a["cliEntryId"], "model": a["model"],
                "reasoningEffort": a["reasoningEffort"], "paused": a["paused"],
                "maxConcurrent": a["maxConcurrent"], "matchTags": a["matchTags"],
                "skillIds": a["skillIds"], "mcpServers": a["mcpServers"],
            })).collect();
            let compact_workflows: Vec<Value> = workflows
                .as_array()
                .into_iter()
                .flatten()
                .map(|w| {
                    let stage_keys: Vec<Value> =
                        serde_json::from_str::<Vec<Value>>(w["stages"].as_str().unwrap_or("[]"))
                            .unwrap_or_default()
                            .into_iter()
                            .filter_map(|s| s.get("key").cloned())
                            .collect();
                    json!({
                        "id": w["id"], "name": w["name"], "description": w["description"],
                        "variables": w["variables"], "stageKeys": stage_keys
                    })
                })
                .collect();
            let compact_skills: Vec<Value> = skills.as_array().into_iter().flatten().map(|s| json!({
                "id": s["id"], "name": s["name"], "description": s["description"], "version": s["version"], "source": s["source"],
            })).collect();
            let compact_types: Vec<Value> = task_types.as_array().into_iter().flatten().map(|t| json!({
                "id": t["id"], "name": t["name"], "icon": t["icon"], "variablesSchema": t["variablesSchema"],
            })).collect();
            Ok(json!({
                "workspace": workspace, "board": board, "projects": projects, "agents": compact_agents,
                "workflows": compact_workflows, "skills": compact_skills, "taskTypes": compact_types,
                "mcpServers": mcp_servers, "cliEntries": cli_entries, "members": members,
            }))
        }
        "create_task" => {
            let board = board_id().await?;
            if board.is_empty() {
                bail!("当前工作区没有看板");
            }
            let title = require(&args, "title")?;
            let mut body =
                json!({ "boardId": board, "title": title, "content": text(&args, "content") });
            copy(
                &args,
                &mut body,
                &["projectId", "typeId", "priority", "tags", "assigneeAgentId"],
            );
            api("POST", "/api/v1/tasks", Some(body)).await
        }
        "list_tasks" => {
            let board = board_id().await?;
            let mut path = format!("/api/v1/tasks?boardId={board}");
            for (arg, param) in [("status", "status"), ("projectId", "projectId")] {
                let value = text(&args, arg);
                if !value.is_empty() {
                    path.push_str(&format!("&{param}={value}"));
                }
            }
            api("GET", &path, None).await
        }
        "get_task" => {
            let id = require(&args, "taskId")?;
            api("GET", &format!("/api/v1/tasks/{id}"), None).await
        }
        "update_task" => {
            let id = require(&args, "taskId")?;
            let mut body = json!({});
            copy(
                &args,
                &mut body,
                &[
                    "title",
                    "content",
                    "priority",
                    "tags",
                    "projectId",
                    "assigneeAgentId",
                    "laneId",
                ],
            );
            if body.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                bail!("没有要更新的字段");
            }
            api("PATCH", &format!("/api/v1/tasks/{id}"), Some(body)).await
        }
        "move_task" => {
            let id = require(&args, "taskId")?;
            let to = require(&args, "to")?;
            let mut body = json!({ "to": to, "reason": text(&args, "reason") });
            if text(&args, "reason").is_empty() {
                body["reason"] = json!("助理操作");
            }
            copy(&args, &mut body, &["expectedVersion"]);
            api(
                "POST",
                &format!("/api/v1/tasks/{id}/transition"),
                Some(body),
            )
            .await
        }
        "add_comment" => {
            let id = require(&args, "taskId")?;
            let body = json!({ "text": require(&args, "text")? });
            api("POST", &format!("/api/v1/tasks/{id}/comments"), Some(body)).await
        }
        "delete_task" => {
            let id = require(&args, "taskId")?;
            api("DELETE", &format!("/api/v1/tasks/{id}"), None).await
        }
        "check_merge" => {
            let id = require(&args, "taskId")?;
            api("POST", &format!("/api/v1/tasks/{id}/check-merge"), None).await
        }
        "set_task_pr" => {
            let id = require(&args, "taskId")?;
            let body = json!({ "prUrl": require(&args, "prUrl")? });
            api(
                "POST",
                &format!("/api/v1/tasks/{id}/pull-request"),
                Some(body),
            )
            .await
        }
        "set_delivery" => {
            let id = require(&args, "taskId")?;
            let body = json!({
                "deliveryId": require(&args, "deliveryId")?,
                "prUrl": require(&args, "prUrl")?,
            });
            api(
                "POST",
                &format!("/api/v1/tasks/{id}/deliveries"),
                Some(body),
            )
            .await
        }
        "rewind_workflow" => {
            let id = require(&args, "taskId")?;
            let body = json!({ "fromStage": require(&args, "fromStage")? });
            api("POST", &format!("/api/v1/tasks/{id}/rewind"), Some(body)).await
        }
        "skip_workflow_stage" => {
            let id = require(&args, "taskId")?;
            let key = require(&args, "stageKey")?;
            api(
                "POST",
                &format!("/api/v1/tasks/{id}/stages/{key}/skip"),
                None,
            )
            .await
        }

        "list_projects" => api("GET", &ws_path("/projects"), None).await,
        "create_project" => {
            let name = require(&args, "name")?;
            let kind = require(&args, "kind")?;
            if kind != "git" && kind != "local" {
                bail!("kind 只能为 git/local");
            }
            let mut body = json!({ "name": name, "kind": kind });
            copy(
                &args,
                &mut body,
                &["remoteUrl", "localPath", "defaultBranch"],
            );
            api("POST", &ws_path("/projects"), Some(body)).await
        }
        "delete_project" => {
            let id = require(&args, "projectId")?;
            api("DELETE", &format!("/api/v1/projects/{id}"), None).await
        }
        "test_project_connection" => {
            let id = require(&args, "projectId")?;
            api(
                "POST",
                &format!("/api/v1/projects/{id}/test-connection"),
                None,
            )
            .await
        }

        "list_workflows" => api("GET", &ws_path("/workflows"), None).await,
        "get_workflow" => {
            let id = require(&args, "workflowId")?;
            api("GET", &format!("/api/v1/workflows/{id}"), None).await
        }
        "create_workflow" => {
            let name = require(&args, "name")?;
            let stages = args.get("stages").cloned().unwrap_or(json!([]));
            if !stages.is_array() {
                bail!("stages 必须是数组");
            }
            let variables = args.get("variables").cloned().unwrap_or(json!([]));
            let body = json!({
                "name": name,
                "description": text(&args, "description"),
                "variables": variables,
                "stages": stages
            });
            api("POST", &ws_path("/workflows"), Some(body)).await
        }
        "update_workflow" => {
            let id = require(&args, "workflowId")?;
            let name = require(&args, "name")?;
            let stages = args.get("stages").cloned().unwrap_or(json!([]));
            if !stages.is_array() {
                bail!("stages 必须是数组");
            }
            let variables = args.get("variables").cloned().unwrap_or(json!([]));
            let body = json!({
                "name": name,
                "description": text(&args, "description"),
                "variables": variables,
                "stages": stages
            });
            api("PATCH", &format!("/api/v1/workflows/{id}"), Some(body)).await
        }
        "delete_workflow" => {
            let id = require(&args, "workflowId")?;
            api("DELETE", &format!("/api/v1/workflows/{id}"), None).await
        }
        "run_workflow" => {
            let id = require(&args, "workflowId")?;
            let title = require(&args, "title")?;
            let mut body = json!({ "title": title, "content": text(&args, "content") });
            copy(
                &args,
                &mut body,
                &["projectId", "typeId", "priority", "tags", "variables"],
            );
            api("POST", &format!("/api/v1/workflows/{id}/run"), Some(body)).await
        }

        "list_skills" => api("GET", &ws_path("/skills"), None).await,
        "create_skill" => {
            let name = require(&args, "name")?;
            let mut body = json!({ "name": name });
            copy(
                &args,
                &mut body,
                &["description", "source", "version", "content"],
            );
            api("POST", &ws_path("/skills"), Some(body)).await
        }
        "update_skill" => {
            let id = require(&args, "skillId")?;
            let name = require(&args, "name")?;
            let mut body = json!({ "name": name });
            copy(
                &args,
                &mut body,
                &["description", "source", "version", "content"],
            );
            api("PATCH", &format!("/api/v1/skills/{id}"), Some(body)).await
        }
        "delete_skill" => {
            let id = require(&args, "skillId")?;
            api("DELETE", &format!("/api/v1/skills/{id}"), None).await
        }

        "list_agents" => api("GET", &ws_path("/agents"), None).await,
        "create_agent" => {
            let name = require(&args, "name")?;
            let cli = require(&args, "cliEntryId")?;
            let mut body = json!({ "name": name, "cliEntryId": cli });
            copy(
                &args,
                &mut body,
                &[
                    "model",
                    "reasoningEffort",
                    "systemPrompt",
                    "policy",
                    "gates",
                    "maxConcurrent",
                    "matchTags",
                    "skillIds",
                    "mcpServers",
                ],
            );
            api("POST", &ws_path("/agents"), Some(body)).await
        }
        "update_agent" => {
            let id = require(&args, "agentId")?;
            let name = require(&args, "name")?;
            let cli = require(&args, "cliEntryId")?;
            let mut body = json!({ "name": name, "cliEntryId": cli });
            copy(
                &args,
                &mut body,
                &[
                    "model",
                    "reasoningEffort",
                    "systemPrompt",
                    "policy",
                    "gates",
                    "maxConcurrent",
                    "matchTags",
                    "skillIds",
                    "mcpServers",
                ],
            );
            api("PATCH", &format!("/api/v1/agents/{id}"), Some(body)).await
        }
        "pause_agent" => {
            let id = require(&args, "agentId")?;
            let paused = args.get("paused").and_then(|v| v.as_bool()).unwrap_or(true);
            api(
                "POST",
                &format!("/api/v1/agents/{id}/pause"),
                Some(json!({ "paused": paused })),
            )
            .await
        }
        "delete_agent" => {
            let id = require(&args, "agentId")?;
            api("DELETE", &format!("/api/v1/agents/{id}"), None).await
        }

        "list_task_types" => api("GET", &ws_path("/task-types"), None).await,
        "instantiate_task_type" => {
            let id = require(&args, "typeId")?;
            let mut body =
                json!({ "variables": args.get("variables").cloned().unwrap_or(json!({})) });
            copy(
                &args,
                &mut body,
                &["projectId", "priority", "assigneeAgentId"],
            );
            api(
                "POST",
                &format!("/api/v1/task-types/{id}/instantiate"),
                Some(body),
            )
            .await
        }

        "list_mcp_servers" => api("GET", &ws_path("/mcp-servers"), None).await,
        "create_mcp_server" => {
            let name = require(&args, "name")?;
            let transport = require(&args, "transport")?;
            if !["stdio", "sse", "http"].contains(&transport.as_str()) {
                bail!("transport 只能为 stdio/sse/http");
            }
            let mut body = json!({ "name": name, "transport": transport });
            copy(&args, &mut body, &["command", "args", "url", "env"]);
            api("POST", &ws_path("/mcp-servers"), Some(body)).await
        }
        "delete_mcp_server" => {
            let id = require(&args, "mcpServerId")?;
            api("DELETE", &format!("/api/v1/mcp-servers/{id}"), None).await
        }
        "probe_mcp_server" => {
            let id = require(&args, "mcpServerId")?;
            api("POST", &format!("/api/v1/mcp-servers/{id}/probe"), None).await
        }

        "list_cli_entries" => {
            api(
                "GET",
                &format!("/api/v1/cli-entries?workspaceId={}", ws()),
                None,
            )
            .await
        }
        "create_cli_entry" => {
            let name = require(&args, "name")?;
            let command = require(&args, "command")?;
            let mut body = json!({ "workspaceId": ws(), "name": name, "command": command });
            copy(&args, &mut body, &["args", "envTemplate"]);
            api("POST", "/api/v1/cli-entries", Some(body)).await
        }
        "delete_cli_entry" => {
            let id = require(&args, "cliEntryId")?;
            api("DELETE", &format!("/api/v1/cli-entries/{id}"), None).await
        }
        "probe_cli_entry" => {
            let id = require(&args, "cliEntryId")?;
            api("POST", &format!("/api/v1/cli-entries/{id}/probe"), None).await
        }

        "list_members" => api("GET", &ws_path("/members"), None).await,
        "invite_member" => {
            let mut body =
                json!({ "role": args.get("role").and_then(|v| v.as_str()).unwrap_or("member") });
            copy(&args, &mut body, &["expiresHours", "oneTime"]);
            api("POST", &ws_path("/invitations"), Some(body)).await
        }

        "fux_api" => {
            let method = text(&args, "method").to_uppercase();
            if !["GET", "POST", "PATCH", "PUT", "DELETE"].contains(&method.as_str()) {
                bail!("method 只能是 GET/POST/PATCH/PUT/DELETE");
            }
            let path = require(&args, "path")?;
            if !path.starts_with("/api/v1/") {
                bail!("path 必须以 /api/v1/ 开头");
            }
            let body = args.get("body").filter(|v| !v.is_null()).cloned();
            api(&method, &path, body).await
        }
        _ => bail!("未知工具: {name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn exposes_workspace_management_tools() {
        let list = tools();
        let tools = list.as_array().expect("tool list");
        let mut names = HashSet::new();
        for tool in tools {
            let name = tool.get("name").and_then(Value::as_str).expect("tool name");
            assert!(names.insert(name.to_string()), "重复工具名: {name}");
            assert!(
                tool.get("inputSchema").is_some(),
                "缺少 inputSchema: {name}"
            );
        }
        for expected in [
            "workspace_overview",
            "create_task",
            "get_task",
            "create_project",
            "create_workflow",
            "run_workflow",
            "create_skill",
            "create_agent",
            "list_mcp_servers",
            "list_cli_entries",
            "invite_member",
            "fux_api",
        ] {
            assert!(names.contains(expected), "缺少工具: {expected}");
        }
        assert!(tools.len() >= 40, "工具数量异常: {}", tools.len());
    }
}
