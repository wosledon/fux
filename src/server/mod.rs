//! Fux 控制面：Actix-web + SQLx(SQLite)。
pub mod api;
pub mod auth;
pub mod db;
pub mod domain;
pub mod error;
pub mod events;
pub mod gitflow;
pub mod models;
pub mod nodes_ws;
pub mod scheduler;
pub mod secrets;
pub mod watchdog;
pub mod workflow;

use crate::proto::ServerMsg;
use events::EventEnvelope;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast, mpsc};

/// 已连接节点通道注册表。
#[derive(Default, Clone)]
pub struct NodeRegistry {
    inner: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<ServerMsg>>>>,
}

impl NodeRegistry {
    pub async fn register(&self, node_id: &str, tx: mpsc::UnboundedSender<ServerMsg>) {
        self.inner.lock().await.insert(node_id.to_string(), tx);
    }
    pub async fn unregister(&self, node_id: &str) {
        self.inner.lock().await.remove(node_id);
    }
    pub async fn send(&self, node_id: &str, msg: ServerMsg) -> bool {
        let map = self.inner.lock().await;
        match map.get(node_id) {
            Some(tx) => tx.send(msg).is_ok(),
            None => false,
        }
    }
}

pub struct AppState {
    pub db: SqlitePool,
    pub jwt_secret: String,
    pub data_dir: PathBuf,
    pub broadcast: broadcast::Sender<EventEnvelope>,
    pub nodes: NodeRegistry,
    /// 本地运行中的 Run → 取消信号
    pub local_runs: Mutex<HashMap<String, mpsc::UnboundedSender<()>>>,
    /// AI 助理常驻 ACP 连接池（按工作区会话复用）
    pub assistant_pool: api::assistant::AssistantPool,
    pub api_base: String,
}

pub type SharedState = actix_web::web::Data<AppState>;

pub async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let data_dir = PathBuf::from(std::env::var("FUX_DATA_DIR").unwrap_or_else(|_| "data".into()));
    let addr = std::env::var("FUX_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let jwt_secret =
        std::env::var("FUX_JWT_SECRET").unwrap_or_else(|_| "fux-dev-secret-change-me".into());
    if jwt_secret == "fux-dev-secret-change-me" {
        tracing::warn!("使用开发默认 JWT 密钥，生产部署请设置 FUX_JWT_SECRET");
    }
    let port = addr.rsplit(':').next().unwrap_or("8080").to_string();
    let db = db::init(&data_dir).await?;
    let (broadcast, _) = broadcast::channel(2048);
    let state = actix_web::web::Data::new(AppState {
        db,
        jwt_secret,
        data_dir,
        broadcast,
        nodes: NodeRegistry::default(),
        local_runs: Mutex::new(HashMap::new()),
        assistant_pool: Default::default(),
        api_base: format!("http://127.0.0.1:{port}"),
    });

    scheduler::spawn(state.clone());
    watchdog::spawn(state.clone());
    gitflow::spawn_merge_checker(state.clone());
    api::assistant::spawn_reaper(state.clone());

    tracing::info!("Fux 控制面启动: http://{addr}");
    actix_web::HttpServer::new(move || {
        actix_web::App::new()
            .app_data(state.clone())
            .app_data(actix_web::web::JsonConfig::default().limit(16 * 1024 * 1024))
            .wrap(actix_cors::Cors::permissive())
            .service(actix_web::web::scope("/api/v1").configure(api::routes))
    })
    .bind(&addr)?
    .run()
    .await?;
    Ok(())
}
