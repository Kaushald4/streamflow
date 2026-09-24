use std::collections::HashMap;
use std::path::PathBuf;

use js_host::{AdapterPackage, JsHost};
use tokio::sync::RwLock;

use crate::stream_session::StreamSession;

/// Shared server state. Wrapped in `Arc` by the caller (axum's `.with_state`
/// requires `Clone`; we clone the `Arc`, not this struct).
pub struct AppState {
    pub http: reqwest::Client,
    pub js_host: JsHost,
    /// Where installed adapter packages live on disk: `<adapters_dir>/<id>/{manifest.json,<entry>}`.
    pub adapters_dir: PathBuf,
    pub installed: RwLock<Vec<AdapterPackage>>,
    /// Random secret generated on first run, persisted to disk. Required
    /// (via the `X-Companion-Token` header) on every state-changing
    /// endpoint, CORS's origin allowlist stops a malicious page's *script*
    /// from reading responses, but not a plain `<form>` POST from ever
    /// being sent, so it alone doesn't stop CSRF. See `require_token_for_mutations`.
    pub pairing_token: String,
    /// Opaque playback sessions, headers stored server-side, player gets `/api/stream/s/{id}`.
    pub stream_sessions: RwLock<HashMap<String, StreamSession>>,
}
