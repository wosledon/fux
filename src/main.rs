//! Fux 控制面入口。
#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    fux::server::run().await
}