use anyhow::{Context, Result};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use std::net::TcpListener as StdTcpListener;
use tokio::{net::TcpListener, sync::watch};
use tracing::{debug, info, warn};

use crate::state::AppState;

pub async fn run(
    state: AppState,
    listener: StdTcpListener,
    shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let bind_addr = listener
        .local_addr()
        .context("failed to inspect WebSocket listener")?;
    let listener =
        TcpListener::from_std(listener).context("failed to create WebSocket listener")?;

    info!(websocket = %format!("ws://{bind_addr}/"), "websocket server listening");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(wait_for_shutdown(shutdown))
        .await
        .context("websocket server failed")?;
    info!("websocket server shutdown completed");
    Ok(())
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(websocket))
        .route("/{*path}", get(websocket))
        .with_state(state)
}

async fn websocket(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let mut text_rx = state.subscribe_websocket_text();
    let (mut sender, mut incoming) = socket.split();

    loop {
        tokio::select! {
            message = text_rx.recv() => {
                match message {
                    Ok(text) => {
                        if sender.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!(skipped, "websocket client lagged behind text stream");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            message = incoming.next() => {
                match message {
                    Some(Ok(Message::Ping(payload))) => {
                        if sender.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let _ = sender.send(Message::Close(frame)).await;
                        break;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        debug!(%error, "websocket client connection failed");
                        break;
                    }
                    None => break,
                }
            }
        }
    }
}

async fn wait_for_shutdown(mut shutdown: watch::Receiver<bool>) {
    if *shutdown.borrow() {
        return;
    }
    let _ = shutdown.changed().await;
}
