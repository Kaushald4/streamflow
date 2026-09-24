//! In-memory playback sessions: headers live on the companion, the player
//! gets an opaque `/api/stream/s/{id}?token=…` URL (no CDN URL in query).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use rand::RngCore;
use reqwest::Url;
use serde::Deserialize;
use serde::Serialize;

use crate::state::AppState;
use crate::stream_proxy::{self, StreamProxyHeaders};

const SESSION_TTL: Duration = Duration::from_secs(4 * 3600);
const MAX_SESSIONS: usize = 256;

#[derive(Clone)]
pub struct StreamSession {
    pub target: String,
    pub headers: StreamProxyHeaders,
    pub created_at: Instant,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterStreamBody {
    pub target: String,
    pub referer: Option<String>,
    pub origin: Option<String>,
    pub site_referer: Option<bool>,
    pub omit_origin: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterStreamResponse {
    pub id: String,
}

#[derive(Deserialize)]
pub struct SessionPlayQuery {
    pub token: Option<String>,
    /// HLS segments / subtitles override the session's primary target.
    pub target: Option<String>,
    pub parent: Option<String>,
}

pub async fn register_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RegisterStreamBody>,
) -> impl IntoResponse {
    if let Err(message) = validate_target(&body.target) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": message }))).into_response();
    }

    let headers = StreamProxyHeaders {
        referer: body.referer,
        origin: body.origin,
        parent: None,
        site_referer: body.site_referer.unwrap_or(false),
        omit_origin: body.omit_origin.unwrap_or(false),
    };

    let id = new_session_id();
    let session = StreamSession {
        target: body.target,
        headers,
        created_at: Instant::now(),
    };

    {
        let mut store = state.stream_sessions.write().await;
        purge_stale(&mut store);
        if store.len() >= MAX_SESSIONS {
            if let Some(oldest) = oldest_key(&store) {
                store.remove(&oldest);
            }
        }
        store.insert(id.clone(), session);
    }

    Json(RegisterStreamResponse { id }).into_response()
}

pub async fn session_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(q): Query<SessionPlayQuery>,
    req: axum::http::Request<axum::body::Body>,
) -> axum::response::Response {
    if q.token.as_deref() != Some(state.pairing_token.as_str()) {
        return stream_proxy::err(StatusCode::UNAUTHORIZED, "Missing or invalid token");
    }

    let session = {
        let store = state.stream_sessions.read().await;
        store.get(&id).cloned()
    };

    let Some(session) = session else {
        return stream_proxy::err(StatusCode::NOT_FOUND, "Stream session not found or expired");
    };

    if session.created_at.elapsed() > SESSION_TTL {
        state.stream_sessions.write().await.remove(&id);
        return stream_proxy::err(StatusCode::NOT_FOUND, "Stream session expired");
    }

    let mut headers = session.headers.clone();
    if let Some(parent) = q.parent {
        headers.parent = Some(parent);
    }

    let target = q.target.unwrap_or(session.target);
    stream_proxy::serve_media(
        &state,
        &target,
        &headers,
        Some(&id),
        req,
    )
    .await
}

fn new_session_id() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn purge_stale(store: &mut HashMap<String, StreamSession>) {
    store.retain(|_, s| s.created_at.elapsed() <= SESSION_TTL);
}

fn oldest_key(store: &HashMap<String, StreamSession>) -> Option<String> {
    store
        .iter()
        .min_by_key(|(_, s)| s.created_at)
        .map(|(k, _)| k.clone())
}

fn validate_target(target: &str) -> Result<(), String> {
    let parsed = Url::parse(target).map_err(|_| "Invalid target URL".to_string())?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("Unsupported protocol".into());
    }
    extractor_core::ssrf::check_target_url(target).map_err(|e| e.to_string())
}
