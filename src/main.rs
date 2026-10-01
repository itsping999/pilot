mod agent;
mod api;
mod app;
mod bot;
mod commands;
mod config;
mod contracts;
mod db;
mod embedding;
mod generation;
mod http;
mod ingestion;
mod middleware;
mod models;
mod rag;
mod retrieval;
mod skills;
mod store;
mod ui;

use std::net::SocketAddr;

use api::build_router;
use app::{build_bot_service, build_rag_service};
use config::Config;
use log::{info, warn};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();
    if let Some(result) = commands::run(&args[1..]) {
        return result;
    }

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run_server())
}

async fn run_server() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env();
    let rag = build_rag_service(&config)?;
    let bot = build_bot_service(&config, rag.clone())?;

    let app = build_router(
        rag,
        bot,
        config.ui_enabled,
        config.request_body_limit_bytes,
        config.api_token.clone(),
        config.rate_limit_rpm,
    );
    let listener = tokio::net::TcpListener::bind(config.addr).await?;
    let addr: SocketAddr = listener.local_addr()?;
    info!("knowledge-pilot listening on {}", addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    match tokio::signal::ctrl_c().await {
        Ok(()) => info!("shutdown signal received"),
        Err(error) => {
            warn!("shutdown signal listener unavailable: {error}");
            std::future::pending::<()>().await;
        }
    }
}
