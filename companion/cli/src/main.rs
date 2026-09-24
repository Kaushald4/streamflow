//! Streamflow Companion: a small background process users run locally,
//! *only* if they want to stream from third-party adapters. It's a bare
//! binary with no window/webview: the UI stays on the hosted Streamflow
//! website, which talks to this process on `127.0.0.1` for the two things
//! that would otherwise burn the host's bandwidth, extraction and stream
//! proxying. TMDB metadata browsing needs neither of those and stays on
//! the hosted site.

mod adapters;
mod extract;
mod state;
mod stream_proxy;
mod stream_session;
mod vidsrc_token;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use js_host::JsHost;
use rand::RngCore;
use serde_json::json;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;

use state::AppState;

const DEFAULT_PORT: u16 = 4310;
const TOKEN_HEADER: &str = "x-companion-token";

/// The companion only ever needs to accept requests that name the hosted
/// site as their origin, browsers can't forge the `Origin` header, so this
/// is a real defense against other websites the user has open silently
/// probing/commanding a server listening on their loopback address. Set
/// `COMPANION_ALLOWED_ORIGINS` (comma-separated) to the real hosted
/// domain(s) in production; defaults cover local development only.
fn allowed_origins() -> Vec<HeaderValue> {
    let raw = std::env::var("COMPANION_ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:3000,http://127.0.0.1:3000,http://192.168.1.34:3000".to_string());
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|origin| HeaderValue::from_str(origin).ok())
        .collect()
}

fn data_root() -> PathBuf {
    std::env::var("STREAMFLOW_DATA_DIR").map(PathBuf::from).unwrap_or_else(|_| {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".streamflow")
    })
}

fn adapters_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("STREAMFLOW_ADAPTERS_DIR") {
        return PathBuf::from(dir);
    }
    data_root().join("adapters")
}

/// Loads the persisted pairing token, generating one on first run. Kept on
/// disk (not regenerated every launch) so the web app only ever needs to
/// pair once per install, not every time the companion restarts.
fn load_or_create_pairing_token(root: &Path) -> anyhow::Result<String> {
    std::fs::create_dir_all(root)?;
    let token_path = root.join("token");

    if let Ok(existing) = std::fs::read_to_string(&token_path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();

    std::fs::write(&token_path, &token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o600));
    }

    Ok(token)
}

fn port() -> u16 {
    std::env::args()
        .skip_while(|a| a != "--port")
        .nth(1)
        .and_then(|s| s.parse().ok())
        .or_else(|| std::env::var("COMPANION_PORT").ok().and_then(|s| s.parse().ok()))
        .unwrap_or(DEFAULT_PORT)
}

async fn health() -> &'static str {
    "ok"
}

/// Unauthenticated by design: the token itself has to come from somewhere,
/// and this is that somewhere. Safe to leave open because the only thing at
/// stake is *reading* the response, which CORS's origin allowlist already
/// protects (a cross-origin page's script can't read it; a `<form>` can't
/// either, since forms have no way to read a response body at all). What
/// CORS can't stop is a bare `<form>` POST *acting* on a mutating endpoint
/// without ever reading the reply, that's what the token on those routes
/// is for.
async fn pair(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(json!({ "token": state.pairing_token }))
}

/// Enforces `X-Companion-Token` on every non-GET request. CORS's origin
/// allowlist alone doesn't stop CSRF: a plain HTML `<form>` POST to a
/// mutating endpoint is sent (and acted on) regardless of CORS, CORS only
/// gates whether the calling page can *read* the response, not whether the
/// request happens. Requiring a secret the attacker can't read (see `pair`
/// above) closes that gap independent of CORS.
async fn require_token_for_mutations(State(state): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    if req.method() != Method::GET {
        let provided = req.headers().get(TOKEN_HEADER).and_then(|v| v.to_str().ok());
        if provided != Some(state.pairing_token.as_str()) {
            return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "Missing or invalid X-Companion-Token" })))
                .into_response();
        }
    }
    next.run(req).await
}

fn build_app(state: Arc<AppState>) -> axum::Router {
    let cors = CorsLayer::new()
        .allow_origin(allowed_origins())
        .allow_methods([Method::GET, Method::POST, Method::DELETE])
        .allow_headers(tower_http::cors::Any);

    axum::Router::new()
        .route("/api/health", axum::routing::get(health))
        .route("/api/pair", axum::routing::get(pair))
        .merge(adapters::router())
        .merge(extract::router())
        .route("/api/stream", axum::routing::get(stream_proxy::handler))
        .route("/api/stream/register", axum::routing::post(stream_session::register_handler))
        .route("/api/stream/s/{id}", axum::routing::get(stream_session::session_handler))
        // Order matters: this must run *before* CORS's own OPTIONS
        // preflight handling, i.e. be the inner layer (added first), so a
        // preflight request (method OPTIONS, no custom header yet) never
        // hits the token check meant for the real request that follows it.
        .layer(axum::middleware::from_fn_with_state(state.clone(), require_token_for_mutations))
        .layer(cors)
        .with_state(state)
}

async fn build_state(adapters_dir: PathBuf, pairing_token: String) -> anyhow::Result<Arc<AppState>> {
    std::fs::create_dir_all(&adapters_dir)?;
    let js_host = JsHost::new();
    let installed = adapters::load_installed_from_disk(&js_host, &adapters_dir);

    // SSRF guard: the stream proxy's own upstream fetches must never be
    // able to reach the user's LAN, same reasoning as js-host's adapter
    // http bridge.
    let http = reqwest::Client::builder()
        .dns_resolver(std::sync::Arc::new(extractor_core::ssrf::SsrfSafeResolver))
        .build()?;

    Ok(Arc::new(AppState {
        http,
        js_host,
        adapters_dir,
        installed: RwLock::new(installed),
        pairing_token,
        stream_sessions: RwLock::new(HashMap::new()),
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pairing_token = load_or_create_pairing_token(&data_root())?;
    let state = build_state(adapters_dir(), pairing_token).await?;
    let app = build_app(state);

    let port = port();
    let origin = "127.0.0.1";
    let listener = tokio::net::TcpListener::bind((origin, port)).await?;
    println!("Streamflow Companion listening on http://{origin}:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

const TEST_TOKEN: &str = "test-pairing-token";

async fn spawn_test_server() -> (String, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let state = build_state(tmp.path().join("adapters"), TEST_TOKEN.to_string()).await.unwrap();
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), tmp)
}

    #[tokio::test]
    async fn stream_proxy_refuses_a_literal_private_ip_target() {
        let (base, _tmp) = spawn_test_server().await;
        let res = reqwest::get(format!(
            "{base}/api/stream?token={TEST_TOKEN}&target={}",
            urlencoding_for_test("http://127.0.0.1:1/movie.m3u8")
        ))
        .await
        .unwrap();
        assert_eq!(res.status(), 403);
    }

    /// The open-relay fix: without this, any page the user has open could
    /// embed a plain <video>/<img> tag pointing at this endpoint (no
    /// JavaScript, no CORS bypass needed) and get the companion to fetch
    /// an arbitrary public URL on its behalf.
    #[tokio::test]
    async fn stream_proxy_refuses_requests_without_a_valid_token() {
        let (base, _tmp) = spawn_test_server().await;

        let res = reqwest::get(format!("{base}/api/stream?url={}", urlencoding_for_test("https://example.com/a.m3u8")))
            .await
            .unwrap();
        assert_eq!(res.status(), 401, "missing token must be rejected");

        let res = reqwest::get(format!(
            "{base}/api/stream?token=wrong&url={}",
            urlencoding_for_test("https://example.com/a.m3u8")
        ))
        .await
        .unwrap();
        assert_eq!(res.status(), 401, "wrong token must be rejected the same as no token");
    }

    fn urlencoding_for_test(s: &str) -> String {
        reqwest::Url::parse_with_params("http://x/", [("target", s)])
            .unwrap()
            .query()
            .unwrap()
            .strip_prefix("target=")
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn health_check_responds_ok() {
        let (base, _tmp) = spawn_test_server().await;
        let res = reqwest::get(format!("{base}/api/health")).await.unwrap();
        assert_eq!(res.status(), 200);
    }

    #[tokio::test]
    async fn pair_endpoint_returns_the_real_token_unauthenticated() {
        let (base, _tmp) = spawn_test_server().await;
        let res = reqwest::get(format!("{base}/api/pair")).await.unwrap();
        assert_eq!(res.status(), 200);
        let body: serde_json::Value = res.json().await.unwrap();
        assert_eq!(body["token"], TEST_TOKEN);
    }

    /// The actual CSRF fix: a plain cross-origin-style POST with no custom
    /// header (exactly what a malicious `<form>` submission looks like,
    /// forms cannot set custom headers) must be rejected, not silently
    /// processed just because CORS doesn't apply to it.
    #[tokio::test]
    async fn mutating_endpoints_reject_requests_without_the_token() {
        let (base, _tmp) = spawn_test_server().await;
        let client = reqwest::Client::new();

        let res = client
            .post(format!("{base}/api/extract"))
            .json(&serde_json::json!({ "kind": "movie", "imdbId": "tt1" }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 401);

        let res = client
            .post(format!("{base}/api/adapters/upload"))
            .body(Vec::<u8>::new())
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 401);

        let res = client.delete(format!("{base}/api/adapters/whatever")).send().await.unwrap();
        assert_eq!(res.status(), 401);

        // Wrong token is rejected the same as no token.
        let res = client
            .post(format!("{base}/api/extract"))
            .header("X-Companion-Token", "not-the-real-token")
            .json(&serde_json::json!({ "kind": "movie", "imdbId": "tt1" }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 401);
    }

    #[tokio::test]
    async fn read_only_endpoints_do_not_require_the_token() {
        let (base, _tmp) = spawn_test_server().await;
        let res = reqwest::get(format!("{base}/api/adapters")).await.unwrap();
        assert_eq!(res.status(), 200);
    }

    #[tokio::test]
    async fn adapter_install_list_uninstall_round_trip_over_http() {
        let (base, _tmp) = spawn_test_server().await;
        let client = reqwest::Client::new();

        let res = client.get(format!("{base}/api/adapters")).send().await.unwrap();
        let body: serde_json::Value = res.json().await.unwrap();
        assert_eq!(body["adapters"].as_array().unwrap().len(), 0);

        let js_bytes = br#"
            export function createAdapter(http) {
                return {
                    async findMovie(input) { return [{ extractorId: "http-test-adapter", url: "test://x", kind: "movie", meta: {} }]; },
                    async extract(source) {
                        return [{ url: "https://example.com/stream.m3u8", type: "hls", source: { extractor: "http-test-adapter", version: "0.1.0" } }];
                    },
                };
            }
        "#;
        let manifest = br#"{
            "id": "http-test-adapter", "name": "HTTP Test Adapter", "version": "0.1.0",
            "entry": "index.js",
            "capabilities": { "movie": true, "series": false, "episodes": false,
                "shortDrama": false, "subtitles": false, "multipleQualities": false, "directStreams": true }
        }"#;
        let zip_bytes = zip_package(manifest, js_bytes);

        let part = reqwest::multipart::Part::bytes(zip_bytes).file_name("adapter.zip");
        let form = reqwest::multipart::Form::new().part("package", part);
        let res = client
            .post(format!("{base}/api/adapters/upload"))
            .header("X-Companion-Token", TEST_TOKEN)
            .multipart(form)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200, "upload should succeed");
        let body: serde_json::Value = res.json().await.unwrap();
        assert_eq!(body["adapter"]["id"], "http-test-adapter");

        let res = client.get(format!("{base}/api/adapters")).send().await.unwrap();
        let body: serde_json::Value = res.json().await.unwrap();
        assert_eq!(body["adapters"].as_array().unwrap().len(), 1);

        let res = client
            .delete(format!("{base}/api/adapters/http-test-adapter"))
            .header("X-Companion-Token", TEST_TOKEN)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200);

        let res = client.get(format!("{base}/api/adapters")).send().await.unwrap();
        let body: serde_json::Value = res.json().await.unwrap();
        assert_eq!(body["adapters"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn extract_runs_installed_adapters_natively() {
        let (base, _tmp) = spawn_test_server().await;
        let client = reqwest::Client::new();

        let js_bytes = br#"
            export function createAdapter(http) {
                return {
                    async findMovie(input) {
                        return [{ extractorId: "extract-test-adapter", url: "test://" + input.imdbId, kind: "movie", meta: {} }];
                    },
                    async extract(source) {
                        return [{
                            url: "https://example.com/native-stream.m3u8",
                            type: "hls",
                            source: { extractor: "extract-test-adapter", version: "0.1.0" },
                        }];
                    },
                };
            }
        "#;
        let manifest = br#"{
            "id": "extract-test-adapter", "name": "Extract Test Adapter", "version": "0.1.0",
            "entry": "index.js",
            "capabilities": { "movie": true, "series": false, "episodes": false,
                "shortDrama": false, "subtitles": false, "multipleQualities": false, "directStreams": true }
        }"#;
        let zip_bytes = zip_package(manifest, js_bytes);
        let part = reqwest::multipart::Part::bytes(zip_bytes).file_name("adapter.zip");
        let form = reqwest::multipart::Form::new().part("package", part);
        let res = client
            .post(format!("{base}/api/adapters/upload"))
            .header("X-Companion-Token", TEST_TOKEN)
            .multipart(form)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200, "upload should succeed");

        let res = client
            .post(format!("{base}/api/extract"))
            .header("X-Companion-Token", TEST_TOKEN)
            .json(&serde_json::json!({ "kind": "movie", "imdbId": "tt1234567" }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200);
        let body: serde_json::Value = res.json().await.unwrap();
        assert!(body["errors"].as_array().unwrap().is_empty(), "unexpected errors: {body:?}");
        let streams = body["streams"].as_array().unwrap();
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0]["url"], "https://example.com/native-stream.m3u8");
    }

    fn zip_package(manifest_json: &[u8], js_bytes: &[u8]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();
            writer.start_file("manifest.json", opts).unwrap();
            std::io::Write::write_all(&mut writer, manifest_json).unwrap();
            writer.start_file("index.js", opts).unwrap();
            std::io::Write::write_all(&mut writer, js_bytes).unwrap();
            writer.finish().unwrap();
        }
        buf.into_inner()
    }
}
