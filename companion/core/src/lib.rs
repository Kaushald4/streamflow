//! Types mirroring `extractor/src/core/types.ts` in the sibling TS package
//! (used by the web build). This crate does not depend on, import, or
//! modify that package, it is an independent reimplementation for the
//! companion's extraction path. Field shapes are kept identical to the TS
//! JSON so the existing frontend (which already types against the TS
//! `ExtractionResult`/`Stream`) needs no changes to consume responses
//! produced here.

pub mod ssrf;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Movie,
    Series,
    Episode,
    ShortDrama,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamType {
    Mp4,
    Hls,
    Dash,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub movie: bool,
    pub series: bool,
    pub episodes: bool,
    pub short_drama: bool,
    pub subtitles: bool,
    pub multiple_qualities: bool,
    pub direct_streams: bool,
}

/// TS models `tmdbId` as `string | number`, mirror that with an untagged enum
/// so either JSON shape round-trips without the frontend needing to change.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrNumber {
    String(String),
    Number(i64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ExtractionInput {
    Movie {
        #[serde(skip_serializing_if = "Option::is_none")]
        imdb_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tmdb_id: Option<StringOrNumber>,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        year: Option<u32>,
    },
    Episode {
        #[serde(skip_serializing_if = "Option::is_none")]
        imdb_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tmdb_id: Option<StringOrNumber>,
        season: u32,
        episode: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    Series {
        #[serde(skip_serializing_if = "Option::is_none")]
        imdb_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tmdb_id: Option<StringOrNumber>,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    #[serde(rename = "short_drama")]
    ShortDrama {
        #[serde(skip_serializing_if = "Option::is_none")]
        show_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        episode_id: Option<StringOrNumber>,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamHeaders {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub referer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_referer: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omit_origin: Option<bool>,
    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleTrack {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSource {
    pub extractor: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(rename = "type")]
    pub stream_type: StreamType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<StreamHeaders>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitles: Option<Vec<SubtitleTrack>>,
    pub source: StreamSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePage {
    pub extractor_id: String,
    pub url: String,
    pub kind: ContentKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionFailure {
    pub extractor_id: String,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionResult {
    pub input: ExtractionInput,
    pub streams: Vec<Stream>,
    pub sources: Vec<SourcePage>,
    pub errors: Vec<ExtractionFailure>,
    pub duration_ms: u64,
}

/// Mirrors `ExternalAdapterManifest` from `extractor/src/adapters/manifest.ts`
/// field-for-field. The companion's adapter packages are the *same* JS+manifest zip
/// format web uses (see `js-host`), the only native piece is the shared
/// `primitives/wasm-decrypt` primitive, resolved at runtime, not a
/// per-adapter compile step, so the existing frontend (`StreamPlugin` in
/// `lib/plugins/catalog.ts`, the `/plugins` registry UI) needs no changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Filename of the JS entry module inside the package, e.g. "index.js".
    pub entry: String,
    pub capabilities: Capabilities,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decryption: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_binding: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter_class: Option<String>,
}

/// The JSON shape the frontend's `StreamPlugin` type (`lib/plugins/catalog.ts`)
/// expects from `/api/adapters`, manifest fields plus computed defaults and
/// `source`. The companion always reports `"external"` since every adapter
/// it runs is a locally installed package, not a hardcoded builtin.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterListing {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub description: String,
    pub version: String,
    pub author: String,
    pub status: String,
    pub adapter_class: String,
    pub transport: String,
    pub decryption: String,
    pub metadata_binding: String,
    pub capabilities: Capabilities,
    pub accent: String,
    pub id_hint: String,
    pub source: &'static str,
}

impl From<&AdapterManifest> for AdapterListing {
    fn from(m: &AdapterManifest) -> Self {
        Self {
            id: m.id.clone(),
            name: m.name.clone(),
            summary: m
                .summary
                .clone()
                .unwrap_or_else(|| "User-installed extraction adapter".to_string()),
            description: m.description.clone().unwrap_or_else(|| {
                "Standalone adapter loaded from a local package at runtime.".to_string()
            }),
            version: m.version.clone(),
            author: m.author.clone().unwrap_or_else(|| "External".to_string()),
            status: m.status.clone().unwrap_or_else(|| "experimental".to_string()),
            adapter_class: m.adapter_class.clone().unwrap_or_else(|| "site-adapter".to_string()),
            transport: m.transport.clone().unwrap_or_else(|| "HLS".to_string()),
            decryption: m.decryption.clone().unwrap_or_else(|| "Adapter-defined".to_string()),
            metadata_binding: m
                .metadata_binding
                .clone()
                .unwrap_or_else(|| "See adapter manifest".to_string()),
            capabilities: m.capabilities,
            accent: m.accent.clone().unwrap_or_else(|| "bg-amber-500".to_string()),
            id_hint: m.id_hint.clone().unwrap_or_else(|| "either".to_string()),
            source: "external",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraction_input_round_trips_tmdb_id_as_string_or_number() {
        let json = r#"{"kind":"movie","tmdbId":603,"imdbId":"tt0133093"}"#;
        let input: ExtractionInput = serde_json::from_str(json).unwrap();
        match input {
            ExtractionInput::Movie { tmdb_id, imdb_id, .. } => {
                assert!(matches!(tmdb_id, Some(StringOrNumber::Number(603))));
                assert_eq!(imdb_id.as_deref(), Some("tt0133093"));
            }
            _ => panic!("expected movie input"),
        }
    }

    #[test]
    fn stream_headers_deserialize_site_referer_boolean() {
        let json = r#"{
            "url": "https://cdn.example.com/master.m3u8",
            "type": "hls",
            "headers": { "referer": "https://gemma.example/", "siteReferer": true },
            "source": { "extractor": "federated-scraper", "version": "1.0.0" }
        }"#;
        let stream: Stream = serde_json::from_str(json).unwrap();
        assert_eq!(stream.headers.as_ref().unwrap().referer.as_deref(), Some("https://gemma.example/"));
        assert_eq!(stream.headers.as_ref().unwrap().site_referer, Some(true));
    }

    #[test]
    fn extraction_result_serializes_camel_case() {
        let result = ExtractionResult {
            input: ExtractionInput::Movie {
                imdb_id: None,
                tmdb_id: Some(StringOrNumber::Number(1)),
                title: None,
                year: None,
            },
            streams: vec![],
            sources: vec![],
            errors: vec![],
            duration_ms: 42,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"durationMs\":42"));
        assert!(json.contains("\"tmdbId\":1"));
    }
}
