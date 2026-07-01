use anyhow::Result;
use bridge_protocol::{PipeLineEvent, MAX_FRAME_LEN};
use std::io;
use tokio::io::AsyncReadExt;
use tracing::{debug, error, info, warn};

use crate::state::AppState;

#[cfg(windows)]
pub async fn run_pipe_server(state: AppState) -> Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let pipe_name = state.pipe_name();
    info!(%pipe_name, "starting named pipe server");

    loop {
        let server = match ServerOptions::new().create(&pipe_name) {
            Ok(server) => server,
            Err(error) => {
                error!(%error, %pipe_name, "failed to create named pipe instance");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
        };

        match server.connect().await {
            Ok(()) => {
                debug!(%pipe_name, "named pipe client connected");
                let client_state = state.clone();
                tokio::spawn(async move {
                    if let Err(error) = handle_pipe_client(server, client_state).await {
                        debug!(%error, "named pipe client disconnected");
                    }
                });
            }
            Err(error) => {
                warn!(%error, %pipe_name, "named pipe connect failed");
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }
    }
}

#[cfg(windows)]
async fn handle_pipe_client(
    mut server: tokio::net::windows::named_pipe::NamedPipeServer,
    state: AppState,
) -> Result<()> {
    loop {
        let payload = match read_payload_async(&mut server).await {
            Ok(Some(payload)) => payload,
            Ok(None) => return Ok(()),
            Err(error) => return Err(error.into()),
        };

        match serde_json::from_slice::<PipeLineEvent>(&payload) {
            Ok(event) => {
                if let Err(error) = state.ingest_pipe_line(event).await {
                    warn!(%error, "failed to ingest pipe line event");
                }
            }
            Err(error) => {
                warn!(%error, "dropping malformed pipe payload");
            }
        }
    }
}

#[cfg(not(windows))]
pub async fn run_pipe_server(_state: AppState) -> Result<()> {
    warn!("named pipes are only available on Windows; pipe server disabled");
    futures_util::future::pending::<()>().await;
    Ok(())
}

async fn read_payload_async<R>(reader: &mut R) -> io::Result<Option<Vec<u8>>>
where
    R: AsyncReadExt + Unpin,
{
    let mut len = [0u8; 4];
    if let Err(error) = reader.read_exact(&mut len).await {
        return if error.kind() == io::ErrorKind::UnexpectedEof
            || error.kind() == io::ErrorKind::BrokenPipe
        {
            Ok(None)
        } else {
            Err(error)
        };
    }

    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("pipe payload too large: {len}"),
        ));
    }

    let mut payload = vec![0u8; len];
    if let Err(error) = reader.read_exact(&mut payload).await {
        return if error.kind() == io::ErrorKind::UnexpectedEof
            || error.kind() == io::ErrorKind::BrokenPipe
        {
            Ok(None)
        } else {
            Err(error)
        };
    }
    Ok(Some(payload))
}
