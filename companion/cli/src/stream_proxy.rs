//! Same-origin stream proxy for CDN segments/manifests. The browser can't
//! send the Referer/Origin/cookies these CDNs expect on a direct request, so
//! the player hits this companion endpoint instead.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use axum::{
    body::Body,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use regex::Regex;
use reqwest::Url;
use serde::Deserialize;
use serde_json::json;

use super::state::AppState;
use super::vidsrc_token::{
    ensure_vidsrc_token_from_playlist, is_vidsrc_cdn_url, is_vidsrc_segment_url,
    merge_cookie_header, prepare_vidsrc_target, store_cookies,
};

const DEFAULT_ALLOWED_HOSTS: &[&str] = &[
    "moon.peakstorm.top",
    "peakstorm.top",
    "hiddenanchor.site",
    "cynosuredatacom.website",
    "vidsrc2.ru",
    "vidsrcme.ru",
    "data.vidsrcme.ru",
];

pub const DEFAULT_UA: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

static MEDIA_FILE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\.(m3u8|mp4|ts|m4s|aac|vtt|webvtt|srt|cmfv|cmfa|m4v|jpg|jpeg|key)(\?|$)")
        .unwrap()
});
static MEDIA_VD_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)/vd/").unwrap());
static CINEBY_R2_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)/r2/").unwrap());
static VIDSRC_PL_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)/pl/").unwrap());
static VIDSRC_CONTENT_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)/content/").unwrap());
static VIDSRC_PAGE_SEGMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)page-\d+\.html").unwrap());

fn get_allowed_hosts() -> Vec<String> {
    let mut hosts: Vec<String> = DEFAULT_ALLOWED_HOSTS.iter().map(|s| s.to_string()).collect();
    if let Ok(extra) = std::env::var("STREAM_PROXY_ALLOWLIST") {
        hosts.extend(extra.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    }
    hosts
}

fn looks_like_media_asset(target_url: &str) -> bool {
    let Ok(url) = Url::parse(target_url) else { return false };
    let path_query = format!("{}{}", url.path(), url.query().map(|q| format!("?{q}")).unwrap_or_default())
        .to_lowercase();

    MEDIA_FILE_PATTERN.is_match(&path_query)
        || MEDIA_VD_PATH.is_match(url.path())
        || CINEBY_R2_PATH.is_match(url.path())
        || VIDSRC_PL_PATH.is_match(url.path())
        || VIDSRC_CONTENT_PATH.is_match(url.path())
        || VIDSRC_PAGE_SEGMENT.is_match(url.path())
}

/// Allow explicit hosts, or sibling CDN hosts when the request carries an
/// extractor referer (not an open proxy) and the URL looks like a stream
/// segment/manifest. HLS manifests often live on one host while
/// init/segments are served from another.
fn is_allowed_stream_host(hostname: &str, target_url: &str, has_referer: bool) -> bool {
    let host = hostname.to_lowercase();
    if get_allowed_hosts()
        .iter()
        .any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")))
    {
        return true;
    }
    has_referer && looks_like_media_asset(target_url)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StreamProxyHeaders {
    pub referer: Option<String>,
    pub origin: Option<String>,
    pub parent: Option<String>,
    pub omit_origin: bool,
    /// When true, keep the extractor-supplied referer for HLS segments instead
    /// of swapping to the parent playlist URL (needed for embed CDNs).
    pub site_referer: bool,
}

/// True when a segment/child URL is on a different host than its parent playlist
/// (common for Viduki TV: master on ngcorp.dad, segments on crimsomdream.site).
fn cross_cdn_segment(target_url: &str, parent_url: &str) -> bool {
    let host = |u: &str| {
        Url::parse(u)
            .ok()
            .and_then(|p| p.host_str().map(|h| h.to_lowercase()))
    };
    match (host(target_url), host(parent_url)) {
        (Some(a), Some(b)) => a != b,
        _ => false,
    }
}

/// Picks the Referer/Origin the upstream CDN expects, varies by host and
/// asset type; some CDNs 403 if Origin is present at all.
fn resolve_upstream_referer(target_url: &str, headers: &StreamProxyHeaders) -> (String, String) {
    if is_vidsrc_segment_url(target_url) {
        let site_referer = headers.referer.clone().unwrap_or_else(|| "https://vidsrc2.ru/".to_string());
        return (site_referer, String::new());
    }

    if let Some(parent) = &headers.parent {
        if !headers.site_referer {
            // Cross-CDN segments (typical Viduki TV) gate on the embed site referer,
            // not the parent playlist URL, same as viduki-decrypt.js DEFAULT_HEADERS.
            if cross_cdn_segment(target_url, parent) {
                if let Some(referer) = &headers.referer {
                    let origin = headers.origin.clone().unwrap_or_else(|| referer.clone());
                    return (referer.clone(), origin);
                }
            }
            if let Ok(parent_url) = Url::parse(parent) {
                return (parent.clone(), parent_url.origin().ascii_serialization());
            }
        }
    }

    if let Some(referer) = &headers.referer {
        let origin = headers.origin.clone().unwrap_or_else(|| {
            Url::parse(referer)
                .map(|u| u.origin().ascii_serialization())
                .unwrap_or_default()
        });
        return (referer.clone(), origin);
    }

    if let Ok(target) = Url::parse(target_url) {
        if VIDSRC_PL_PATH.is_match(target.path()) {
            let site_referer = headers.referer.clone().unwrap_or_else(|| "https://vidsrc2.ru/".to_string());
            let origin = headers.origin.clone().unwrap_or_else(|| {
                Url::parse(&site_referer)
                    .map(|u| u.origin().ascii_serialization())
                    .unwrap_or_default()
            });
            return (site_referer, origin);
        }
    }

    let referer = headers.referer.clone().unwrap_or_else(|| "https://cineby.at/".to_string());
    let origin = headers
        .origin
        .clone()
        .unwrap_or_else(|| Url::parse(&referer).map(|u| u.origin().ascii_serialization()).unwrap_or_default());
    (referer, origin)
}

fn upstream_headers(target_url: &str, params: &StreamProxyHeaders) -> HashMap<String, String> {
    let (referer, origin) = resolve_upstream_referer(target_url, params);

    let segment_like = Url::parse(target_url)
        .map(|target| {
            !is_m3u8_url(target_url, None)
                && (is_vidsrc_segment_url(target_url)
                    || CINEBY_R2_PATH.is_match(target.path())
                    || MEDIA_FILE_PATTERN.is_match(&format!(
                        "{}{}",
                        target.path(),
                        target.query().map(|q| format!("?{q}")).unwrap_or_default()
                    )))
        })
        .unwrap_or(false);

    let mut result = HashMap::new();
    result.insert("Referer".to_string(), referer);
    result.insert("User-Agent".to_string(), DEFAULT_UA.to_string());
    result.insert(
        "Accept".to_string(),
        if segment_like {
            "video/mp4,video/*,application/octet-stream,*/*".to_string()
        } else {
            "application/vnd.apple.mpegurl,application/x-mpegURL,video/*,*/*".to_string()
        },
    );

    let skip_origin = params.omit_origin || is_vidsrc_segment_url(target_url);

    if !skip_origin && !origin.is_empty() {
        result.insert("Origin".to_string(), origin);
    }

    result
}

pub fn is_m3u8_url(url: &str, content_type: Option<&str>) -> bool {
    if url.to_lowercase().contains(".m3u8") {
        return true;
    }
    let Some(ct) = content_type else { return false };
    let ct = ct.to_lowercase();
    ct.contains("mpegurl") || ct.contains("m3u8")
}

fn resolve_media_url(line: &str, base_url: &str) -> String {
    Url::parse(base_url)
        .and_then(|base| base.join(line.trim()))
        .map(|u| u.to_string())
        .unwrap_or_else(|_| line.trim().to_string())
}

fn build_proxy_query(target_url: &str, headers: &StreamProxyHeaders, parent_url: Option<&str>, token: &str) -> String {
    // Building via a placeholder base URL reuses Url's own percent-encoding
    // instead of pulling in a separate form-encoding dependency.
    let mut placeholder = Url::parse("http://proxy.local/").unwrap();
    {
        let mut qp = placeholder.query_pairs_mut();
        qp.append_pair("target", target_url);
        if let Some(r) = &headers.referer {
            qp.append_pair("referer", r);
        }
        if let Some(o) = &headers.origin {
            qp.append_pair("origin", o);
        }
        if let Some(p) = parent_url {
            qp.append_pair("parent", p);
        }
        if headers.site_referer {
            qp.append_pair("siteReferer", "1");
        }
        if headers.omit_origin {
            qp.append_pair("omitOrigin", "1");
        }
        // Every URL generated here is for another request back to this same
        // authenticated endpoint (a playlist's segments/keys), it needs the
        // same token the original request carried, or every segment fetch
        // 401s the moment the player follows one of these.
        qp.append_pair("token", token);
    }
    placeholder.query().unwrap_or_default().to_string()
}

fn proxy_url_for_target(
    target_url: &str,
    headers: &StreamProxyHeaders,
    parent_url: Option<&str>,
    token: &str,
    proxy_base: &str,
) -> String {
    format!(
        "{proxy_base}/api/stream?{}",
        build_proxy_query(target_url, headers, parent_url, token)
    )
}

fn session_url_for_target(
    session_id: &str,
    target_url: &str,
    parent_url: Option<&str>,
    token: &str,
    proxy_base: &str,
) -> String {
    let mut placeholder = Url::parse("http://proxy.local/").unwrap();
    {
        let mut qp = placeholder.query_pairs_mut();
        qp.append_pair("token", token);
        qp.append_pair("target", target_url);
        if let Some(p) = parent_url {
            qp.append_pair("parent", p);
        }
    }
    format!(
        "{proxy_base}/api/stream/s/{session_id}?{}",
        placeholder.query().unwrap_or_default()
    )
}

/// Rewrites HLS segment lines to session URLs, headers stay on the companion.
fn rewrite_hls_playlist_session(
    body: &str,
    playlist_url: &str,
    session_id: &str,
    token: &str,
    proxy_base: &str,
) -> String {
    body.lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return line.to_string();
            }

            if let Some(stripped) = trimmed.strip_prefix('#') {
                let _ = stripped;
                static URI_ATTR: LazyLock<Regex> =
                    LazyLock::new(|| Regex::new(r#"URI="([^"]+)""#).unwrap());
                return URI_ATTR
                    .replace_all(trimmed, |caps: &regex::Captures| {
                        let uri = &caps[1];
                        let absolute =
                            ensure_vidsrc_token_from_playlist(&resolve_media_url(uri, playlist_url), playlist_url);
                        format!(
                            r#"URI="{}""#,
                            session_url_for_target(session_id, &absolute, Some(playlist_url), token, proxy_base)
                        )
                    })
                    .to_string();
            }

            let absolute =
                ensure_vidsrc_token_from_playlist(&resolve_media_url(trimmed, playlist_url), playlist_url);
            session_url_for_target(session_id, &absolute, Some(playlist_url), token, proxy_base)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Rewrites an HLS playlist so every segment URL goes through this same
/// origin proxy instead of hitting the CDN directly from the browser.
/// URLs must be absolute, the player runs on the hosted site (e.g.
/// localhost:3000) while the companion listens on 127.0.0.1:4310, so a
/// relative `/api/stream?…` would resolve against the wrong host.
fn rewrite_hls_playlist(
    body: &str,
    playlist_url: &str,
    headers: &StreamProxyHeaders,
    token: &str,
    proxy_base: &str,
) -> String {
    body.lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return line.to_string();
            }

            if let Some(stripped) = trimmed.strip_prefix('#') {
                let _ = stripped;
                static URI_ATTR: LazyLock<Regex> =
                    LazyLock::new(|| Regex::new(r#"URI="([^"]+)""#).unwrap());
                return URI_ATTR
                    .replace_all(trimmed, |caps: &regex::Captures| {
                        let uri = &caps[1];
                        let absolute =
                            ensure_vidsrc_token_from_playlist(&resolve_media_url(uri, playlist_url), playlist_url);
                        format!(
                            r#"URI="{}""#,
                            proxy_url_for_target(&absolute, headers, Some(playlist_url), token, proxy_base)
                        )
                    })
                    .to_string();
            }

            let absolute =
                ensure_vidsrc_token_from_playlist(&resolve_media_url(trimmed, playlist_url), playlist_url);
            proxy_url_for_target(&absolute, headers, Some(playlist_url), token, proxy_base)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn proxy_base_from_request(req: &axum::http::Request<Body>) -> String {
    if let Ok(url) = std::env::var("COMPANION_PUBLIC_URL") {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            return trimmed.trim_end_matches('/').to_string();
        }
    }
    let host = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("127.0.0.1:4310");
    format!("http://{host}")
}

/// Remembers which Referer/Origin variant actually worked for a host, so the
/// many `Range` requests that make up one video don't re-probe the whole
/// variant list every time. Best-effort: a miss just falls back to probing.
static HOST_HEADER_PROFILES: LazyLock<Mutex<HashMap<String, StreamProxyHeaders>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn host_of(target_url: &str) -> Option<String> {
    Url::parse(target_url).ok()?.host_str().map(|host| host.to_lowercase())
}

fn remembered_profile(host: &str) -> Option<StreamProxyHeaders> {
    HOST_HEADER_PROFILES.lock().unwrap().get(host).cloned()
}

fn remember_profile(host: &str, headers: &StreamProxyHeaders) {
    // Only self-contained header sets are reusable, one carrying a `parent`
    // describes one specific segment and would be wrong for another asset.
    if headers.parent.is_some() {
        return;
    }
    HOST_HEADER_PROFILES.lock().unwrap().insert(host.to_string(), headers.clone());
}

/// The statuses a different Referer/Origin can plausibly fix. Everything else
/// (404/429/5xx) is the host telling us to go away, and cycling variants at it
/// only burns the request budget, for 429 it makes the limit worse.
fn is_header_retry_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 400 | 401 | 403)
}

fn is_rate_limited(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 429 | 503)
}

/// "Retry a few times, then skip", the same shape the source sites' own
/// players use. That's why a 429 here must not be fatal: there are usually
/// several other candidates to fall through to.
const RATE_LIMIT_ATTEMPTS: usize = 3;
/// Ceiling for a retry pause. Deliberately short, failing over to another
/// source is a much better use of the wait than stalling on one host.
const MAX_RETRY_DELAY_MS: u64 = 1_500;
const BASE_RETRY_DELAY_MS: u64 = 300;

/// Back-off before retrying a rate-limited/overloaded host: honour the
/// server's own `Retry-After` when it sends one, otherwise exponential backoff
/// with jitter so simultaneous streams don't retry in lockstep.
fn rate_limit_delay(res: &reqwest::Response, attempt: usize) -> Duration {
    if let Some(after) = retry_after(res) {
        return after.min(Duration::from_millis(MAX_RETRY_DELAY_MS));
    }
    let jitter = rand::random::<u64>() % 150;
    let delay = BASE_RETRY_DELAY_MS * (1 << attempt.min(3)) + jitter;
    Duration::from_millis(delay.min(MAX_RETRY_DELAY_MS))
}

/// `Retry-After` in seconds, when present (a 429 usually carries one).
fn retry_after(res: &reqwest::Response) -> Option<Duration> {
    let seconds: u64 = res
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(Duration::from_secs(seconds))
}

async fn fetch_upstream_once(
    http: &reqwest::Client,
    target: &str,
    headers: &StreamProxyHeaders,
    range: Option<&str>,
) -> anyhow::Result<reqwest::Response> {
    let host = Url::parse(target)?.host_str().unwrap_or_default().to_string();
    let mut base = upstream_headers(target, headers);
    merge_cookie_header(&host, &mut base);

    let mut req_headers = HeaderMap::new();
    for (k, v) in &base {
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            req_headers.insert(name, value);
        }
    }
    if let Some(r) = range {
        req_headers.insert(reqwest::header::RANGE, HeaderValue::from_str(r)?);
    }

    let res = http.get(target).headers(req_headers).send().await?;
    store_cookies(&host, res.headers());
    Ok(res)
}

async fn fetch_vidsrc_segment_minimal(
    http: &reqwest::Client,
    target: &str,
    referer: Option<&str>,
) -> anyhow::Result<reqwest::Response> {
    let host = Url::parse(target)?.host_str().unwrap_or_default().to_string();
    let mut headers: HashMap<String, String> = HashMap::new();
    headers.insert("User-Agent".to_string(), DEFAULT_UA.to_string());
    headers.insert("Accept".to_string(), "*/*".to_string());
    if let Some(r) = referer {
        headers.insert("Referer".to_string(), r.to_string());
    }
    merge_cookie_header(&host, &mut headers);

    let mut req_headers = HeaderMap::new();
    for (k, v) in &headers {
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            req_headers.insert(name, value);
        }
    }

    let res = http.get(target).headers(req_headers).send().await?;
    store_cookies(&host, res.headers());
    Ok(res)
}

fn cdn_site_referer(target: &str) -> Option<String> {
    Url::parse(target)
        .ok()
        .map(|u| format!("{}/", u.origin().ascii_serialization()))
}

async fn fetch_upstream(
    http: &reqwest::Client,
    target: &str,
    headers: &StreamProxyHeaders,
    range: Option<&str>,
) -> anyhow::Result<reqwest::Response> {
    if is_vidsrc_segment_url(target) {
        let site_referer = headers.referer.clone().unwrap_or_else(|| "https://vidsrc2.ru/".to_string());
        for referer in [Some(site_referer.as_str()), None] {
            let res = fetch_vidsrc_segment_minimal(http, target, referer).await?;
            if res.status().is_success() {
                return Ok(res);
            }
            if res.status() != reqwest::StatusCode::FORBIDDEN {
                return Ok(res);
            }
        }
    }

    let mut attempts: Vec<StreamProxyHeaders> = Vec::new();

    // 1. Extractor-configured headers (Referer + Origin when provided).
    attempts.push(headers.clone());

    // 2. Same referer without Origin (some CDNs reject cross-origin).
    if !headers.omit_origin {
        let mut no_origin = headers.clone();
        no_origin.omit_origin = true;
        attempts.push(no_origin);
    }

    // 3–4. CDN self-referer, with and without Origin.
    if let Some(cdn_ref) = cdn_site_referer(target) {
        if headers.referer.as_deref() != Some(cdn_ref.as_str()) {
            let cdn_origin = cdn_ref.trim_end_matches('/').to_string();
            attempts.push(StreamProxyHeaders {
                referer: Some(cdn_ref.clone()),
                origin: Some(cdn_origin.clone()),
                omit_origin: false,
                ..headers.clone()
            });
            attempts.push(StreamProxyHeaders {
                referer: Some(cdn_ref),
                omit_origin: true,
                ..headers.clone()
            });
        }
    }

    // 5. Site referer fallback when parent-based referer fails (cross-CDN edge cases).
    if headers.parent.is_some() && !headers.site_referer {
        if let Some(site_ref) = &headers.referer {
            attempts.push(StreamProxyHeaders {
                referer: Some(site_ref.clone()),
                origin: headers.origin.clone(),
                omit_origin: false,
                site_referer: true,
                parent: None,
                ..headers.clone()
            });
            attempts.push(StreamProxyHeaders {
                referer: Some(site_ref.clone()),
                origin: headers.origin.clone(),
                omit_origin: true,
                site_referer: true,
                parent: None,
                ..headers.clone()
            });
        }
    }

    // Try the header set that already worked for this host first, so the many
    // `Range` requests behind one video don't re-probe the whole variant list.
    let host = host_of(target);
    if let Some(host) = &host {
        if let Some(profile) = remembered_profile(host) {
            attempts.retain(|candidate| *candidate != profile);
            attempts.insert(0, profile);
        }
    }

    let mut last = None;
    for attempt in &attempts {
        let mut rate_limit_tries = 0usize;

        loop {
            let res = fetch_upstream_once(http, target, attempt, range).await?;
            let status = res.status();

            if status.is_success() {
                if let Some(host) = &host {
                    remember_profile(host, attempt);
                }
                return Ok(res);
            }

            // A rate-limited host gets a couple of quick retries, then we stop
            // and let the caller surface it, the UI moves to another source.
            if is_rate_limited(status) && rate_limit_tries + 1 < RATE_LIMIT_ATTEMPTS {
                let delay = rate_limit_delay(&res, rate_limit_tries);
                eprintln!(
                    "[stream] {status} from {}; retrying in {delay:?} ({}/{RATE_LIMIT_ATTEMPTS})",
                    host.as_deref().unwrap_or_default(),
                    rate_limit_tries + 1
                );
                drop(res);
                tokio::time::sleep(delay).await;
                rate_limit_tries += 1;
                continue;
            }

            last = Some(res);
            break;
        }

        // Only a header-shaped failure justifies trying another Referer/Origin.
        if !last.as_ref().map(|res| res.status()).is_some_and(is_header_retry_status) {
            break;
        }
    }

    if let Some(res) = last {
        return Ok(res);
    }

    if let Some(referer) = &headers.referer {
        let mut req_headers = HeaderMap::new();
        req_headers.insert("Referer", HeaderValue::from_str(referer)?);
        req_headers.insert("User-Agent", HeaderValue::from_static(DEFAULT_UA));
        req_headers.insert("Accept", HeaderValue::from_static("video/mp4,video/*,*/*"));
        if let Some(r) = range {
            req_headers.insert(reqwest::header::RANGE, HeaderValue::from_str(r)?);
        }
        return Ok(http.get(target).headers(req_headers).send().await?);
    }

    fetch_upstream_once(http, target, headers, range).await
}

#[derive(Deserialize)]
pub struct StreamQuery {
    /// Upstream media URL. Named `target` (not `url`) so PlayerJS does not
    /// unwrap `?url=https://…` proxy links and fetch the CDN directly.
    #[serde(alias = "url")]
    target: Option<String>,
    referer: Option<String>,
    origin: Option<String>,
    parent: Option<String>,
    site_referer: Option<String>,
    omit_origin: Option<String>,
    token: Option<String>,
}

pub async fn serve_media(
    state: &AppState,
    target: &str,
    proxy_headers: &StreamProxyHeaders,
    session_id: Option<&str>,
    req: axum::http::Request<Body>,
) -> Response {
    let Ok(parsed) = Url::parse(target) else {
        return err(StatusCode::BAD_REQUEST, "Invalid url");
    };
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return err(StatusCode::BAD_REQUEST, "Unsupported protocol");
    }
    if let Err(e) = extractor_core::ssrf::check_target_url(target) {
        return err(StatusCode::FORBIDDEN, &e);
    }

    let has_referer = proxy_headers.referer.is_some() || proxy_headers.parent.is_some();
    if !is_allowed_stream_host(parsed.host_str().unwrap_or_default(), target, has_referer) {
        return err(
            StatusCode::FORBIDDEN,
            &format!("Host not allowed: {}", parsed.host_str().unwrap_or_default()),
        );
    }

    let fetch_target = if is_vidsrc_cdn_url(target) {
        prepare_vidsrc_target(&state.http, target).await
    } else {
        target.to_string()
    };

    let range = req
        .headers()
        .get(axum::http::header::RANGE)
        .and_then(|v| v.to_str().ok());

    let upstream = match fetch_upstream(&state.http, &fetch_target, proxy_headers, range).await {
        Ok(res) => res,
        Err(e) => return err(StatusCode::BAD_GATEWAY, &format!("Upstream fetch failed: {e}")),
    };

    if !upstream.status().is_success() {
        let status = upstream.status();
        let host = parsed.host_str().unwrap_or_default();
        eprintln!("[stream] upstream {status} from {host}{}", parsed.path());
        // Pass the meaningful statuses through rather than masking everything
        // as 502: the player's own retry behaviour and the UI's source failover
        // both need to see "denied" / "rate limited", not a generic gateway error.
        let propagated = match status.as_u16() {
            401 | 403 | 404 | 429 => {
                StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY)
            }
            _ => StatusCode::BAD_GATEWAY,
        };
        return err(propagated, &format!("Upstream returned {status} from {host}"));
    }

    let content_type = upstream
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    if is_m3u8_url(&fetch_target, content_type.as_deref()) {
        let text = match upstream.text().await {
            Ok(t) => t,
            Err(e) => return err(StatusCode::BAD_GATEWAY, &format!("Failed to read playlist: {e}")),
        };
        let proxy_base = proxy_base_from_request(&req);
        let rewritten = if let Some(sid) = session_id {
            rewrite_hls_playlist_session(&text, &fetch_target, sid, &state.pairing_token, &proxy_base)
        } else {
            rewrite_hls_playlist(&text, &fetch_target, proxy_headers, &state.pairing_token, &proxy_base)
        };

        let mut res = Response::new(Body::from(rewritten));
        res.headers_mut().insert(
            "Content-Type",
            HeaderValue::from_static("application/vnd.apple.mpegurl"),
        );
        res.headers_mut().insert("Cache-Control", HeaderValue::from_static("no-store"));
        return res;
    }

    let lower_target = fetch_target.to_lowercase();
    let is_vtt = lower_target.contains(".vtt")
        || lower_target.contains(".webvtt")
        || content_type.as_deref().is_some_and(|ct| ct.to_lowercase().contains("vtt"));

    let mut response_headers = HeaderMap::new();
    if is_vtt {
        response_headers.insert("Content-Type", HeaderValue::from_static("text/vtt; charset=utf-8"));
    } else if let Some(ct) = &content_type {
        if let Ok(value) = HeaderValue::from_str(ct) {
            response_headers.insert("Content-Type", value);
        }
    }
    response_headers.insert("Cache-Control", HeaderValue::from_static("public, max-age=120"));
    for name in [
        reqwest::header::CONTENT_RANGE,
        reqwest::header::ACCEPT_RANGES,
        reqwest::header::CONTENT_LENGTH,
    ] {
        if let Some(v) = upstream.headers().get(&name) {
            response_headers.insert(name, v.clone());
        }
    }

    let status = StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::OK);
    let stream = upstream.bytes_stream();
    let mut res = Response::new(Body::from_stream(stream));
    *res.status_mut() = status;
    *res.headers_mut() = response_headers;
    res
}

pub async fn handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<StreamQuery>,
    req: axum::http::Request<Body>,
) -> Response {
    // This endpoint is loaded by a native <video src>/<img src>, which can't
    // send the X-Companion-Token header the other mutating/sensitive routes
    // require, so the token travels as a query param instead, exactly like
    // a signed URL. Without this, *any* page the user has open could embed
    // a tag pointing at this endpoint with an arbitrary public target URL,
    // no JavaScript or CORS bypass needed, just an HTML tag, turning the
    // companion into an open relay for whatever that page wants fetched.
    if q.token.as_deref() != Some(state.pairing_token.as_str()) {
        return err(StatusCode::UNAUTHORIZED, "Missing or invalid token");
    }

    let Some(target) = q.target else {
        return err(StatusCode::BAD_REQUEST, "Missing target parameter");
    };

    let proxy_headers = StreamProxyHeaders {
        referer: q.referer.clone(),
        origin: q.origin.clone(),
        parent: q.parent.clone(),
        site_referer: q.site_referer.as_deref() == Some("1"),
        omit_origin: q.omit_origin.as_deref() == Some("1"),
    };

    serve_media(&state, &target, &proxy_headers, None, req).await
}

pub fn err(status: StatusCode, message: &str) -> Response {
    (status, axum::Json(json!({ "error": message }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_exact_and_subdomain_matches_from_the_default_list() {
        assert!(is_allowed_stream_host("moon.peakstorm.top", "https://moon.peakstorm.top/x.m3u8", false));
        assert!(is_allowed_stream_host("sub.peakstorm.top", "https://sub.peakstorm.top/x.m3u8", false));
        assert!(!is_allowed_stream_host("peakstorm.top.evil.com", "https://peakstorm.top.evil.com/x", false));
    }

    #[test]
    fn allows_unlisted_hosts_only_with_a_referer_and_a_media_looking_path() {
        assert!(is_allowed_stream_host("evil.com", "https://evil.com/movie.m3u8", true));
        assert!(!is_allowed_stream_host("evil.com", "https://evil.com/page.html", true));
        assert!(!is_allowed_stream_host("evil.com", "https://evil.com/movie.m3u8", false));
    }

    #[test]
    fn looks_like_media_asset_matches_known_extensions_and_cdn_path_shapes() {
        assert!(looks_like_media_asset("https://x.com/a/b.m3u8"));
        assert!(looks_like_media_asset("https://x.com/a/b.mp4?token=1"));
        assert!(looks_like_media_asset("https://x.com/r2/segment"));
        assert!(looks_like_media_asset("https://x.com/vd/thing"));
        assert!(looks_like_media_asset("https://x.com/content/page-3.html"));
        assert!(!looks_like_media_asset("https://x.com/some/random/page"));
    }

    /// The variant list is only worth cycling for header-shaped failures;
    /// cycling it at a 429/404/500 just burns the request budget.
    #[test]
    fn only_header_shaped_failures_continue_the_variant_list() {
        assert!(is_header_retry_status(reqwest::StatusCode::BAD_REQUEST));
        assert!(is_header_retry_status(reqwest::StatusCode::UNAUTHORIZED));
        assert!(is_header_retry_status(reqwest::StatusCode::FORBIDDEN));

        assert!(!is_header_retry_status(reqwest::StatusCode::NOT_FOUND));
        assert!(!is_header_retry_status(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(!is_header_retry_status(reqwest::StatusCode::INTERNAL_SERVER_ERROR));
    }

    #[test]
    fn rate_limiting_is_recognised_as_retryable_but_denial_is_not() {
        assert!(is_rate_limited(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(is_rate_limited(reqwest::StatusCode::SERVICE_UNAVAILABLE));
        assert!(!is_rate_limited(reqwest::StatusCode::FORBIDDEN));
    }

    #[test]
    fn a_working_header_profile_is_remembered_per_host() {
        let headers = StreamProxyHeaders {
            referer: Some("https://web.nxsha.app/".to_string()),
            origin: Some("https://web.nxsha.app".to_string()),
            ..Default::default()
        };

        remember_profile("profile-reuse.example", &headers);

        assert_eq!(remembered_profile("profile-reuse.example"), Some(headers));
        assert_eq!(remembered_profile("profile-other.example"), None);
    }

    /// A profile tied to a specific `parent` playlist would be wrong for any
    /// other asset on the same host, so it must never be reused.
    #[test]
    fn a_profile_carrying_a_parent_is_not_reusable() {
        let headers = StreamProxyHeaders {
            parent: Some("https://cdn.example.com/x/index.m3u8".to_string()),
            ..Default::default()
        };

        remember_profile("profile-parent.example", &headers);

        assert_eq!(remembered_profile("profile-parent.example"), None);
    }

    #[test]
    fn resolve_upstream_referer_uses_site_referer_for_cross_cdn_viduki_tv_segments() {
        let headers = StreamProxyHeaders {
            referer: Some("https://www.viduki.net/".to_string()),
            origin: Some("https://www.viduki.net/".to_string()),
            parent: Some(
                "https://cdn2.ngcorp.dad/tv/o0Qs7gnfeJmfKGVx6pX9F9DsxdjAshCYrvFb4mwcW2k.OAZldagM/bigtits.m3u8"
                    .to_string(),
            ),
            ..Default::default()
        };
        let (referer, origin) = resolve_upstream_referer(
            "https://crimsomdream.site/BYmxvRbJWbvTP-segment",
            &headers,
        );
        assert_eq!(referer, "https://www.viduki.net/");
        assert_eq!(origin, "https://www.viduki.net/");
    }

    #[test]
    fn resolve_upstream_referer_prefers_parent_when_present() {
        let headers = StreamProxyHeaders {
            parent: Some("https://cineby.at/watch/1".to_string()),
            ..Default::default()
        };
        let (referer, origin) = resolve_upstream_referer("https://cdn.example.com/seg.ts", &headers);
        assert_eq!(referer, "https://cineby.at/watch/1");
        assert_eq!(origin, "https://cineby.at");
    }

    #[test]
    fn resolve_upstream_referer_omits_origin_for_vidsrc_segments() {
        let headers = StreamProxyHeaders::default();
        let (referer, origin) =
            resolve_upstream_referer("https://cdn.example.com/content/foo.ts", &headers);
        assert_eq!(referer, "https://vidsrc2.ru/");
        assert_eq!(origin, "");
    }

    #[test]
    fn resolve_upstream_referer_keeps_site_referer_over_parent_for_embed_cdns() {
        let headers = StreamProxyHeaders {
            referer: Some("https://gemma416okl.com/".to_string()),
            origin: Some("https://gemma416okl.com".to_string()),
            parent: Some("https://cdn.example.com/r2/index.m3u8".to_string()),
            site_referer: true,
            ..Default::default()
        };
        let (referer, origin) =
            resolve_upstream_referer("https://cdn.example.com/r2/segment1.ts", &headers);
        assert_eq!(referer, "https://gemma416okl.com/");
        assert_eq!(origin, "https://gemma416okl.com");
    }

    #[test]
    fn resolve_upstream_referer_uses_primeflix_site_headers_for_dolphin_page_segments() {
        let headers = StreamProxyHeaders {
            referer: Some("https://primeflix.ru/".to_string()),
            origin: Some("https://primeflix.ru/".to_string()),
            parent: Some(
                "https://cube.dolphin-d55.workers.dev/file1/abc/480p/playlist.m3u8".to_string(),
            ),
            site_referer: true,
            ..Default::default()
        };
        let (referer, origin) = resolve_upstream_referer(
            "https://cube.dolphin-d55.workers.dev/file1/abc/480p/page-0.html",
            &headers,
        );
        assert_eq!(referer, "https://primeflix.ru/");
        assert_eq!(origin, "https://primeflix.ru/");
    }

    #[test]
    fn resolve_upstream_referer_falls_back_to_cineby_default() {
        let headers = StreamProxyHeaders::default();
        let (referer, origin) = resolve_upstream_referer("https://cdn.example.com/r2/manifest.m3u8", &headers);
        assert_eq!(referer, "https://cineby.at/");
        assert_eq!(origin, "https://cineby.at");
    }

    const PROXY_BASE: &str = "http://127.0.0.1:4310";

    #[test]
    fn rewrite_hls_playlist_proxies_relative_segment_lines() {
        let body = "#EXTM3U\n#EXTINF:10,\nsegment1.ts\n";
        let headers = StreamProxyHeaders::default();
        let rewritten =
            rewrite_hls_playlist(body, "https://cdn.example.com/r2/index.m3u8", &headers, "tok123", PROXY_BASE);

        let proxied_line = rewritten.lines().last().unwrap();
        assert!(proxied_line.starts_with("http://127.0.0.1:4310/api/stream?"));
        assert!(proxied_line.contains("target=https%3A%2F%2Fcdn.example.com%2Fr2%2Fsegment1.ts"));
        assert!(proxied_line.contains("token=tok123"), "rewritten segment URL must carry the auth token: {proxied_line}");
    }

    #[test]
    fn rewrite_hls_playlist_proxies_uri_attributes_in_tags() {
        let body = "#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n";
        let headers = StreamProxyHeaders::default();
        let rewritten =
            rewrite_hls_playlist(body, "https://cdn.example.com/r2/index.m3u8", &headers, "tok123", PROXY_BASE);
        assert!(rewritten.contains("URI=\"http://127.0.0.1:4310/api/stream?target=https%3A%2F%2Fcdn.example.com%2Fr2%2Fkey.bin"));
        assert!(rewritten.contains("token=tok123"), "rewritten URI attribute must carry the auth token: {rewritten}");
    }

    #[test]
    fn is_m3u8_url_matches_extension_or_content_type() {
        assert!(is_m3u8_url("https://x.com/a.m3u8", None));
        assert!(is_m3u8_url("https://x.com/a", Some("application/x-mpegURL")));
        assert!(!is_m3u8_url("https://x.com/a.mp4", Some("video/mp4")));
    }
}
