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
mod websocket;

use anyhow::{Context, Result};
use std::{
    collections::BTreeSet,
    future::Future,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener as StdTcpListener, UdpSocket},
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
    let websocket =
        prepare_websocket_listener(config.websocket.enabled, config.websocket_bind_addr());
    let websocket_bind_addr = websocket.as_ref().map(|prepared| prepared.bind_addr);
    let websocket_listener = websocket.map(|prepared| prepared.listener);
    let state = AppState::load_with_config_path(config, config_path.clone())?;
    let local_url = local_browser_url(bind_addr);
    let websocket_local_url = websocket_bind_addr.map(local_websocket_url);

    Ok(PreparedServer {
        state,
        bind_addr,
        local_url,
        websocket_bind_addr,
        websocket_local_url,
        websocket_listener,
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
        websocket_bind_addr,
        websocket_local_url,
        websocket_listener,
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
        websocket = websocket_bind_addr
            .map(|addr| format!("ws://{addr}/"))
            .unwrap_or_else(|| "disabled".to_owned()),
        websocket_local_url = websocket_local_url.as_deref().unwrap_or("disabled"),
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
        info!("server shutdown signal received");
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
            _ = wait_for_shutdown(pipe_shutdown) => {
                info!("pipe server shutdown requested");
            }
        }
        info!("pipe server task finished");
    });

    let cleanup_state = state.clone();
    let cleanup_shutdown = shutdown_rx.clone();
    let cleanup_task = tokio::spawn(async move {
        cleanup_loop(cleanup_state, cleanup_shutdown).await;
        info!("cleanup task finished");
    });

    let websocket_task = if let Some(websocket_listener) = websocket_listener {
        let websocket_state = state.clone();
        let websocket_shutdown = shutdown_rx.clone();
        Some(tokio::spawn(async move {
            match websocket::run(websocket_state, websocket_listener, websocket_shutdown).await {
                Ok(()) => info!("websocket server task finished"),
                Err(error) => warn!(%error, "websocket server unavailable"),
            }
        }))
    } else {
        info!("websocket server disabled");
        None
    };

    info!("http server listening");
    axum::serve(listener, http::router(state))
        .with_graceful_shutdown(wait_for_shutdown(shutdown_rx.clone()))
        .await?;
    info!("http server shutdown completed");

    let _ = shutdown_task.await;
    info!("shutdown signal task joined");
    let _ = pipe_task.await;
    info!("pipe server task joined");
    let _ = cleanup_task.await;
    info!("cleanup task joined");
    if let Some(websocket_task) = websocket_task {
        let _ = websocket_task.await;
        info!("websocket server task joined");
    }
    info!("textractor media bridge stopped");
    Ok(())
}

pub(crate) struct PreparedServer {
    state: AppState,
    pub(crate) bind_addr: SocketAddr,
    pub(crate) local_url: String,
    pub(crate) websocket_bind_addr: Option<SocketAddr>,
    pub(crate) websocket_local_url: Option<String>,
    websocket_listener: Option<StdTcpListener>,
    config_path: Option<PathBuf>,
    open: bool,
}

struct PreparedWebSocketListener {
    bind_addr: SocketAddr,
    listener: StdTcpListener,
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
            _ = wait_for_shutdown(shutdown.clone()) => {
                info!("cleanup shutdown requested");
                break;
            }
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

fn local_websocket_url(bind_addr: SocketAddr) -> String {
    let host = match bind_addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => "localhost".to_owned(),
        IpAddr::V6(ip) if ip.is_unspecified() => "localhost".to_owned(),
        IpAddr::V6(ip) => format!("[{ip}]"),
        IpAddr::V4(ip) => ip.to_string(),
    };
    format!("ws://{host}:{}/", bind_addr.port())
}

pub(crate) fn localhost_endpoint_label(bind_addr: SocketAddr) -> String {
    format!("localhost:{}", bind_addr.port())
}

fn prepare_websocket_listener(
    enabled: bool,
    bind_addr: SocketAddr,
) -> Option<PreparedWebSocketListener> {
    if !enabled {
        return None;
    }

    let mut first_error = None;
    for candidate in websocket_bind_candidates(bind_addr) {
        match bind_websocket_listener(candidate) {
            Ok(listener) => {
                if candidate != bind_addr {
                    info!(
                        websocket = %format!("ws://{candidate}/"),
                        requested = %format!("ws://{bind_addr}/"),
                        "websocket fallback port selected"
                    );
                }
                return Some(prepared_websocket_listener(candidate, listener));
            }
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error.to_string());
                }
                if should_cleanup_websocket_port(candidate.port()) {
                    warn!(
                        %error,
                        websocket = %format!("ws://{candidate}/"),
                        "websocket port unavailable; attempting one-time listener cleanup"
                    );
                    cleanup_websocket_port_owner(candidate.port());
                    if let Ok(listener) = bind_websocket_listener_after_cleanup(candidate) {
                        info!(
                            websocket = %format!("ws://{candidate}/"),
                            "websocket port selected after listener cleanup"
                        );
                        return Some(prepared_websocket_listener(candidate, listener));
                    }
                }
            }
        }
    }

    warn!(
        error = first_error.unwrap_or_else(|| "no candidate ports available".to_owned()),
        requested = %format!("ws://{bind_addr}/"),
        "websocket server unavailable"
    );
    None
}

fn bind_websocket_listener(bind_addr: SocketAddr) -> Result<StdTcpListener> {
    let listener = StdTcpListener::bind(bind_addr)
        .with_context(|| format!("failed to bind WebSocket server on {bind_addr}"))?;
    listener
        .set_nonblocking(true)
        .context("failed to configure WebSocket listener")?;
    Ok(listener)
}

fn bind_websocket_listener_after_cleanup(bind_addr: SocketAddr) -> Result<StdTcpListener> {
    let mut last_error = None;
    for _ in 0..10 {
        match bind_websocket_listener(bind_addr) {
            Ok(listener) => return Ok(listener),
            Err(error) => {
                last_error = Some(error);
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("failed to bind {bind_addr}")))
}

fn prepared_websocket_listener(
    fallback_addr: SocketAddr,
    listener: StdTcpListener,
) -> PreparedWebSocketListener {
    PreparedWebSocketListener {
        bind_addr: listener.local_addr().unwrap_or(fallback_addr),
        listener,
    }
}

fn websocket_bind_candidates(bind_addr: SocketAddr) -> impl Iterator<Item = SocketAddr> {
    (bind_addr.port()..=u16::MAX).map(move |port| SocketAddr::new(bind_addr.ip(), port))
}

fn should_cleanup_websocket_port(port: u16) -> bool {
    matches!(port, 6677 | 6678)
}

fn cleanup_websocket_port_owner(port: u16) {
    let pids = tcp_listener_pids(port);
    if pids.is_empty() {
        warn!(port, "no listener owner found for occupied websocket port");
        return;
    }

    let current_pid = std::process::id();
    for pid in pids {
        if pid == current_pid {
            warn!(
                port,
                pid, "skipping current process as websocket port owner"
            );
            continue;
        }
        match terminate_process(pid) {
            Ok(()) => info!(port, pid, "terminated process occupying websocket port"),
            Err(error) => warn!(%error, port, pid, "failed to terminate websocket port owner"),
        }
    }
}

#[cfg(windows)]
fn tcp_listener_pids(port: u16) -> BTreeSet<u32> {
    let output = windows_hidden_command("netstat")
        .args(["-ano", "-p", "tcp"])
        .output();
    let Ok(output) = output else {
        return BTreeSet::new();
    };

    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .filter_map(|line| tcp_listener_pid_from_netstat_line(line, port))
        .collect()
}

#[cfg(not(windows))]
fn tcp_listener_pids(_port: u16) -> BTreeSet<u32> {
    BTreeSet::new()
}

fn tcp_listener_pid_from_netstat_line(line: &str, port: u16) -> Option<u32> {
    let parts = line.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 5 || parts[0] != "TCP" {
        return None;
    }
    local_endpoint_matches_port(parts[1], port).then(|| parts.last()?.parse().ok())?
}

fn local_endpoint_matches_port(endpoint: &str, port: u16) -> bool {
    endpoint
        .rsplit_once(':')
        .and_then(|(_, endpoint_port)| endpoint_port.parse::<u16>().ok())
        == Some(port)
}

#[cfg(windows)]
fn terminate_process(pid: u32) -> Result<()> {
    let status = windows_hidden_command("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .status()
        .context("failed to launch taskkill")?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("taskkill exited with {status}")
    }
}

#[cfg(not(windows))]
fn terminate_process(pid: u32) -> Result<()> {
    anyhow::bail!("process termination is only implemented on Windows for pid {pid}")
}

#[cfg(windows)]
fn windows_hidden_command(program: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = std::process::Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
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
    fn local_websocket_url_uses_localhost_for_unspecified_bind() {
        let url = local_websocket_url(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 6677));

        assert_eq!(url, "ws://localhost:6677/");
    }

    #[test]
    fn websocket_bind_candidates_count_up_from_requested_port() {
        let candidates =
            websocket_bind_candidates(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 6677))
                .take(3)
                .collect::<Vec<_>>();

        assert_eq!(
            candidates,
            vec![
                SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 6677),
                SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 6678),
                SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 6679),
            ]
        );
    }

    #[test]
    fn websocket_cleanup_is_limited_to_first_compatibility_ports() {
        assert!(should_cleanup_websocket_port(6677));
        assert!(should_cleanup_websocket_port(6678));
        assert!(!should_cleanup_websocket_port(6679));
    }

    #[test]
    fn netstat_listener_parser_extracts_matching_port_pid() {
        let line = "  TCP    0.0.0.0:6677           0.0.0.0:0              LISTENING       1234";

        assert_eq!(tcp_listener_pid_from_netstat_line(line, 6677), Some(1234));
        assert_eq!(tcp_listener_pid_from_netstat_line(line, 6678), None);
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
