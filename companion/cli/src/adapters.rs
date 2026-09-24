//! The companion's adapter registry, installs the same JS+manifest zip
//! package format the web build's own adapter pipeline uses. Adapters run
//! as plain JS via `js-host`'s embedded QuickJS engine; the one native
//! piece is the shared `primitives/wasm-decrypt` primitive, resolved by
//! `js-host` at runtime, nothing adapter-specific needs a native
//! implementation.

use std::io::Cursor;
use std::path::{Path as StdPath, PathBuf};
use std::sync::Arc;

use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use extractor_core::AdapterListing;
use js_host::{AdapterPackage, JsHost};
use serde_json::json;

use super::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/adapters", get(list))
        .route("/api/adapters/upload", post(upload))
        .route("/api/adapters/{id}", delete(uninstall))
}

/// Scans `adapters_dir` and loads every `<id>/{manifest.json,<entry>}` pair
/// found. Called once at server startup; `installed` is kept in memory and
/// updated in place by install/uninstall after that.
pub fn load_installed_from_disk(host: &JsHost, adapters_dir: &StdPath) -> Vec<AdapterPackage> {
    let Ok(entries) = std::fs::read_dir(adapters_dir) else {
        return Vec::new();
    };

    let mut packages = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        match load_one(host, &dir) {
            Ok(pkg) => packages.push(pkg),
            Err(e) => eprintln!("skipping adapter at {}: {e}", dir.display()),
        }
    }
    packages
}

fn load_one(host: &JsHost, dir: &StdPath) -> anyhow::Result<AdapterPackage> {
    let manifest_json = std::fs::read(dir.join("manifest.json"))?;
    let manifest: extractor_core::AdapterManifest = serde_json::from_slice(&manifest_json)?;
    let js_bytes = std::fs::read(dir.join(&manifest.entry))?;
    host.load_package(&manifest_json, &js_bytes)
}

async fn list(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let installed = state.installed.read().await;
    let adapters: Vec<AdapterListing> = installed.iter().map(|p| (&p.manifest).into()).collect();
    Json(json!({ "adapters": adapters }))
}

async fn upload(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut zip_bytes: Option<Vec<u8>> = None;

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => return err(StatusCode::BAD_REQUEST, format!("Invalid upload: {e}")),
        };
        if field.name() == Some("package") {
            match field.bytes().await {
                Ok(bytes) => zip_bytes = Some(bytes.to_vec()),
                Err(e) => return err(StatusCode::BAD_REQUEST, format!("Failed to read upload: {e}")),
            }
        }
    }

    let Some(zip_bytes) = zip_bytes else {
        return err(StatusCode::BAD_REQUEST, "Missing adapter package (.zip)".into());
    };
    if zip_bytes.is_empty() {
        return err(StatusCode::BAD_REQUEST, "Uploaded file is empty".into());
    }

    match install_from_zip(&state, &zip_bytes).await {
        Ok(listing) => {
            (StatusCode::OK, Json(json!({ "ok": true, "adapter": listing }))).into_response()
        }
        Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

async fn install_from_zip(state: &AppState, zip_bytes: &[u8]) -> anyhow::Result<AdapterListing> {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes))?;

    let mut manifest_json: Option<Vec<u8>> = None;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.name().ends_with("manifest.json") {
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut buf)?;
            manifest_json = Some(buf);
            break;
        }
    }
    let manifest_json =
        manifest_json.ok_or_else(|| anyhow::anyhow!("Zip must contain manifest.json"))?;
    let manifest: extractor_core::AdapterManifest = serde_json::from_slice(&manifest_json)?;

    let mut js_bytes: Option<Vec<u8>> = None;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.name() == manifest.entry || file.name().ends_with(&format!("/{}", manifest.entry)) {
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut buf)?;
            js_bytes = Some(buf);
            break;
        }
    }
    let js_bytes = js_bytes
        .ok_or_else(|| anyhow::anyhow!("Entry module \"{}\" not found in package", manifest.entry))?;

    // Validate it actually loads before committing anything to disk, a
    // broken package must never get left behind reporting "installed".
    let package = state.js_host.load_package(&manifest_json, &js_bytes)?;

    let dest: PathBuf = state.adapters_dir.join(&manifest.id);
    std::fs::create_dir_all(&dest)?;
    std::fs::write(dest.join("manifest.json"), &manifest_json)?;
    std::fs::write(dest.join(&manifest.entry), &js_bytes)?;

    let listing: AdapterListing = (&package.manifest).into();

    let mut installed = state.installed.write().await;
    installed.retain(|p| p.manifest.id != manifest.id);
    installed.push(package);

    Ok(listing)
}

async fn uninstall(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> impl IntoResponse {
    let dest = state.adapters_dir.join(&id);
    if let Err(e) = std::fs::remove_dir_all(&dest) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return err(StatusCode::BAD_REQUEST, format!("Remove failed: {e}"));
        }
    }

    let mut installed = state.installed.write().await;
    installed.retain(|p| p.manifest.id != id);

    (StatusCode::OK, Json(json!({ "ok": true }))).into_response()
}

fn err(status: StatusCode, message: String) -> axum::response::Response {
    (status, Json(json!({ "error": message }))).into_response()
}
