use anyhow::Result;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::path::Path;
use std::time::Duration;

pub async fn init(data_dir: &Path) -> Result<SqlitePool> {
    tokio::fs::create_dir_all(data_dir).await?;
    let opts = SqliteConnectOptions::new()
        .filename(data_dir.join("fux.db"))
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(10))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(opts)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    seed(&pool).await?;
    encrypt_legacy_cli_env(&pool).await?;
    Ok(pool)
}

/// 存量 CLI env 模板加密回填：env 加密上线前为明文 JSON，启动时统一改存密文（幂等）。
/// 解密成功说明已是密文，跳过；非 JSON 的脏数据原样保留。
async fn encrypt_legacy_cli_env(pool: &SqlitePool) -> Result<()> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, env_template FROM cli_entries WHERE env_template != '' AND env_template != '{}'",
    )
    .fetch_all(pool)
    .await?;
    for (id, env) in rows {
        if crate::server::secrets::decrypt(&env).is_ok() {
            continue; // 已是密文
        }
        if serde_json::from_str::<serde_json::Value>(&env).is_err() {
            continue; // 非 JSON 明文，不动
        }
        let enc = crate::server::secrets::encrypt(&env)?;
        sqlx::query("UPDATE cli_entries SET env_template = ? WHERE id = ?")
            .bind(&enc)
            .bind(&id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// 默认 CLI 条目（PRD FR-09 首批四条 + 演示 mock）与本地节点。
async fn seed(pool: &SqlitePool) -> Result<()> {
    let defaults = [
        ("cli-codex", "Codex", "codex", "codex-acp", "[]"),
        (
            "cli-opencode",
            "OpenCode",
            "opencode",
            "opencode",
            "[\"acp\"]",
        ),
        ("cli-pi", "Pi", "pi", "pi-acp", "[]"),
        (
            "cli-copilot",
            "Copilot CLI",
            "copilot",
            "copilot",
            "[\"--acp\",\"--stdio\"]",
        ),
        (
            "cli-mock",
            "Mock（演示/测试）",
            "mock",
            "fux-mock-agent",
            "[]",
        ),
    ];
    for (id, name, kind, command, args) in defaults {
        sqlx::query(
            "INSERT OR IGNORE INTO cli_entries (id, workspace_id, name, kind, command, args, env_template, is_default) VALUES (?, NULL, ?, ?, ?, ?, '{}', 1)",
        )
        .bind(id)
        .bind(name)
        .bind(kind)
        .bind(command)
        .bind(args)
        .execute(pool)
        .await?;
    }
    let caps = serde_json::json!({
        "os": std::env::consts::OS,
        "clis": crate::proto::probe_clis(),
        "projectPaths": [],
        "load": 0,
    });
    sqlx::query(
        "INSERT INTO nodes (id, name, mode, token, approved, conn_status, capabilities) VALUES ('local', '本地节点（控制面）', 'local', '', 1, 'online', ?) ON CONFLICT(id) DO UPDATE SET capabilities=excluded.capabilities, conn_status='online'",
    )
    .bind(caps.to_string())
    .execute(pool)
    .await?;
    Ok(())
}
