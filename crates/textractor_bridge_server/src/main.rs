mod assets;
mod config;
mod history;
mod http;
mod media;
mod pipe;
mod state;
mod time;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    time::Duration,
};
use tokio::net::TcpListener;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::{config::AppConfig, state::AppState};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let args = Args::parse();
    let (config, config_path) = AppConfig::load_from_default_locations(args.config)?;
    let bind_addr = config.bind_addr();
    let state = AppState::load_with_config_path(config, config_path.clone())?;
    let local_url = local_browser_url(bind_addr, state.session_token());
    let session_info_path = write_session_info(bind_addr, state.session_token());

    info!(
        config = config_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<defaults>".to_owned()),
        data_dir = %state.dirs().root.display(),
        pipe = %state.pipe_name(),
        http = %format!("http://{bind_addr}"),
        local_url = %local_url,
        session_info = session_info_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unavailable>".to_owned()),
        "textractor media bridge starting"
    );
    if let Some(token) = state.session_token() {
        info!(
            %token,
            phone_url = %format!("http://<PC-LAN-IP>:{}?token={}", bind_addr.port(), token),
            "LAN session token"
        );
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
        open_browser(&local_url);
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

fn local_browser_url(bind_addr: SocketAddr, token: Option<&str>) -> String {
    let host = match bind_addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => "127.0.0.1".to_owned(),
        IpAddr::V6(ip) if ip.is_unspecified() => "[::1]".to_owned(),
        IpAddr::V6(ip) => format!("[{ip}]"),
        IpAddr::V4(ip) => ip.to_string(),
    };
    let mut url = format!("http://{host}:{}", bind_addr.port());
    if let Some(token) = token {
        url.push_str("?token=");
        url.push_str(token);
    }
    url
}

fn write_session_info(bind_addr: SocketAddr, token: Option<&str>) -> Option<PathBuf> {
    let path = std::env::current_dir()
        .ok()?
        .join("textractor_bridge_server.session.json");
    let payload = session_info(bind_addr, token);
    let bytes = match serde_json::to_vec_pretty(&payload) {
        Ok(bytes) => bytes,
        Err(error) => {
            warn!(%error, "failed to serialize session info");
            return None;
        }
    };
    if let Err(error) = std::fs::write(&path, bytes) {
        warn!(%error, path = %path.display(), "failed to write session info");
        return None;
    }
    Some(path)
}

fn session_info(bind_addr: SocketAddr, token: Option<&str>) -> Value {
    json!({
        "bind": format!("http://{bind_addr}"),
        "localUrl": local_browser_url(bind_addr, token),
        "phoneUrlTemplate": phone_url_template(bind_addr, token),
        "sessionTokenRequired": token.is_some(),
        "sessionToken": token,
    })
}

fn phone_url_template(bind_addr: SocketAddr, token: Option<&str>) -> String {
    let mut url = format!("http://<PC-LAN-IP>:{}", bind_addr.port());
    if let Some(token) = token {
        url.push_str("?token=");
        url.push_str(token);
    }
    url
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
        let url = local_browser_url(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 7788),
            Some("abc"),
        );

        assert_eq!(url, "http://127.0.0.1:7788?token=abc");
    }

    #[test]
    fn local_browser_url_formats_ipv6_hosts() {
        let url = local_browser_url(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 7788), None);

        assert_eq!(url, "http://[::1]:7788");
    }

    #[test]
    fn session_info_includes_phone_url_template_and_token() {
        let info = session_info(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 7788),
            Some("abc"),
        );

        assert_eq!(info["localUrl"], "http://127.0.0.1:7788?token=abc");
        assert_eq!(
            info["phoneUrlTemplate"],
            "http://<PC-LAN-IP>:7788?token=abc"
        );
        assert_eq!(info["sessionToken"], "abc");
        assert_eq!(info["sessionTokenRequired"], true);
    }

    #[test]
    fn session_info_omits_token_when_not_required() {
        let info = session_info(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 7788),
            None,
        );

        assert_eq!(info["localUrl"], "http://127.0.0.1:7788");
        assert_eq!(info["phoneUrlTemplate"], "http://<PC-LAN-IP>:7788");
        assert!(info["sessionToken"].is_null());
        assert_eq!(info["sessionTokenRequired"], false);
    }
}
