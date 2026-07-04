#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod assets;
mod config;
mod history;
mod http;
mod media;
mod pipe;
mod state;
mod time;
#[cfg(windows)]
mod tray;

use anyhow::{Context, Result};
use std::{
    future::Future,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    path::PathBuf,
    time::Duration,
};
use tokio::net::TcpListener;
use tokio::sync::{oneshot, watch};
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::{config::AppConfig, state::AppState};

fn main() -> Result<()> {
    init_tracing();

    let args = Args::parse();
    let prepared = prepare_server(args.clone())?;
    if args.use_tray() {
        #[cfg(windows)]
        {
            match tray::run(prepared) {
                Ok(()) => return Ok(()),
                Err(error) => {
                    warn!(%error, "tray mode unavailable; running without tray");
                    return run_console(args);
                }
            }
        }
    }

    run_prepared_console(prepared)
}

fn prepare_server(args: Args) -> Result<PreparedServer> {
    let (config, config_path) = AppConfig::load_from_default_locations(args.config)?;
    let bind_addr = config.bind_addr();
    let state = AppState::load_with_config_path(config, config_path.clone())?;
    let local_url = local_browser_url(bind_addr);

    Ok(PreparedServer {
        state,
        bind_addr,
        local_url,
        config_path,
        open: args.open,
    })
}

fn run_console(args: Args) -> Result<()> {
    run_prepared_console(prepare_server(args)?)
}

fn run_prepared_console(prepared: PreparedServer) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to create server runtime")?;
    runtime.block_on(run_server(prepared, shutdown_signal()))
}

#[cfg(windows)]
pub(crate) fn run_server_thread(
    prepared: PreparedServer,
    shutdown_rx: oneshot::Receiver<()>,
) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to create server runtime")?;
    runtime.block_on(run_server(prepared, async move {
        let _ = shutdown_rx.await;
    }))
}

async fn run_server<S>(prepared: PreparedServer, shutdown: S) -> Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    let PreparedServer {
        state,
        bind_addr,
        local_url,
        config_path,
        open,
    } = prepared;

    info!(
        config = config_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<defaults>".to_owned()),
        data_dir = %state.dirs().root.display(),
        pipe = %state.pipe_name(),
        http = %format!("http://{bind_addr}"),
        local_url = %local_url,
        "textractor media bridge starting"
    );

    let listener = TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("failed to bind HTTP server on {bind_addr}"))?;

    if open {
        open_browser(&local_url);
    }

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let shutdown_task = tokio::spawn(async move {
        shutdown.await;
        let _ = shutdown_tx.send(true);
    });

    let pipe_state = state.clone();
    let pipe_shutdown = shutdown_rx.clone();
    let pipe_task = tokio::spawn(async move {
        tokio::select! {
            result = pipe::run_pipe_server(pipe_state) => {
                if let Err(error) = result {
                    error!(%error, "pipe server stopped");
                }
            }
            _ = wait_for_shutdown(pipe_shutdown) => {}
        }
    });

    let cleanup_state = state.clone();
    let cleanup_shutdown = shutdown_rx.clone();
    let cleanup_task = tokio::spawn(async move {
        cleanup_loop(cleanup_state, cleanup_shutdown).await;
    });

    axum::serve(listener, http::router(state))
        .with_graceful_shutdown(wait_for_shutdown(shutdown_rx.clone()))
        .await?;

    let _ = shutdown_task.await;
    let _ = pipe_task.await;
    let _ = cleanup_task.await;
    Ok(())
}

pub(crate) struct PreparedServer {
    state: AppState,
    pub(crate) bind_addr: SocketAddr,
    pub(crate) local_url: String,
    config_path: Option<PathBuf>,
    open: bool,
}

#[derive(Clone, Debug, Default)]
struct Args {
    config: Option<PathBuf>,
    open: bool,
    no_tray: bool,
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
                "--no-tray" => parsed.no_tray = true,
                _ => {}
            }
        }
        parsed
    }

    fn use_tray(&self) -> bool {
        cfg!(all(windows, not(debug_assertions))) && !self.no_tray
    }
}

async fn cleanup_loop(state: AppState, shutdown: watch::Receiver<bool>) {
    loop {
        tokio::select! {
            _ = wait_for_shutdown(shutdown.clone()) => break,
            _ = tokio::time::sleep(Duration::from_secs(60)) => {}
        }
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

async fn wait_for_shutdown(mut shutdown: watch::Receiver<bool>) {
    if *shutdown.borrow() {
        return;
    }
    let _ = shutdown.changed().await;
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("textractor_bridge_server=info,tower_http=info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

fn local_browser_url(bind_addr: SocketAddr) -> String {
    let host = match bind_addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => "127.0.0.1".to_owned(),
        IpAddr::V6(ip) if ip.is_unspecified() => "[::1]".to_owned(),
        IpAddr::V6(ip) => format!("[{ip}]"),
        IpAddr::V4(ip) => ip.to_string(),
    };
    format!("http://{host}:{}", bind_addr.port())
}

pub(crate) fn local_lan_url(bind_addr: SocketAddr) -> Option<String> {
    local_lan_url_with_detector(bind_addr, detect_lan_ipv4)
}

fn local_lan_url_with_detector<F>(bind_addr: SocketAddr, detect_ipv4: F) -> Option<String>
where
    F: FnOnce() -> Option<Ipv4Addr>,
{
    let ip = match bind_addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => detect_ipv4()?,
        IpAddr::V4(ip) if !ip.is_loopback() => ip,
        _ => return None,
    };
    Some(format!("http://{ip}:{}/", bind_addr.port()))
}

fn detect_lan_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(8, 8, 8, 8), 80)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_unspecified() && !ip.is_loopback() => Some(ip),
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn local_browser_url_uses_loopback_for_unspecified_bind() {
        let url = local_browser_url(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 7788));

        assert_eq!(url, "http://127.0.0.1:7788");
    }

    #[test]
    fn local_browser_url_formats_ipv6_hosts() {
        let url = local_browser_url(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 7788));

        assert_eq!(url, "http://[::1]:7788");
    }

    #[test]
    fn local_lan_url_uses_concrete_non_loopback_ipv4() {
        let url = local_lan_url_with_detector(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 23)), 7788),
            || None,
        );

        assert_eq!(url.as_deref(), Some("http://192.168.1.23:7788/"));
    }

    #[test]
    fn local_lan_url_rejects_loopback_ipv4() {
        let url = local_lan_url_with_detector(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 7788),
            || Some(Ipv4Addr::new(192, 168, 1, 23)),
        );

        assert_eq!(url, None);
    }

    #[test]
    fn local_lan_url_detects_ipv4_for_unspecified_bind() {
        let url = local_lan_url_with_detector(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 7788),
            || Some(Ipv4Addr::new(192, 168, 1, 23)),
        );

        assert_eq!(url.as_deref(), Some("http://192.168.1.23:7788/"));
    }
}
