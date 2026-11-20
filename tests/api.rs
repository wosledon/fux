//! 集成测试：API 全流程 / 状态机闭环 / mock agent 端到端 / 质量阀。
use actix_web::{App, test, web};
use fux::server::{AppState, NodeRegistry, api, db, scheduler, watchdog};
use serde_json::{Value, json};
use tokio::sync::{Mutex, broadcast};

struct Ctx {
    state: web::Data<AppState>,
    _dir: tempfile::TempDir,
}

async fn setup() -> Ctx {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::init(dir.path()).await.unwrap();
    let (broadcast, _) = broadcast::channel(256);
    let state = web::Data::new(AppState {
        db: pool,
        jwt_secret: "test-secret".into(),
        data_dir: dir.path().to_path_buf(),
        broadcast,
        nodes: NodeRegistry::default(),
        local_runs: Mutex::new(std::collections::HashMap::new()),
        assistant_pool: Default::default(),
        api_base: "http://127.0.0.1:8080".into(),
    });
    Ctx { state, _dir: dir }
}

macro_rules! app {
    ($ctx:expr) => {
        test::init_service(
            App::new()
                .app_data($ctx.state.clone())
                .service(web::scope("/api/v1").configure(api::routes)),
        )
        .await
    };
}

async fn call<S, B>(app: &S, req: actix_web::test::TestRequest) -> (u16, Value)
where
    S: actix_web::dev::Service<
            actix_http::Request,
            Response = actix_web::dev::ServiceResponse<B>,
            Error = actix_web::Error,
        >,
    B: actix_web::body::MessageBody,
{
    let resp = test::call_service(app, req.to_request()).await;
    let status = resp.status().as_u16();
    let body = test::read_body(resp).await;
    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, v)
}

async fn register<S, B>(app: &S, name: &str) -> String
where
    S: actix_web::dev::Service<
            actix_http::Request,
            Response = actix_web::dev::ServiceResponse<B>,
            Error = actix_web::Error,
        >,
    B: actix_web::body::MessageBody,
{
    let (s, v) = call(
        app,
        test::TestRequest::post()
            .uri("/api/v1/auth/register")
            .set_json(json!({ "username": name, "password": "password123" })),
    )
    .await;
    assert_eq!(s, 200, "register: {v}");
    v["token"].as_str().unwrap().to_string()
}

fn auth(req: actix_web::test::TestRequest, token: &str) -> actix_web::test::TestRequest {
    req.insert_header(("Authorization", format!("Bearer {token}")))
}

async fn call_text<S, B>(app: &S, req: actix_web::test::TestRequest) -> (u16, String)
where
    S: actix_web::dev::Service<
            actix_http::Request,
            Response = actix_web::dev::ServiceResponse<B>,
            Error = actix_web::Error,
        >,
    B: actix_web::body::MessageBody,
{
    let resp = test::call_service(app, req.to_request()).await;
    let status = resp.status().as_u16();
    let body = test::read_body(resp).await;
    (status, String::from_utf8_lossy(&body).to_string())
}

async fn create_ws<S, B>(app: &S, token: &str) -> (String, String)
where
    S: actix_web::dev::Service<
            actix_http::Request,
            Response = actix_web::dev::ServiceResponse<B>,
            Error = actix_web::Error,
        >,
    B: actix_web::body::MessageBody,
{
    let (s, v) = call(
        app,
        auth(
            test::TestRequest::post()
                .uri("/api/v1/workspaces")
                .set_json(json!({ "name": "测试工作区" })),
            token,
        ),
    )
    .await;
    assert_eq!(s, 200, "create ws: {v}");
    let ws = v["id"].as_str().unwrap().to_string();
    let (s, v) = call(
        app,
        auth(
            test::TestRequest::get().uri(&format!("/api/v1/workspaces/{ws}/board")),
            token,
        ),
    )
    .await;
    assert_eq!(s, 200);
    let board = v["board"]["id"].as_str().unwrap().to_string();
    assert_eq!(v["lanes"].as_array().unwrap().len(), 7, "默认 7 条语义泳道");
    (ws, board)
}

#[actix_web::test]
async fn auth_workspace_and_rbac() {
    let ctx = setup().await;
    let app = app!(ctx);
    let owner = register(&app, "owner1").await;
    let (ws, board) = create_ws(&app, &owner).await;

    // 建卡（待规划）
    let (s, task) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri("/api/v1/tasks")
                .set_json(json!({ "boardId": board, "title": "任务A", "content": "内容" })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200);
    let tid = task["id"].as_str().unwrap().to_string();
    assert_eq!(task["status"], "planning");

    // 非法转移：planning → done
    let (s, _) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/tasks/{tid}/transition"))
                .set_json(json!({ "to": "done" })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 409);

    // planning → ready
    let (s, v) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/tasks/{tid}/transition"))
                .set_json(json!({ "to": "ready" })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200);
    assert_eq!(v["status"], "ready");

    // 用户不能手动认领（ready → in_progress 仅调度器）
    let (s, _) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/tasks/{tid}/transition"))
                .set_json(json!({ "to": "in_progress" })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 403);

    // 邀请 member
    let member = register(&app, "member1").await;
    let (s, inv) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/workspaces/{ws}/invitations"))
                .set_json(json!({ "role": "member" })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200);
    let code = inv["code"].as_str().unwrap();
    let (s, _) = call(
        &app,
        auth(
            test::TestRequest::post().uri(&format!("/api/v1/invitations/{code}/accept")),
            &member,
        ),
    )
    .await;
    assert_eq!(s, 200);
    // 一次性码失效
    let (s, _) = call(
        &app,
        auth(
            test::TestRequest::post().uri(&format!("/api/v1/invitations/{code}/accept")),
            &member,
        ),
    )
    .await;
    assert_eq!(s, 409);

    // member 可建卡、不可删工作区
    let (s, _) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri("/api/v1/tasks")
                .set_json(json!({ "boardId": board, "title": "member 建的卡" })),
            &member,
        ),
    )
    .await;
    assert_eq!(s, 200);
    let (s, _) = call(
        &app,
        auth(
            test::TestRequest::delete().uri(&format!("/api/v1/workspaces/{ws}")),
            &member,
        ),
    )
    .await;
    assert_eq!(s, 403);

    // 事件时间线：task.created 在 task.moved 之前（钉住顺序）
    let events = fux::server::events::list_after(&ctx.state.db, &tid, 0)
        .await
        .unwrap();
    let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(kinds[0], "task.created");
    assert!(kinds.contains(&"task.moved"));
    assert!(events.windows(2).all(|w| w[0].seq < w[1].seq), "seq 单调");
}

#[actix_web::test]
async fn review_rework_archive_loop() {
    let ctx = setup().await;
    let app = app!(ctx);
    let owner = register(&app, "owner2").await;
    let (_ws, board) = create_ws(&app, &owner).await;
    let (_, task) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri("/api/v1/tasks")
                .set_json(json!({ "boardId": board, "title": "闭环任务" })),
            &owner,
        ),
    )
    .await;
    let tid = task["id"].as_str().unwrap().to_string();

    // 模拟智能体/系统推进：ready → in_progress → review
    let actor = "system";
    fux::server::domain::move_task(&ctx.state, &tid, "ready", actor, Default::default())
        .await
        .unwrap();
    fux::server::domain::move_task(&ctx.state, &tid, "in_progress", actor, Default::default())
        .await
        .unwrap();
    fux::server::domain::move_task(&ctx.state, &tid, "review", actor, Default::default())
        .await
        .unwrap();

    // 未合并且未人工确认 → 不可批准
    let (s, _) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/tasks/{tid}/transition"))
                .set_json(json!({ "to": "done" })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 409);

    // admin 人工确认合并 → 批准
    let (s, v) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/tasks/{tid}/transition"))
                .set_json(json!({ "to": "done", "manualConfirm": true })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["status"], "done");

    // 已完成任务评论 → 返工回待办（T7）
    let (s, v) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/tasks/{tid}/comments"))
                .set_json(json!({ "text": "还有问题，请返工" })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200);
    assert_eq!(v["status"], "ready");

    // 再走到 done → 归档
    fux::server::domain::move_task(&ctx.state, &tid, "in_progress", actor, Default::default())
        .await
        .unwrap();
    fux::server::domain::move_task(&ctx.state, &tid, "review", actor, Default::default())
        .await
        .unwrap();
    sqlx::query("UPDATE tasks SET pr_state = 'merged' WHERE id = ?")
        .bind(&tid)
        .execute(&ctx.state.db)
        .await
        .unwrap();
    let (s, v) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/tasks/{tid}/transition"))
                .set_json(json!({ "to": "done" })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200, "检测到合并后可直接批准: {v}");
    let (s, v) = call(
        &app,
        auth(
            test::TestRequest::post().uri(&format!("/api/v1/tasks/{tid}/archive")),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200);
    assert_eq!(v["status"], "archived");
}

/// 端到端：mock agent 认领 → 执行 → git 交付 → review → 批准。
#[actix_web::test]
async fn e2e_mock_agent_delivery() {
    let ctx = setup().await;
    let app = app!(ctx);
    let owner = register(&app, "owner3").await;
    let (ws, board) = create_ws(&app, &owner).await;

    // 本地 git 项目
    let repo = ctx._dir.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.email", "t@t"],
        vec!["config", "user.name", "t"],
    ] {
        let out = std::process::Command::new("git")
            .args(&args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    std::fs::write(repo.join("a.txt"), "hello").unwrap();
    for args in [vec!["add", "-A"], vec!["commit", "-m", "init"]] {
        let out = std::process::Command::new("git")
            .args(&args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(out.status.success());
    }

    let (_, project) = call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/workspaces/{ws}/projects"))
        .set_json(json!({ "name": "本地仓库", "kind": "local", "localPath": repo.to_string_lossy() })), &owner)).await;
    let pid = project["id"].as_str().unwrap().to_string();

    // CLI 条目指向测试 mock 二进制 + 智能体
    let mock_bin = env!("CARGO_BIN_EXE_fux-mock-agent");
    let (_, cli) = call(&app, auth(test::TestRequest::post().uri("/api/v1/cli-entries")
        .set_json(json!({ "workspaceId": ws, "name": "MockTest", "command": mock_bin, "args": [] })), &owner)).await;
    let cli_id = cli["id"].as_str().unwrap().to_string();
    let (_, agent) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/workspaces/{ws}/agents"))
                .set_json(
                    json!({ "name": "测试智能体", "cliEntryId": cli_id, "policy": "full_auto" }),
                ),
            &owner,
        ),
    )
    .await;
    let aid = agent["id"].as_str().unwrap().to_string();

    let (_, task) = call(&app, auth(test::TestRequest::post().uri("/api/v1/tasks")
        .set_json(json!({ "boardId": board, "title": "端到端任务", "content": "请写一个文件", "projectId": pid, "assigneeAgentId": aid })), &owner)).await;
    let tid = task["id"].as_str().unwrap().to_string();
    call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/tasks/{tid}/transition"))
                .set_json(json!({ "to": "ready" })),
            &owner,
        ),
    )
    .await;

    scheduler::spawn(ctx.state.clone());

    // 等待自动认领 → 执行 → review（无 remote → 本地合并模式）
    let mut status = String::new();
    for _ in 0..80 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let t = fux::server::domain::load_task(&ctx.state, &tid)
            .await
            .unwrap();
        status = t.status.clone();
        if status == "review" || status == "blocked" {
            break;
        }
    }
    assert_eq!(status, "review", "mock agent 应完成并进入审核");
    let t = fux::server::domain::load_task(&ctx.state, &tid)
        .await
        .unwrap();
    assert_eq!(
        t.pr_state.as_deref(),
        Some("local"),
        "无 remote → 本地合并模式"
    );
    assert!(t.branch.as_deref().unwrap_or("").starts_with("fux/task-"));

    // 时间线关键事件顺序：run.started < agent.message < pr.linked < task.moved(review)
    let events = fux::server::events::list_after(&ctx.state.db, &tid, 0)
        .await
        .unwrap();
    let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
    let pos = |k: &str| {
        kinds
            .iter()
            .position(|x| *x == k)
            .unwrap_or_else(|| panic!("缺少事件 {k}, 实际: {kinds:?}"))
    };
    assert!(pos("run.started") < pos("agent.message"));
    assert!(pos("agent.message") < pos("tool.call"));
    assert!(pos("pr.linked") < kinds.len());
    assert!(kinds.contains(&"task.moved"));

    // worktree 中确有 mock 产出且已提交
    let wt = fux::git::worktree_dir(&ctx.state.data_dir, &tid);
    assert!(wt.join("demo.txt").exists(), "mock 应写入 demo.txt");
    let log = std::process::Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(&wt)
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&log.stdout).contains("fux(task)"),
        "应有交付提交"
    );

    // 人工确认批准 → done；评论返工 → ready
    let (s, _) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/tasks/{tid}/transition"))
                .set_json(json!({ "to": "done", "manualConfirm": true })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200);
    let (s, v) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/tasks/{tid}/comments"))
                .set_json(json!({ "text": "返工" })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200);
    assert_eq!(v["status"], "ready");
}

/// 工作流：DAG 校验、两节点自动推进、多节点 Run、交付、返工。
#[actix_web::test]
async fn workflow_dag_run_and_rewind() {
    let ctx = setup().await;
    let app = app!(ctx);
    let owner = register(&app, "wf-owner").await;
    let (ws, _board) = create_ws(&app, &owner).await;

    let repo = ctx._dir.path().join("wfrepo");
    std::fs::create_dir_all(&repo).unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.email", "t@t"],
        vec!["config", "user.name", "t"],
    ] {
        let out = std::process::Command::new("git")
            .args(&args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(out.status.success());
    }
    std::fs::write(repo.join("a.txt"), "hello").unwrap();
    for args in [vec!["add", "-A"], vec!["commit", "-m", "init"]] {
        assert!(
            std::process::Command::new("git")
                .args(&args)
                .current_dir(&repo)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let (_, project) = call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/workspaces/{ws}/projects"))
        .set_json(json!({ "name": "WF 仓库", "kind": "local", "localPath": repo.to_string_lossy() })), &owner)).await;
    let pid = project["id"].as_str().unwrap().to_string();

    let mock_bin = env!("CARGO_BIN_EXE_fux-mock-agent");
    let (_, cli) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri("/api/v1/cli-entries")
                .set_json(
                    json!({ "workspaceId": ws, "name": "MockWF", "command": mock_bin, "args": [] }),
                ),
            &owner,
        ),
    )
    .await;
    let (_, agent) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/workspaces/{ws}/agents"))
                .set_json(
                    json!({ "name": "WF 智能体", "cliEntryId": cli["id"], "policy": "full_auto" }),
                ),
            &owner,
        ),
    )
    .await;
    let aid = agent["id"].as_str().unwrap().to_string();

    // 循环依赖必须被拒绝
    let (s, _) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/workspaces/{ws}/workflows"))
                .set_json(json!({ "name": "坏流程", "stages": [
            { "key": "a", "name": "A", "agentId": aid, "dependsOn": ["b"] },
            { "key": "b", "name": "B", "agentId": aid, "dependsOn": ["a"] }
        ] })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 400, "循环依赖应被拒绝");

    // 两节点串行（同项目）
    let (s, wf) = call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/workspaces/{ws}/workflows"))
        .set_json(json!({ "name": "设计→开发", "stages": [
            { "key": "design", "name": "设计", "agentId": aid, "projectId": pid },
            { "key": "dev", "name": "开发", "agentId": aid, "projectId": pid, "dependsOn": ["design"] }
        ] })), &owner)).await;
    assert_eq!(s, 200, "{wf}");
    let wfid = wf["id"].as_str().unwrap().to_string();

    // 入口发起 → 直接 ready
    let (s, task) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/workflows/{wfid}/run"))
                .set_json(json!({ "title": "工作流任务", "content": "按流程实现" })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200, "{task}");
    let tid = task["id"].as_str().unwrap().to_string();
    assert_eq!(task["status"], "ready");

    scheduler::spawn(ctx.state.clone());

    let mut status = String::new();
    for _ in 0..120 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        status = fux::server::domain::load_task(&ctx.state, &tid)
            .await
            .unwrap()
            .status;
        if status == "review" || status == "blocked" {
            break;
        }
    }
    assert_eq!(status, "review", "工作流节点应全部完成并进入审核");

    let runs = sqlx::query_as::<_, fux::server::models::Run>(
        "SELECT * FROM runs WHERE task_id = ? ORDER BY started_at",
    )
    .bind(&tid)
    .fetch_all(&ctx.state.db)
    .await
    .unwrap();
    let keys: Vec<&str> = runs.iter().filter_map(|r| r.stage_key.as_deref()).collect();
    assert!(
        keys.contains(&"design") && keys.contains(&"dev"),
        "两个节点都应执行: {keys:?}"
    );
    assert!(
        runs.iter().all(|r| r.status == "finished"),
        "节点应全部成功"
    );
    assert!(
        runs.iter().all(|r| r.head_sha.is_some()),
        "应记录节点产物 commit: {:?}",
        runs.iter()
            .map(|r| (&r.stage_key, &r.status, &r.head_sha, &r.branch))
            .collect::<Vec<_>>()
    );

    let deliveries = sqlx::query_as::<_, fux::server::models::Delivery>(
        "SELECT * FROM deliveries WHERE task_id = ?",
    )
    .bind(&tid)
    .fetch_all(&ctx.state.db)
    .await
    .unwrap();
    assert_eq!(deliveries.len(), 1, "同项目汇点应生成一条交付");
    assert_eq!(deliveries[0].node_key, "dev");
    assert_eq!(deliveries[0].pr_state.as_deref(), Some("local"));

    let events = fux::server::events::list_after(&ctx.state.db, &tid, 0)
        .await
        .unwrap();
    let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
    for k in [
        "workflow.started",
        "workflow.stage_completed",
        "workflow.completed",
    ] {
        assert!(kinds.contains(&k), "缺少事件 {k}: {kinds:?}");
    }

    // 驳回返工：只重置汇点节点，重跑后重新进入审核
    let old_dev_run = runs
        .iter()
        .find(|r| r.stage_key.as_deref() == Some("dev"))
        .unwrap()
        .id
        .clone();
    let (s, v) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/tasks/{tid}/transition"))
                .set_json(json!({ "to": "ready", "reason": "请调整实现" })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200, "驳回应成功: {v}");
    for _ in 0..120 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        status = fux::server::domain::load_task(&ctx.state, &tid)
            .await
            .unwrap()
            .status;
        if status == "review" || status == "blocked" {
            break;
        }
    }
    assert_eq!(status, "review", "驳回后应重跑汇点节点并回到审核");
    let new_dev_run: String = sqlx::query_scalar(
        "SELECT id FROM runs WHERE task_id = ? AND stage_key = 'dev' ORDER BY started_at DESC LIMIT 1",
    )
    .bind(&tid)
    .fetch_one(&ctx.state.db)
    .await
    .unwrap();
    assert_ne!(old_dev_run, new_dev_run, "汇点节点应生成新的 Run");
    let delivery_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM deliveries WHERE task_id = ?")
            .bind(&tid)
            .fetch_one(&ctx.state.db)
            .await
            .unwrap();
    assert_eq!(delivery_count, 1, "重跑后应重新生成一条交付");

    // 从 design 返工：重置该节点及下游
    let (s, v) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/tasks/{tid}/rewind"))
                .set_json(json!({ "fromStage": "design" })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert!(
        matches!(v["status"].as_str(), Some("ready") | Some("in_progress")),
        "返工后应重新排队或已被调度器认领: {v}"
    );

    // 重新跑完后可人工确认批准
    for _ in 0..120 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        status = fux::server::domain::load_task(&ctx.state, &tid)
            .await
            .unwrap()
            .status;
        if status == "review" || status == "blocked" {
            break;
        }
    }
    assert_eq!(status, "review");
    let (s, _) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/tasks/{tid}/transition"))
                .set_json(json!({ "to": "done", "manualConfirm": true })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200, "全部交付已人工确认后可批准");
}

/// 突然中断：自动重试 ≤2 次后阻塞。
#[actix_web::test]
async fn e2e_interrupt_retry_then_block() {
    let ctx = setup().await;
    let app = app!(ctx);
    let owner = register(&app, "owner4").await;
    let (ws, board) = create_ws(&app, &owner).await;
    let mock_bin = env!("CARGO_BIN_EXE_fux-mock-agent");
    let (_, cli) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri("/api/v1/cli-entries")
                .set_json(json!({ "workspaceId": ws, "name": "MockFail", "command": mock_bin })),
            &owner,
        ),
    )
    .await;
    let (_, agent) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/workspaces/{ws}/agents"))
                .set_json(
                    json!({ "name": "失败智能体", "cliEntryId": cli["id"].as_str().unwrap() }),
                ),
            &owner,
        ),
    )
    .await;
    let aid = agent["id"].as_str().unwrap().to_string();
    let (_, task) = call(&app, auth(test::TestRequest::post().uri("/api/v1/tasks")
        .set_json(json!({ "boardId": board, "title": "中断任务", "content": "FAIL 模拟崩溃", "assigneeAgentId": aid })), &owner)).await;
    let tid = task["id"].as_str().unwrap().to_string();
    call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/tasks/{tid}/transition"))
                .set_json(json!({ "to": "ready" })),
            &owner,
        ),
    )
    .await;
    scheduler::spawn(ctx.state.clone());

    let mut status = String::new();
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let t = fux::server::domain::load_task(&ctx.state, &tid)
            .await
            .unwrap();
        status = t.status.clone();
        if status == "blocked" {
            break;
        }
    }
    assert_eq!(status, "blocked", "中断重试耗尽后应阻塞");
    let runs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE task_id = ?")
        .bind(&tid)
        .fetch_one(&ctx.state.db)
        .await
        .unwrap();
    assert_eq!(runs, 3, "1 次原始 + 2 次自动重试");
    // 每个 run 都应记录真实 ACP 会话 id（session_started 建立即落库），
    // 且重试 run 续接的是同一个会话（session/load）
    let sids: Vec<Option<String>> =
        sqlx::query_scalar("SELECT acp_session_id FROM runs WHERE task_id = ? ORDER BY started_at")
            .bind(&tid)
            .fetch_all(&ctx.state.db)
            .await
            .unwrap();
    assert_eq!(sids.len(), 3, "应取到 3 个 run 的会话 id");
    let first = sids[0].clone().expect("run 应记录 ACP 会话 id");
    assert!(!first.is_empty(), "会话 id 不应为空");
    for s in &sids[1..] {
        assert_eq!(
            s.as_deref(),
            Some(first.as_str()),
            "重试 run 应续接同一 ACP 会话"
        );
    }
    let t = fux::server::domain::load_task(&ctx.state, &tid)
        .await
        .unwrap();
    assert!(t.block_reason.unwrap_or_default().contains("运行失败"));
}

/// 质量阀：连续相同工具调用 → 熔断阻塞（不自动重试）。
#[actix_web::test]
async fn watchdog_loop_trip() {
    let ctx = setup().await;
    let app = app!(ctx);
    let owner = register(&app, "owner5").await;
    let (ws, board) = create_ws(&app, &owner).await;
    let (_, task) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri("/api/v1/tasks")
                .set_json(json!({ "boardId": board, "title": "循环任务" })),
            &owner,
        ),
    )
    .await;
    let tid = task["id"].as_str().unwrap().to_string();

    // 伪造运行中状态 + 相同工具调用
    let run_id = fux::new_id();
    sqlx::query("UPDATE tasks SET status = 'in_progress', lease_run_id = ?, lease_expires_at = ? WHERE id = ?")
        .bind(&run_id).bind(fux::server::gitflow::lease_deadline()).bind(&tid).execute(&ctx.state.db).await.unwrap();
    let agent_id = fux::new_id();
    sqlx::query(
        "INSERT INTO agents (id, workspace_id, name, cli_entry_id) VALUES (?,?,?, 'cli-mock')",
    )
    .bind(&agent_id)
    .bind(&ws)
    .bind("循环智能体")
    .execute(&ctx.state.db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO runs (id, task_id, agent_id, node_id, status, last_event_at) VALUES (?,?,?,?, 'running', ?)")
        .bind(&run_id).bind(&tid).bind(&agent_id).bind("local").bind(fux::now()).execute(&ctx.state.db).await.unwrap();
    for i in 0..5 {
        fux::server::events::append(&ctx.state.db, &ctx.state.broadcast, &tid, "tool.call", "agent:x", Some(&run_id),
            json!({ "toolCallId": format!("c{i}"), "title": "读同一文件", "kind": "read", "rawInput": { "path": "same.txt" } })).await.unwrap();
    }

    // 5 次不再触发（给“重复确认/重读文件”留余量）
    watchdog::scan_once(&ctx.state).await.unwrap();
    let t = fux::server::domain::load_task(&ctx.state, &tid)
        .await
        .unwrap();
    assert_eq!(t.status, "in_progress", "5 次相同调用不应熔断");

    for i in 5..10 {
        fux::server::events::append(&ctx.state.db, &ctx.state.broadcast, &tid, "tool.call", "agent:x", Some(&run_id),
            json!({ "toolCallId": format!("c{i}"), "title": "读同一文件", "kind": "read", "rawInput": { "path": "same.txt" } })).await.unwrap();
    }

    watchdog::scan_once(&ctx.state).await.unwrap();
    let t = fux::server::domain::load_task(&ctx.state, &tid)
        .await
        .unwrap();
    assert_eq!(t.status, "blocked");
    assert!(
        t.block_reason
            .unwrap_or_default()
            .contains("质量阀熔断 [loop]")
    );
    let kinds: Vec<String> = fux::server::events::list_after(&ctx.state.db, &tid, 0)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert!(kinds.contains(&"watchdog.loop".to_string()));
}

/// AI 助理：多会话 CRUD + mock 智能体流式对话。
#[actix_web::test]
async fn assistant_sessions_and_chat() {
    let ctx = setup().await;
    let app = app!(ctx);
    let owner = register(&app, "assistant-owner").await;
    let (ws, _board) = create_ws(&app, &owner).await;

    // 无智能体：拒绝且不产生会话
    let (s, _) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri("/api/v1/assistant/chat")
                .set_json(json!({ "workspaceId": ws, "message": "你好" })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 400, "没有智能体时应提示先创建");
    let (s, v) = call(
        &app,
        auth(
            test::TestRequest::get().uri(&format!("/api/v1/workspaces/{ws}/assistant/sessions")),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200);
    assert_eq!(v.as_array().unwrap().len(), 0, "失败请求不应留下空会话");

    // mock 智能体
    let mock_bin = env!("CARGO_BIN_EXE_fux-mock-agent");
    let (_, cli) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri("/api/v1/cli-entries")
                .set_json(json!({ "workspaceId": ws, "name": "MockAssistant", "command": mock_bin, "args": [] })),
            &owner,
        ),
    )
    .await;
    let (s, _) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/workspaces/{ws}/agents"))
                .set_json(
                    json!({ "name": "助理", "cliEntryId": cli["id"], "policy": "full_auto" }),
                ),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200);

    // 流式对话：首个事件返回 session
    let (s, body) = call_text(
        &app,
        auth(
            test::TestRequest::post()
                .uri("/api/v1/assistant/chat")
                .set_json(json!({ "workspaceId": ws, "message": "总结当前看板" })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200, "chat: {body}");
    assert!(body.contains("\"type\":\"chunk\""), "应有流式分片: {body}");
    let first = body
        .lines()
        .find(|l| l.starts_with("data: "))
        .expect("缺少 SSE 事件");
    let ev: Value = serde_json::from_str(first.trim_start_matches("data: ")).unwrap();
    let sid = ev["sessionId"]
        .as_str()
        .expect("缺少 sessionId")
        .to_string();

    // 同一会话第二条消息应复用常驻 ACP 连接，而不是重新冷启动
    let (s, body2) = call_text(
        &app,
        auth(
            test::TestRequest::post()
                .uri("/api/v1/assistant/chat")
                .set_json(json!({ "workspaceId": ws, "message": "第二个问题", "sessionId": sid })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200, "second chat: {body2}");
    assert!(
        body2.contains("\"type\":\"chunk\""),
        "复用会话应有流式分片: {body2}"
    );
    assert!(body2.contains("复用已有会话"), "应复用连接: {body2}");
    assert!(
        !body2.contains("正在创建会话"),
        "不应重新创建 ACP 会话: {body2}"
    );
    assert!(
        body2.contains("\"type\":\"thought\""),
        "应转发思考过程: {body2}"
    );
    assert!(
        body2.contains("\"type\":\"tool\""),
        "应转发工具调用: {body2}"
    );

    // 会话列表与消息
    let (s, v) = call(
        &app,
        auth(
            test::TestRequest::get().uri(&format!("/api/v1/workspaces/{ws}/assistant/sessions")),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200);
    let list = v.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"].as_str(), Some(sid.as_str()));
    assert!(list[0]["messageCount"].as_i64().unwrap_or(0) >= 2);

    let (s, v) = call(
        &app,
        auth(
            test::TestRequest::get().uri(&format!("/api/v1/assistant/sessions/{sid}/messages")),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200);
    let msgs = v.as_array().unwrap();
    assert_eq!(msgs[0]["role"], "user");
    assert!(msgs.iter().any(|m| m["role"] == "assistant"));
    let process = msgs
        .iter()
        .find(|m| m["role"] == "process")
        .expect("应有执行过程");
    let p: Value = serde_json::from_str(process["content"].as_str().unwrap()).unwrap();
    assert!(p["thoughts"].as_str().unwrap().contains("分析任务"));
    assert!(
        p["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["title"].as_str().unwrap().contains("编辑"))
    );

    // 生成结束后状态应为 false（页面刷新/离开后据此停止轮询）
    let (s, v) = call(
        &app,
        auth(
            test::TestRequest::get().uri(&format!("/api/v1/assistant/sessions/{sid}/status")),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200);
    assert_eq!(v["generating"], false);

    // 重命名与删除
    let (s, _) = call(
        &app,
        auth(
            test::TestRequest::patch()
                .uri(&format!("/api/v1/assistant/sessions/{sid}"))
                .set_json(json!({ "title": "看板总结" })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200);
    let (s, _) = call(
        &app,
        auth(
            test::TestRequest::delete().uri(&format!("/api/v1/assistant/sessions/{sid}")),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200);
    let (_, v) = call(
        &app,
        auth(
            test::TestRequest::get().uri(&format!("/api/v1/workspaces/{ws}/assistant/sessions")),
            &owner,
        ),
    )
    .await;
    assert_eq!(v.as_array().unwrap().len(), 0);
}

#[actix_web::test]
async fn cli_models_probe_reads_acp_config_options() {
    let ctx = setup().await;
    let app = app!(ctx);
    let owner = register(&app, "models1").await;
    let (ws, _) = create_ws(&app, &owner).await;
    let mock_bin = env!("CARGO_BIN_EXE_fux-mock-agent");
    let (_, cli) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri("/api/v1/cli-entries")
                .set_json(json!({ "workspaceId": ws, "name": "MockModels", "command": mock_bin, "args": [] })),
            &owner,
        ),
    )
    .await;
    let cli_id = cli["id"].as_str().unwrap().to_string();

    let (s, v) = call(
        &app,
        auth(
            test::TestRequest::post().uri(&format!("/api/v1/cli-entries/{cli_id}/models")),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200, "models: {v}");
    assert_eq!(v["available"], true, "mock CLI 应可探测: {v}");
    let models = v["catalog"]["models"].as_array().unwrap();
    assert_eq!(models.len(), 2, "应读取到 2 个模型: {v}");
    assert_eq!(models[0]["id"], "mock-fast");
    assert_eq!(v["catalog"]["currentModelId"], "mock-fast");
    assert_eq!(v["catalog"]["efforts"].as_array().unwrap().len(), 3);
}

/// 会话恢复：CLI 声明 loadSession 时，带 resume_session_id 的 run 必须走 session/load
/// （返回请求的会话 id），而不是 session/new（mock 会返回 cwd）。
#[actix_web::test]
async fn runner_resumes_acp_session_when_supported() {
    use async_trait::async_trait;
    use fux::runner::{RunSink, RunSpec, execute};
    use std::sync::Mutex;

    struct TestSink {
        started: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl RunSink for TestSink {
        async fn emit(&self, _kind: &str, _payload: Value) {}
        async fn permission(&self, _request: Value) -> String {
            "reject".into()
        }
        async fn heartbeat(&self, _turn_count: u32) {}
        async fn pending_guidance(&self) -> Vec<String> {
            vec![]
        }
        async fn session_started(&self, session_id: &str) {
            self.started.lock().unwrap().push(session_id.to_string());
        }
    }

    fn spec(workdir: &std::path::Path, command: &str, resume: Option<&str>) -> RunSpec {
        RunSpec {
            run_id: "run-resume".into(),
            task_id: "task-resume".into(),
            title: "恢复测试".into(),
            content: "请完成工作".into(),
            workdir: workdir.to_path_buf(),
            command: command.to_string(),
            args: vec![],
            env: vec![],
            cli_kind: "mock".into(),
            model: String::new(),
            effort: String::new(),
            system_prompt: String::new(),
            skills: vec![],
            mcp_servers: vec![],
            policy: "full_auto".into(),
            guidance: vec![],
            context_summary: String::new(),
            resume_session_id: resume.map(str::to_string),
            project: None,
            branch: "fux/test".into(),
            base_branch: None,
            stage_key: String::new(),
            stage_name: String::new(),
            upstream: vec![],
        }
    }

    let mock_bin = env!("CARGO_BIN_EXE_fux-mock-agent");
    let dir = tempfile::tempdir().unwrap();
    let workdir = dir.path().to_path_buf();

    // 1) 带上上个 run 的会话 id → 走 session/load，会话 id 原样续接
    let sink = std::sync::Arc::new(TestSink {
        started: Mutex::new(vec![]),
    });
    let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let outcome = execute(
        spec(&workdir, mock_bin, Some("prev-session-123")),
        sink.clone(),
        rx,
    )
    .await;
    assert_eq!(
        outcome.status, "finished",
        "恢复会话后应正常完成: {:?}",
        outcome.error
    );
    assert_eq!(
        outcome.session_id, "prev-session-123",
        "应通过 session/load 续接原会话"
    );
    assert_eq!(
        sink.started.lock().unwrap().as_slice(),
        ["prev-session-123"],
        "session_started 应上报真实会话 id"
    );

    // 2) 不带会话 id → 走 session/new（mock 以 cwd 作为会话 id）
    let sink2 = std::sync::Arc::new(TestSink {
        started: Mutex::new(vec![]),
    });
    let (_tx2, rx2) = tokio::sync::mpsc::unbounded_channel();
    let outcome2 = execute(spec(&workdir, mock_bin, None), sink2, rx2).await;
    assert_eq!(outcome2.status, "finished");
    let expect_new = std::path::absolute(&workdir)
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert_eq!(outcome2.session_id, expect_new, "无 resume 时应新建会话");
}

/// 存量 CLI env 明文应在启动回填时改存密文（兼容读取不破）。
#[actix_web::test]
async fn legacy_cli_env_is_encrypted_on_startup() {
    let dir = tempfile::tempdir().unwrap();
    {
        let pool = db::init(dir.path()).await.unwrap();
        sqlx::query("UPDATE cli_entries SET env_template = ? WHERE id = 'cli-mock'")
            .bind("{\"MOCK_TOKEN\":\"plain-123\"}")
            .execute(&pool)
            .await
            .unwrap();
    }
    // 重新初始化同一数据目录：存量明文应被加密
    let pool = db::init(dir.path()).await.unwrap();
    let stored: String =
        sqlx::query_scalar("SELECT env_template FROM cli_entries WHERE id = 'cli-mock'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_ne!(stored, "{\"MOCK_TOKEN\":\"plain-123\"}", "明文应被回填加密");
    assert!(!stored.contains("plain-123"), "密文不应含明文片段");
    assert_eq!(
        fux::server::secrets::cli_env_plain(&stored),
        json!({ "MOCK_TOKEN": "plain-123" }),
        "解密后应还原原值"
    );
}

/// CLI env 模板：写入即加密，接口不回传明文。
#[actix_web::test]
async fn cli_entry_env_is_encrypted_at_rest() {
    let ctx = setup().await;
    let app = app!(ctx);
    let owner = register(&app, "owner-env").await;
    let (ws, _board) = create_ws(&app, &owner).await;

    let (_, cli) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri("/api/v1/cli-entries")
                .set_json(json!({
                    "workspaceId": ws, "name": "带密钥", "command": "echo",
                    "envTemplate": { "API_KEY": "sk-secret" }
                })),
            &owner,
        ),
    )
    .await;
    assert_eq!(
        cli["envTemplate"], "<encrypted>",
        "创建响应不应回传密钥明文"
    );
    let cli_id = cli["id"].as_str().unwrap().to_string();

    let stored: String = sqlx::query_scalar("SELECT env_template FROM cli_entries WHERE id = ?")
        .bind(&cli_id)
        .fetch_one(&ctx.state.db)
        .await
        .unwrap();
    assert!(!stored.contains("sk-secret"), "入库必须是密文");
    assert_eq!(
        fux::server::secrets::cli_env_plain(&stored),
        json!({ "API_KEY": "sk-secret" })
    );

    // 列表同样不回传明文
    let (_, list) = call(
        &app,
        auth(
            test::TestRequest::get().uri(&format!("/api/v1/cli-entries?workspaceId={ws}")),
            &owner,
        ),
    )
    .await;
    let row = list
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == cli_id)
        .expect("列表应包含新建条目");
    assert_eq!(row["envTemplate"], "<encrypted>");
}

/// 助理会话权限：非 full_auto 策略下，写操作权限请求必须被拒绝并提示，而不是静默放行。
#[actix_web::test]
async fn assistant_respects_permission_policy() {
    let ctx = setup().await;
    let app = app!(ctx);
    let owner = register(&app, "assistant-perm").await;
    let (ws, _board) = create_ws(&app, &owner).await;

    let mock_bin = env!("CARGO_BIN_EXE_fux-mock-agent");
    let (_, cli) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri("/api/v1/cli-entries")
                .set_json(json!({ "workspaceId": ws, "name": "MockAsk", "command": mock_bin, "args": [] })),
            &owner,
        ),
    )
    .await;
    // 只读自动放行策略（写操作需人工审批）
    let (s, _) = call(
        &app,
        auth(
            test::TestRequest::post()
                .uri(&format!("/api/v1/workspaces/{ws}/agents"))
                .set_json(
                    json!({ "name": "谨慎助理", "cliEntryId": cli["id"], "policy": "read_auto" }),
                ),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200);

    // ASK 触发 mock 的 session/request_permission（kind=edit）
    let (s, body) = call_text(
        &app,
        auth(
            test::TestRequest::post()
                .uri("/api/v1/assistant/chat")
                .set_json(json!({ "workspaceId": ws, "message": "ASK 帮我改文件" })),
            &owner,
        ),
    )
    .await;
    assert_eq!(s, 200, "chat: {body}");
    assert!(
        body.contains("已阻止「写入文件 demo.txt」"),
        "写操作权限应按策略拒绝并提示: {body}"
    );
    assert!(body.contains("权限已处理"), "mock 应收到拒绝决策: {body}");
    assert!(body.contains("\"type\":\"done\""), "流应正常收尾: {body}");
}
