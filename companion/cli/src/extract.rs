//! `/api/extract`, native, in-process extraction via `js-host`. Runs
//! entirely on the user's machine (this binary), not the hosted server.
//!
//! This deliberately doesn't replicate the web pipeline's full
//! resolver/registry/dedupe/rank machinery (`extractor/src/core/pipeline.ts`)
//! It tries every installed adapter that claims to support the requested
//! kind, aggregates whatever streams and errors come back, and returns that.
//! Good enough for what the UI needs (a list of playable streams); ranking
//!/dedup can follow up if it turns out to matter in practice.

use std::sync::Arc;

use std::time::Duration;

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use extractor_core::{Capabilities, ExtractionFailure, ExtractionInput, ExtractionResult, StringOrNumber};
use js_host::{AdapterPackage, JsHost};
use serde::Deserialize;
use serde_json::json;

use super::state::AppState;

/// Per-adapter budget. Adapters fan out across many upstream hosts (federated
/// scrapers resolve nested sources and are legitimately slow), so this is a
/// backstop against a hung adapter rather than a target: because each adapter
/// runs, and is rendered, independently, a long budget costs nothing to the
/// sources that already answered.
///
/// `COMPANION_EXTRACT_TIMEOUT_MS` overrides it for every adapter;
/// `COMPANION_EXTRACT_TIMEOUTS="nxsha=300000,vidsrc2=20000"` overrides
/// individual ones.
const DEFAULT_EXTRACT_TIMEOUT_MS: u64 = 180_000;

fn extract_timeout_for(extractor_id: &str) -> Duration {
    if let Ok(raw) = std::env::var("COMPANION_EXTRACT_TIMEOUTS") {
        for entry in raw.split(',') {
            let mut parts = entry.split('=').map(str::trim);
            let (Some(id), Some(value)) = (parts.next(), parts.next()) else { continue };
            if id != extractor_id {
                continue;
            }
            if let Some(ms) = value.parse::<u64>().ok().filter(|ms| *ms > 0) {
                return Duration::from_millis(ms);
            }
        }
    }

    let ms = std::env::var("COMPANION_EXTRACT_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|ms| *ms > 0)
        .unwrap_or(DEFAULT_EXTRACT_TIMEOUT_MS);
    Duration::from_millis(ms)
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/api/extract", post(handler))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtractBody {
    kind: String,
    #[serde(default)]
    tmdb_id: Option<StringOrNumber>,
    #[serde(default)]
    imdb_id: Option<String>,
    #[serde(default)]
    season: Option<u32>,
    #[serde(default)]
    episode: Option<u32>,
    #[serde(default)]
    extractors: Option<Vec<String>>,
}

async fn handler(State(state): State<Arc<AppState>>, Json(body): Json<ExtractBody>) -> impl IntoResponse {
    let input = match body.kind.as_str() {
        "movie" => ExtractionInput::Movie {
            imdb_id: body.imdb_id,
            tmdb_id: body.tmdb_id,
            title: None,
            year: None,
        },
        "episode" => {
            let (Some(season), Some(episode)) = (body.season, body.episode) else {
                return err(StatusCode::BAD_REQUEST, "season and episode are required for TV");
            };
            ExtractionInput::Episode {
                imdb_id: body.imdb_id,
                tmdb_id: body.tmdb_id,
                season,
                episode,
                title: None,
            }
        }
        _ => return err(StatusCode::BAD_REQUEST, "Unsupported kind"),
    };

    let candidates: Vec<AdapterPackage> = {
        let installed = state.installed.read().await;
        installed
            .iter()
            .filter(|p| body.extractors.as_ref().map_or(true, |ids| ids.iter().any(|id| id == &p.manifest.id)))
            .filter(|p| supports_kind(&p.manifest.capabilities, &input))
            .cloned()
            .collect()
    };

    let mut streams = Vec::new();
    let mut sources = Vec::new();
    let mut errors: Vec<ExtractionFailure> = Vec::new();
    let mut duration_ms = 0u64;

    // Adapters run concurrently: a slow federated scraper must not hold back
    // the fast direct resolvers, and each one fails (or times out) on its own.
    let mut set = tokio::task::JoinSet::new();
    for package in candidates {
        let input = input.clone();
        let extractor_id = package.manifest.id.clone();
        let timeout = extract_timeout_for(&extractor_id);
        set.spawn(async move {
            let blocking = tokio::task::spawn_blocking(move || JsHost::new().extract(&package, &input));
            match tokio::time::timeout(timeout, blocking).await {
                Ok(Ok(Ok(result))) => Ok(result),
                Ok(Ok(Err(e))) => Err(ExtractionFailure {
                    extractor_id,
                    code: "UNKNOWN".to_string(),
                    message: e.to_string(),
                    layer: Some("extraction".to_string()),
                }),
                Ok(Err(join_err)) => Err(ExtractionFailure {
                    extractor_id,
                    code: "UNKNOWN".to_string(),
                    message: format!("adapter task failed: {join_err}"),
                    layer: Some("extraction".to_string()),
                }),
                Err(_) => Err(ExtractionFailure {
                    extractor_id,
                    code: "TIMEOUT".to_string(),
                    message: format!("Extraction timed out after {}s", timeout.as_secs()),
                    layer: Some("extraction".to_string()),
                }),
            }
        });
    }

    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(Ok(result)) => {
                streams.extend(result.streams);
                sources.extend(result.sources);
                errors.extend(result.errors);
                duration_ms += result.duration_ms;
            }
            Ok(Err(failure)) => errors.push(failure),
            Err(join_err) => errors.push(ExtractionFailure {
                extractor_id: "*".to_string(),
                code: "UNKNOWN".to_string(),
                message: format!("adapter task failed: {join_err}"),
                layer: Some("extraction".to_string()),
            }),
        }
    }

    if streams.is_empty() && errors.is_empty() {
        errors.push(ExtractionFailure {
            extractor_id: body.extractors.and_then(|e| e.into_iter().next()).unwrap_or_else(|| "*".to_string()),
            code: "NO_STREAM".to_string(),
            message: "No source pages discovered".to_string(),
            layer: Some("discovery".to_string()),
        });
    }

    let result = ExtractionResult { input, streams, sources, errors, duration_ms };
    (StatusCode::OK, Json(result)).into_response()
}

fn supports_kind(capabilities: &Capabilities, input: &ExtractionInput) -> bool {
    match input {
        ExtractionInput::Movie { .. } => capabilities.movie,
        ExtractionInput::Episode { .. } => capabilities.episodes,
        ExtractionInput::Series { .. } => capabilities.series,
        ExtractionInput::ShortDrama { .. } => capabilities.short_drama,
    }
}

fn err(status: StatusCode, message: &str) -> axum::response::Response {
    (status, Json(json!({ "error": message }))).into_response()
}
