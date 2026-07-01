mod assets;
mod config;
mod history;
mod http;
mod media;
mod pipe;
mod state;
mod time;

use anyhow::{Context, Result};
use std::{path::PathBuf, time::Duration};
use tokio::net::TcpListener;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::{config::AppConfig, state::AppState};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let args = Args::parse();
    let (config, config_path) = AppConfig::load_from_default_locations(args.config)?;
    let bind_addr = config.bind_addr();
    let state = AppState::load(config)?;

    info!(
        config = config_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<defaults>".to_owned()),
        data_dir = %state.dirs().root.display(),
        pipe = %state.pipe_name(),
        http = %format!("http://{bind_addr}"),
        "textractor media bridge starting"
    );
    if let Some(token) = state.session_token() {
        info!(%token, "LAN session token");
    }

    let pipe_state = state.clone();
    let pipe_task = tokio::spawn(async move {
        if let Err(error) = pipe::run_pipe_server(pipe_state).await {
            error!(%error, "pipe server stopped");
        }
    });

    let cleanup_state = state.clone();
    let cleanup_task = tokio::spawn(async move {
        cleanup_loop(cleanup_state).await;
    });

    if args.open {
        open_browser(&format!("http://{bind_addr}"));
    }

    let listener = TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("failed to bind HTTP server on {bind_addr}"))?;
    axum::serve(listener, http::router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    pipe_task.abort();
    cleanup_task.abort();
    Ok(())
}

#[derive(Debug, Default)]
struct Args {
    config: Option<PathBuf>,
    open: bool,
}

impl Args {
    fn parse() -> Self {
        let mut parsed = Args::default();
        let mut args = std::env::args_os().skip(1);
        while let Some(arg) = args.next() {
            match arg.to_string_lossy().as_ref() {
                "--config" | "-c" => {
                    parsed.config = args.next().map(PathBuf::from);
                }
                "--open" => parsed.open = true,
                _ => {}
            }
        }
        parsed
    }
}

async fn cleanup_loop(state: AppState) {
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
        match state.cleanup_assets_and_history().await {
            Ok(0) => {}
            Ok(count) => info!(count, "purged expired asset-backed line records"),
            Err(error) => error!(%error, "asset cleanup failed"),
        }
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("textractor_bridge_server=info,tower_http=info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

#[cfg(windows)]
fn open_browser(url: &str) {
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(not(windows))]
fn open_browser(_url: &str) {}
