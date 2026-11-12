//! Run the reader inside Codex without opening a separate desktop window.
//! cargo run --example workspace_server -j 4 -- <data-directory> [port]
use reader_rust::{
    app::{bootstrap, config::AppConfig},
    server::{assets::StaticSource, WebOptions, WebServer},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let storage = std::path::PathBuf::from(args.next().unwrap_or_else(|| "workspace-data".into()));
    std::fs::create_dir_all(&storage)?;
    let storage = storage.canonicalize()?;
    let port = args.next().map(|p| p.parse()).transpose()?.unwrap_or(1122);
    let cfg = AppConfig {
        storage_dir: storage.to_string_lossy().into_owned(),
        assets_dir: storage.join("assets").to_string_lossy().into_owned(),
        database_url: format!("sqlite:{}?mode=rwc", storage.join("reader.db").display()),
        ..AppConfig::default()
    };
    bootstrap::init_tracing("warn");
    let state = bootstrap::build_state(&cfg).await?;
    let static_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("frontend/dist");
    anyhow::ensure!(
        static_dir.join("index.html").exists(),
        "先执行 cd frontend && npm ci && npm run build"
    );
    let server = WebServer::start(
        state,
        WebOptions { port, lan: false },
        Some(StaticSource::Directory(static_dir)),
    )
    .await?;
    println!("工作区：http://{}/#/workspace", server.addr);
    println!("数据目录：{}", storage.display());
    tokio::signal::ctrl_c().await?;
    server.stop();
    Ok(())
}
