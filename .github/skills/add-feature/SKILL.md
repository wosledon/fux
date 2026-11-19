---
name: add-feature
description: '新增端到端功能：从 Rust /api/v1 端点、状态机与事件，到前端 api.ts 类型、页面与路由、TanStack Query 缓存，再到 tests/api.rs 集成测试的完整脚手架流程。当要加一个新资源、新接口、新页面或打通前后端字段时使用。'
argument-hint: '描述要新增的功能，例如「给项目加归档开关」'
---

# 新增端到端功能

按顺序执行，每完成一段就做一次编译/类型检查，避免最后一次性排错。

先确认改动面：

- 只是已有结构加字段 → 迁移（可选）+ `models.rs` + `api.ts` 类型 + 页面渲染。
- 新资源/新接口 → 下面全流程。
- 只是内部机制（调度/质量阀/ACP）→ 后端段 + `docs/TESTING.md` 的测试要求，跳过前端。

## 1. 数据库（需要新表/新列时）

按 [migrations.instructions.md](../../instructions/migrations.instructions.md) 新建 `migrations/000N_*.sql`，然后同步 `src/server/models.rs` 的 `FromRow` 行结构与 `frontend/src/api.ts` 的接口定义。

## 2. 后端端点

1. 新建/打开 `src/server/api/<resource>.rs`，文件头写 `//! 一句话职责`。
2. handler 形态：

```rust
pub async fn list(
    state: web::Data<AppState>,
    user: AuthUser,
    path: web::Path<String>,
) -> ApiResult<impl Responder> {
    let ws_id = path.into_inner();
    auth::require(&state.db, &ws_id, &user.id, "member").await?;   // 先鉴权
    let rows = sqlx::query_as::<_, Foo>("SELECT * FROM foos WHERE workspace_id = ?") // 再按工作区过滤
        .bind(&ws_id)
        .fetch_all(&state.db)
        .await?;
    Ok(web::Json(rows))
}
```

3. 请求体结构体同时放同文件：`#[derive(Deserialize)] #[serde(rename_all = "camelCase")] pub struct FooReq { ... }`。
4. 需要状态变更走 `domain::move_task`；需要留痕走 `events::append`（`kind` 用点号命名）。
5. 在 `src/server/api/mod.rs` 加 `pub mod <resource>;` 和 `cfg.configure(<resource>::routes);`。
6. 敏感操作（删除、归档、配置）用 `require(..., "admin")`。

## 3. 前端

1. `frontend/src/api.ts`：加 `interface Foo`（camelCase，JSON 列标注为 `string`），按需加 `getFoo/postFoo` 便捷函数。
2. 新建或修改 `frontend/src/pages/XxxPage.tsx`：

```tsx
export default function XxxPage() {
  const { wsId, role } = useOutletContext<WsCtx>()      // 不要用 useParams 读 wsId
  const qc = useQueryClient()
  const isAdmin = role === 'owner' || role === 'admin'
  const { data: rows = [] } = useQuery({
    queryKey: ['foos', wsId],
    queryFn: () => get<Foo[]>(`/workspaces/${wsId}/foos`),
  })
  const create = async () => {
    try {
      await post(`/workspaces/${wsId}/foos`, { name })
      qc.invalidateQueries({ queryKey: ['foos', wsId] })   // 不做乐观更新
      setOpen(false)
    } catch (e: any) { setErr(e.message) }
  }
  // ...
}
```

3. 新页面在 `src/App.tsx` 的 `/ws/:wsId` 子路由下注册；需要出现在侧栏时同步 `pages/WorkspaceLayout.tsx` 的 `NAV_GROUPS`。
4. 复用 `src/ui.tsx`（`PageHeader` / `EmptyState` / `DeleteButton` / `useToast`），颜色取 `theme.ts` 的 `STATUS_ACCENT` / `LANE_ACCENT`。
5. 类型导入必须 `import type`（`verbatimModuleSyntax`）；后端 JSON 字符串列用 `parseJson`。

## 4. 测试

在 `tests/api.rs` 加一个 `#[actix_web::test]`，复用 `setup()` / `app!` / `call()` / `register()` / `auth()` / `create_ws()`：

```rust
#[actix_web::test]
async fn foos_crud_and_rbac() {
    let Ctx { state, _dir } = setup().await;
    let app = app!(state);
    let token = register(app, "u1").await;
    let (ws, _board) = create_ws(app, &token).await;
    let (s, v) = call(app, test::TestRequest::post().uri(&format!("/api/v1/workspaces/{ws}/foos"))
        .set_json(serde_json::json!({"name":"x"}))
        .insert_header(("Authorization", auth(&token))).to_request()).await;
    assert_eq!(s, 200, "{v}");
}
```

同时覆盖：未登录 401、非成员 403、`expectedVersion` 过期 409（如涉及状态流转）。

## 5. 验证

```powershell
cargo build
cargo test
cd frontend; npm run build
```

前端页面改完还要 `npm run dev` 起 5173，按 `README.md` 的 5 分钟演示路径手测一遍：建卡 → 认领 → 时间线 → 审核 → 归档。

## 提交前自检

- [ ] 每个查询都先 `auth::require` 再带 `workspace_id` 条件
- [ ] ID/时间戳用 `crate::new_id()` / `crate::now()`
- [ ] 状态只经 `domain::move_task` 改，且落了事件
- [ ] 响应 camelCase，前端类型已同步
- [ ] 文案、注释、错误消息为中文
- [ ] `cargo test` 与 `npm run build` 通过