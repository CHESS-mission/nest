#![recursion_limit = "256"]

use clap::Parser;
use tracing_subscriber::EnvFilter;
use nest_server::{
    config::Config, server::NestServer
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info"))
        )
        .init();

    let config = Config::parse();
    let server = NestServer::from_config(config)?;
    server.serve().await?;

    Ok(())
}

