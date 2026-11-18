//! 集成测试：API 全流程 / 状态机闭环 / mock agent 端到端 / 质量阀。
use actix_web::{test, web, App};
use fux::server::{api, db, scheduler, watchdog, AppState, NodeRegistry};
use serde_json::{json, Value};
use tokio::sync::{broadcast, Mutex};

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
    S: actix_web::dev::Service<actix_http::Request, Response = actix_web::dev::ServiceResponse<B>, Error = actix_web::Error>,
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
    S: actix_web::dev::Service<actix_http::Request, Response = actix_web::dev::ServiceResponse<B>, Error = actix_web::Error>,
    B: actix_web::body::MessageBody,
{
    let (s, v) = call(app, test::TestRequest::post().uri("/api/v1/auth/register")
        .set_json(json!({ "username": name, "password": "password123" }))).await;
    assert_eq!(s, 200, "register: {v}");
    v["token"].as_str().unwrap().to_string()
}

fn auth(req: actix_web::test::TestRequest, token: &str) -> actix_web::test::TestRequest {
    req.insert_header(("Authorization", format!("Bearer {token}")))
}

async fn create_ws<S, B>(app: &S, token: &str) -> (String, String)
where
    S: actix_web::dev::Service<actix_http::Request, Response = actix_web::dev::ServiceResponse<B>, Error = actix_web::Error>,
    B: actix_web::body::MessageBody,
{
    let (s, v) = call(app, auth(test::TestRequest::post().uri("/api/v1/workspaces").set_json(json!({ "name": "测试工作区" })), token)).await;
    assert_eq!(s, 200, "create ws: {v}");
    let ws = v["id"].as_str().unwrap().to_string();
    let (s, v) = call(app, auth(test::TestRequest::get().uri(&format!("/api/v1/workspaces/{ws}/board")), token)).await;
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
    let (s, task) = call(&app, auth(test::TestRequest::post().uri("/api/v1/tasks")
        .set_json(json!({ "boardId": board, "title": "任务A", "content": "内容" })), &owner)).await;
    assert_eq!(s, 200);
    let tid = task["id"].as_str().unwrap().to_string();
    assert_eq!(task["status"], "planning");

    // 非法转移：planning → done
    let (s, _) = call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/tasks/{tid}/transition"))
        .set_json(json!({ "to": "done" })), &owner)).await;
    assert_eq!(s, 409);

    // planning → ready
    let (s, v) = call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/tasks/{tid}/transition"))
        .set_json(json!({ "to": "ready" })), &owner)).await;
    assert_eq!(s, 200);
    assert_eq!(v["status"], "ready");

    // 用户不能手动认领（ready → in_progress 仅调度器）
    let (s, _) = call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/tasks/{tid}/transition"))
        .set_json(json!({ "to": "in_progress" })), &owner)).await;
    assert_eq!(s, 403);

    // 邀请 member
    let member = register(&app, "member1").await;
    let (s, inv) = call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/workspaces/{ws}/invitations"))
        .set_json(json!({ "role": "member" })), &owner)).await;
    assert_eq!(s, 200);
    let code = inv["code"].as_str().unwrap();
    let (s, _) = call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/invitations/{code}/accept")), &member)).await;
    assert_eq!(s, 200);
    // 一次性码失效
    let (s, _) = call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/invitations/{code}/accept")), &member)).await;
    assert_eq!(s, 409);

    // member 可建卡、不可删工作区
    let (s, _) = call(&app, auth(test::TestRequest::post().uri("/api/v1/tasks")
        .set_json(json!({ "boardId": board, "title": "member 建的卡" })), &member)).await;
    assert_eq!(s, 200);
    let (s, _) = call(&app, auth(test::TestRequest::delete().uri(&format!("/api/v1/workspaces/{ws}")), &member)).await;
    assert_eq!(s, 403);

    // 事件时间线：task.created 在 task.moved 之前（钉住顺序）
    let events = fux::server::events::list_after(&ctx.state.db, &tid, 0).await.unwrap();
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
    let (_, task) = call(&app, auth(test::TestRequest::post().uri("/api/v1/tasks")
        .set_json(json!({ "boardId": board, "title": "闭环任务" })), &owner)).await;
    let tid = task["id"].as_str().unwrap().to_string();

    // 模拟智能体/系统推进：ready → in_progress → review
    let actor = "system";
    fux::server::domain::move_task(&ctx.state, &tid, "ready", actor, Default::default()).await.unwrap();
    fux::server::domain::move_task(&ctx.state, &tid, "in_progress", actor, Default::default()).await.unwrap();
    fux::server::domain::move_task(&ctx.state, &tid, "review", actor, Default::default()).await.unwrap();

    // 未合并且未人工确认 → 不可批准
    let (s, _) = call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/tasks/{tid}/transition"))
        .set_json(json!({ "to": "done" })), &owner)).await;
    assert_eq!(s, 409);

    // admin 人工确认合并 → 批准
    let (s, v) = call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/tasks/{tid}/transition"))
        .set_json(json!({ "to": "done", "manualConfirm": true })), &owner)).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["status"], "done");

    // 已完成任务评论 → 返工回待办（T7）
    let (s, v) = call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/tasks/{tid}/comments"))
        .set_json(json!({ "text": "还有问题，请返工" })), &owner)).await;
    assert_eq!(s, 200);
    assert_eq!(v["status"], "ready");

    // 再走到 done → 归档
    fux::server::domain::move_task(&ctx.state, &tid, "in_progress", actor, Default::default()).await.unwrap();
    fux::server::domain::move_task(&ctx.state, &tid, "review", actor, Default::default()).await.unwrap();
    sqlx::query("UPDATE tasks SET pr_state = 'merged' WHERE id = ?").bind(&tid).execute(&ctx.state.db).await.unwrap();
    let (s, v) = call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/tasks/{tid}/transition"))
        .set_json(json!({ "to": "done" })), &owner)).await;
    assert_eq!(s, 200, "检测到合并后可直接批准: {v}");
    let (s, v) = call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/tasks/{tid}/archive")), &owner)).await;
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
        let out = std::process::Command::new("git").args(&args).current_dir(&repo).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }
    std::fs::write(repo.join("a.txt"), "hello").unwrap();
    for args in [vec!["add", "-A"], vec!["commit", "-m", "init"]] {
        let out = std::process::Command::new("git").args(&args).current_dir(&repo).output().unwrap();
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
    let (_, agent) = call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/workspaces/{ws}/agents"))
        .set_json(json!({ "name": "测试智能体", "cliEntryId": cli_id, "policy": "full_auto" })), &owner)).await;
    let aid = agent["id"].as_str().unwrap().to_string();

    let (_, task) = call(&app, auth(test::TestRequest::post().uri("/api/v1/tasks")
        .set_json(json!({ "boardId": board, "title": "端到端任务", "content": "请写一个文件", "projectId": pid, "assigneeAgentId": aid })), &owner)).await;
    let tid = task["id"].as_str().unwrap().to_string();
    call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/tasks/{tid}/transition"))
        .set_json(json!({ "to": "ready" })), &owner)).await;

    scheduler::spawn(ctx.state.clone());

    // 等待自动认领 → 执行 → review（无 remote → 本地合并模式）
    let mut status = String::new();
    for _ in 0..80 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let t = fux::server::domain::load_task(&ctx.state, &tid).await.unwrap();
        status = t.status.clone();
        if status == "review" || status == "blocked" { break; }
    }
    assert_eq!(status, "review", "mock agent 应完成并进入审核");
    let t = fux::server::domain::load_task(&ctx.state, &tid).await.unwrap();
    assert_eq!(t.pr_state.as_deref(), Some("local"), "无 remote → 本地合并模式");
    assert!(t.branch.as_deref().unwrap_or("").starts_with("fux/task-"));

    // 时间线关键事件顺序：run.started < agent.message < pr.linked < task.moved(review)
    let events = fux::server::events::list_after(&ctx.state.db, &tid, 0).await.unwrap();
    let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
    let pos = |k: &str| kinds.iter().position(|x| *x == k).unwrap_or_else(|| panic!("缺少事件 {k}, 实际: {kinds:?}"));
    assert!(pos("run.started") < pos("agent.message"));
    assert!(pos("agent.message") < pos("tool.call"));
    assert!(pos("pr.linked") < kinds.len());
    assert!(kinds.contains(&"task.moved"));

    // worktree 中确有 mock 产出且已提交
    let wt = fux::git::worktree_dir(&ctx.state.data_dir, &tid);
    assert!(wt.join("demo.txt").exists(), "mock 应写入 demo.txt");
    let log = std::process::Command::new("git").args(["log", "--oneline"]).current_dir(&wt).output().unwrap();
    assert!(String::from_utf8_lossy(&log.stdout).contains("fux(task)"), "应有交付提交");

    // 人工确认批准 → done；评论返工 → ready
    let (s, _) = call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/tasks/{tid}/transition"))
        .set_json(json!({ "to": "done", "manualConfirm": true })), &owner)).await;
    assert_eq!(s, 200);
    let (s, v) = call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/tasks/{tid}/comments"))
        .set_json(json!({ "text": "返工" })), &owner)).await;
    assert_eq!(s, 200);
    assert_eq!(v["status"], "ready");
}

/// 突然中断：自动重试 ≤2 次后阻塞。
#[actix_web::test]
async fn e2e_interrupt_retry_then_block() {
    let ctx = setup().await;
    let app = app!(ctx);
    let owner = register(&app, "owner4").await;
    let (ws, board) = create_ws(&app, &owner).await;
    let mock_bin = env!("CARGO_BIN_EXE_fux-mock-agent");
    let (_, cli) = call(&app, auth(test::TestRequest::post().uri("/api/v1/cli-entries")
        .set_json(json!({ "workspaceId": ws, "name": "MockFail", "command": mock_bin })), &owner)).await;
    let (_, agent) = call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/workspaces/{ws}/agents"))
        .set_json(json!({ "name": "失败智能体", "cliEntryId": cli["id"].as_str().unwrap() })), &owner)).await;
    let aid = agent["id"].as_str().unwrap().to_string();
    let (_, task) = call(&app, auth(test::TestRequest::post().uri("/api/v1/tasks")
        .set_json(json!({ "boardId": board, "title": "中断任务", "content": "FAIL 模拟崩溃", "assigneeAgentId": aid })), &owner)).await;
    let tid = task["id"].as_str().unwrap().to_string();
    call(&app, auth(test::TestRequest::post().uri(&format!("/api/v1/tasks/{tid}/transition"))
        .set_json(json!({ "to": "ready" })), &owner)).await;
    scheduler::spawn(ctx.state.clone());

    let mut status = String::new();
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let t = fux::server::domain::load_task(&ctx.state, &tid).await.unwrap();
        status = t.status.clone();
        if status == "blocked" { break; }
    }
    assert_eq!(status, "blocked", "中断重试耗尽后应阻塞");
    let runs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE task_id = ?").bind(&tid).fetch_one(&ctx.state.db).await.unwrap();
    assert_eq!(runs, 3, "1 次原始 + 2 次自动重试");
    let t = fux::server::domain::load_task(&ctx.state, &tid).await.unwrap();
    assert!(t.block_reason.unwrap_or_default().contains("运行失败"));
}

/// 质量阀：连续相同工具调用 → 熔断阻塞（不自动重试）。
#[actix_web::test]
async fn watchdog_loop_trip() {
    let ctx = setup().await;
    let app = app!(ctx);
    let owner = register(&app, "owner5").await;
    let (ws, board) = create_ws(&app, &owner).await;
    let (_, task) = call(&app, auth(test::TestRequest::post().uri("/api/v1/tasks")
        .set_json(json!({ "boardId": board, "title": "循环任务" })), &owner)).await;
    let tid = task["id"].as_str().unwrap().to_string();

    // 伪造运行中状态 + 5 次相同工具调用
    let run_id = fux::new_id();
    sqlx::query("UPDATE tasks SET status = 'in_progress', lease_run_id = ?, lease_expires_at = ? WHERE id = ?")
        .bind(&run_id).bind(fux::server::gitflow::lease_deadline()).bind(&tid).execute(&ctx.state.db).await.unwrap();
    let agent_id = fux::new_id();
    sqlx::query("INSERT INTO agents (id, workspace_id, name, cli_entry_id) VALUES (?,?,?, 'cli-mock')")
        .bind(&agent_id).bind(&ws).bind("循环智能体").execute(&ctx.state.db).await.unwrap();
    sqlx::query("INSERT INTO runs (id, task_id, agent_id, node_id, status, last_event_at) VALUES (?,?,?,?, 'running', ?)")
        .bind(&run_id).bind(&tid).bind(&agent_id).bind("local").bind(fux::now()).execute(&ctx.state.db).await.unwrap();
    for i in 0..5 {
        fux::server::events::append(&ctx.state.db, &ctx.state.broadcast, &tid, "tool.call", "agent:x", Some(&run_id),
            json!({ "toolCallId": format!("c{i}"), "title": "读同一文件", "kind": "read", "rawInput": { "path": "same.txt" } })).await.unwrap();
    }

    watchdog::scan_once(&ctx.state).await.unwrap();
    let t = fux::server::domain::load_task(&ctx.state, &tid).await.unwrap();
    assert_eq!(t.status, "blocked");
    assert!(t.block_reason.unwrap_or_default().contains("质量阀熔断 [loop]"));
    let kinds: Vec<String> = fux::server::events::list_after(&ctx.state.db, &tid, 0).await.unwrap().into_iter().map(|e| e.kind).collect();
    assert!(kinds.contains(&"watchdog.loop".to_string()));
}