use axum::{
    body::Body,
    extract::{Path, Query, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode, Uri},
    middleware::{self, Next},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use bridge_protocol::{
    AudioEndReason, BrowserEvent, BrowserHello, BrowserLineAddedEvent, ErrorEvent, LineId,
    MinePrepareRequest, PROTOCOL_VERSION,
};
use serde::{Deserialize, Serialize};
use std::{convert::Infallible, path::PathBuf, time::Duration};
use tower_http::{cors::CorsLayer, services::ServeDir, trace::TraceLayer};
use tracing::warn;

use crate::{config::AppConfig, state::AppState};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinesQuery {
    limit: Option<usize>,
    before_seq: Option<u64>,
    after_seq: Option<u64>,
    source_key: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicConfig {
    protocol_version: u32,
    config: AppConfig,
    pipe_name: String,
    data_dir: String,
    session_token_required: bool,
    session_token: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Health {
    ok: bool,
    protocol_version: u32,
    newest_seq: Option<u64>,
}

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/config", get(config))
        .route("/api/events", get(events))
        .route("/api/lines", get(lines))
        .route("/api/lines/{line_id}/audio/finish", post(finish_audio))
        .route("/api/mine/prepare", post(mine_prepare))
        .route("/api/assets/{asset_id}/base64", post(asset_base64))
        .route("/assets/{asset_id}", get(asset_download))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_session_token,
        ));

    Router::new()
        .route("/api/health", get(health))
        .merge(protected)
        .fallback_service(ServeDir::new(web_ui_dir()).append_index_html_on_directories(true))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        ok: true,
        protocol_version: PROTOCOL_VERSION,
        newest_seq: state.newest_seq(),
    })
}

async fn config(State(state): State<AppState>) -> Json<PublicConfig> {
    Json(PublicConfig {
        protocol_version: PROTOCOL_VERSION,
        config: state.config().clone(),
        pipe_name: state.pipe_name(),
        data_dir: state.dirs().root.display().to_string(),
        session_token_required: state.token_required(),
        session_token: state.session_token().map(ToOwned::to_owned),
    })
}

async fn lines(
    State(state): State<AppState>,
    Query(query): Query<LinesQuery>,
) -> Json<bridge_protocol::LineHistoryPage> {
    Json(state.line_page(
        query.limit.unwrap_or(100),
        query.before_seq,
        query.after_seq,
        query.source_key.as_deref(),
    ))
}

async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let mut receiver = state.subscribe();
    let replay = last_event_id
        .map(|seq| state.line_page(500, None, Some(seq), None).lines)
        .unwrap_or_default();
    let newest = state.newest_seq();

    let stream = async_stream::stream! {
        let hello = BrowserEvent::Hello(BrowserHello {
            protocol_version: PROTOCOL_VERSION,
            server_version: env!("CARGO_PKG_VERSION").to_owned(),
            newest_seq: newest,
        });
        if let Some(event) = sse_event("hello", newest.unwrap_or(0), &hello) {
            yield Ok(event);
        }

        for line in replay {
            let seq = line.line_seq;
            let payload = BrowserEvent::LineAdded(BrowserLineAddedEvent { line });
            if let Some(event) = sse_event("line_added", seq, &payload) {
                yield Ok(event);
            }
        }

        loop {
            match receiver.recv().await {
                Ok(message) => {
                    if let Some(event) = sse_event(message.event_name, message.id, &message.payload) {
                        yield Ok(event);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    let payload = BrowserEvent::Error(ErrorEvent {
                        code: "sse_lagged".to_owned(),
                        message: format!("client skipped {skipped} event(s); refresh history"),
                    });
                    if let Some(event) = sse_event("error", newest.unwrap_or(0), &payload) {
                        yield Ok(event);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

async fn finish_audio(
    State(state): State<AppState>,
    Path(line_id): Path<LineId>,
) -> Result<Json<bridge_protocol::AudioFinishResponse>, ApiError> {
    Ok(Json(
        state
            .finish_audio(line_id, AudioEndReason::Manual)
            .await
            .map_err(ApiError::bad_request)?,
    ))
}

async fn mine_prepare(
    State(state): State<AppState>,
    Json(request): Json<MinePrepareRequest>,
) -> Result<Json<bridge_protocol::MinePrepareResponse>, ApiError> {
    Ok(Json(
        state.prepare_mine(request).map_err(ApiError::bad_request)?,
    ))
}

async fn asset_base64(
    State(state): State<AppState>,
    Path(asset_id): Path<String>,
) -> Result<Json<bridge_protocol::AssetBase64Response>, ApiError> {
    Ok(Json(
        state.asset_base64(&asset_id).map_err(ApiError::not_found)?,
    ))
}

async fn asset_download(
    State(state): State<AppState>,
    Path(asset_id): Path<String>,
) -> Result<Response, ApiError> {
    let asset = state
        .find_asset_info(&asset_id)
        .ok_or_else(|| ApiError::not_found(anyhow::anyhow!("asset not found")))?;
    let bytes = state
        .load_asset_bytes(&asset_id)
        .map_err(ApiError::not_found)?;

    let mut response = Body::from(bytes).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&asset.mime_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    Ok(response)
}

async fn require_session_token(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    if !state.token_required() || request_has_token(&req, state.session_token()) {
        return next.run(req).await;
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorEvent {
            code: "session_token_required".to_owned(),
            message: "session token required".to_owned(),
        }),
    )
        .into_response()
}

fn request_has_token(req: &Request, expected: Option<&str>) -> bool {
    let Some(expected) = expected else {
        return true;
    };

    let header_token = req
        .headers()
        .get("x-session-token")
        .and_then(|value| value.to_str().ok())
        .or_else(|| {
            req.headers()
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
        });
    if header_token == Some(expected) {
        return true;
    }

    query_param(req.uri(), "token").as_deref() == Some(expected)
}

fn query_param(uri: &Uri, name: &str) -> Option<String> {
    uri.query()?.split('&').find_map(|pair| {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next()?;
        let value = parts.next().unwrap_or_default();
        (key == name).then(|| value.to_owned())
    })
}

fn sse_event(event_name: &str, id: u64, payload: &BrowserEvent) -> Option<Event> {
    match serde_json::to_string(payload) {
        Ok(data) => Some(
            Event::default()
                .event(event_name)
                .id(id.to_string())
                .data(data),
        ),
        Err(error) => {
            warn!(%error, event_name, id, "failed to serialize SSE event");
            None
        }
    }
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    code: &'static str,
    error: anyhow::Error,
}

impl ApiError {
    fn bad_request(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "bad_request",
            error,
        }
    }

    fn not_found(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            error,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorEvent {
                code: self.code.to_owned(),
                message: self.error.to_string(),
            }),
        )
            .into_response()
    }
}

fn web_ui_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("TEXTRACTOR_MEDIA_BRIDGE_WEB_UI").map(PathBuf::from) {
        return path;
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let local_dist = cwd.join("web_ui").join("dist");
    if local_dist.exists() {
        return local_dist;
    }
    let local_source = cwd.join("web_ui");
    if local_source.exists() {
        return local_source;
    }
    let workspace_web_ui = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("web_ui");
    let workspace_dist = workspace_web_ui.join("dist");
    if workspace_dist.exists() {
        workspace_dist
    } else {
        workspace_web_ui
    }
}
