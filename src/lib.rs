//! Fux — AI 工作台。
//! 共享模块：ACP 客户端 / git 工具 / 节点协议 / 通用运行器；server 为控制面实现。
pub mod acp;
pub mod git;
pub mod proto;
pub mod runner;
pub mod server;

pub fn now() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
