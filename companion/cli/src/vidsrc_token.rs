//! Vidsrc2 CDN JWTs: manifests need a fresh token; segments reuse whatever
//! token the playlist already embedded in the URL.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use regex::Regex;

const VIDSRC2_TOKEN_URL: &str = "https://horologyhollow.site/generate.php";
const VIDSRC2_REFERER: &str = "https://vidsrc2.ru/";

/// Clamps on how long any single token is reused. The JWT's own `exp` sets the
/// real lifetime (vidsrc mints short-lived, ~20–30 min tokens); the ceiling
/// only bounds the damage if a token claims to outlive what the CDN honors,
/// and the floor stops a nearly-expired token from thrashing the endpoint.
const TOKEN_MAX_TTL: Duration = Duration::from_secs(30 * 60);
const TOKEN_MIN_TTL: Duration = Duration::from_secs(30);
/// Re-mint this long before `exp`, so a fetch already in flight can't race it.
const TOKEN_EXPIRY_SKEW: Duration = Duration::from_secs(120);
/// Wait this long after a failed refresh before touching the endpoint again,
/// without it a bare 429 repeats on literally every subsequent request.
const TOKEN_FAILURE_BACKOFF: Duration = Duration::from_secs(60);
/// Ceiling for a server-supplied `Retry-After`, so one header can't park us.
const TOKEN_FAILURE_BACKOFF_MAX: Duration = Duration::from_secs(5 * 60);
const TOKEN_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

static VIDSRC_PL_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)/pl/").unwrap());
static VIDSRC_CONTENT_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)/content/").unwrap());
static VIDSRC_PAGE_SEGMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)page-\d+\.html").unwrap());

/// The cached token plus the two timestamps that decide whether we call the
/// token endpoint again. Keeping the last good token separate from the
/// refresh schedule is what lets a *failed* refresh still serve a token
/// instead of degrading to none.
struct TokenState {
    token: Option<String>,
    /// Reuse the token until this instant, derived from the JWT's `exp`.
    refresh_at: Option<Instant>,
    /// Don't call the token endpoint before this (set after a failure, so a
    /// 429 doesn't repeat on every request that follows it).
    retry_not_before: Option<Instant>,
}

static TOKEN_STATE: Mutex<TokenState> = Mutex::new(TokenState {
    token: None,
    refresh_at: None,
    retry_not_before: None,
});

/// Single-flight guard. Without it, concurrent playlist requests all miss the
/// cache together and stampede the rate-limited token endpoint, which is
/// exactly where the back-to-back 429s came from.
static TOKEN_FETCH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

static COOKIE_JAR: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// CDN hosts rotate; detect vidsrc-style paths on any host.
pub fn is_vidsrc_cdn_url(target_url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(target_url) else {
        return false;
    };
    VIDSRC_PL_PATH.is_match(url.path()) || VIDSRC_CONTENT_PATH.is_match(url.path())
}

/// Host suffixes where vidsrc serves `page-*.html` segment files.
fn is_vidsrc_page_host(host: &str) -> bool {
    let host = host.to_lowercase();
    [
        "peakstorm.top",
        "hiddenanchor.site",
        "cynosuredatacom.website",
        "vidsrc2.ru",
        "vidsrcme.ru",
        "data.vidsrcme.ru",
    ]
    .iter()
    .any(|marker| host == *marker || host.ends_with(&format!(".{marker}")))
}

pub fn is_vidsrc_segment_url(target_url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(target_url) else {
        return false;
    };
    if VIDSRC_CONTENT_PATH.is_match(url.path()) {
        return true;
    }
    // page-*.html is also used by other embed CDNs (e.g. Primeflix dolphin workers).
    if VIDSRC_PAGE_SEGMENT.is_match(url.path()) {
        return is_vidsrc_page_host(url.host_str().unwrap_or_default());
    }
    false
}

/// Returns a usable token, refreshing only when the cached one is due.
///
/// Three things stop this from hammering `generate.php`, which rate-limits:
/// 1. a single-flight lock, so concurrent requests wait for one fetch instead
///    of all racing it;
/// 2. a TTL taken from the JWT's own `exp`, so a 20–30 min token is reused for
///    its whole life rather than re-minted every 60s;
/// 3. a failure backoff that still serves the last good token, so a rate limit
///    degrades to a possibly-stale token rather than to no token at all.
pub async fn fetch_vidsrc_token(http: &reqwest::Client) -> anyhow::Result<String> {
    if let Some(token) = fresh_cached_token() {
        return Ok(token);
    }

    let _single_flight = TOKEN_FETCH_LOCK.lock().await;

    // Another request may have refreshed while we waited for the lock.
    if let Some(token) = fresh_cached_token() {
        return Ok(token);
    }

    if let Some(remaining) = backoff_remaining() {
        return match cached_token() {
            Some(token) => {
                eprintln!("[vidsrc] token refresh cooling down for {remaining:?}; reusing cached token");
                Ok(token)
            }
            None => Err(anyhow::anyhow!(
                "vidsrc token endpoint cooling down for {remaining:?} after a failure"
            )),
        };
    }

    match request_vidsrc_token(http).await {
        Ok(token) => {
            let ttl = token_ttl(&token);
            {
                let mut state = TOKEN_STATE.lock().unwrap();
                state.token = Some(token.clone());
                state.refresh_at = Some(Instant::now() + ttl);
                state.retry_not_before = None;
            }
            eprintln!("[vidsrc] acquired token, reusing for {ttl:?}");
            Ok(token)
        }
        Err(err) => {
            let backoff = err.retry_after.unwrap_or(TOKEN_FAILURE_BACKOFF);
            let stale = {
                let mut state = TOKEN_STATE.lock().unwrap();
                state.retry_not_before = Some(Instant::now() + backoff);
                state.token.clone()
            };
            match stale {
                Some(token) => {
                    eprintln!("[vidsrc] token refresh failed ({}); reusing cached token", err.message);
                    Ok(token)
                }
                None => {
                    eprintln!("[vidsrc] token fetch failed ({}); backing off {backoff:?}", err.message);
                    Err(anyhow::anyhow!("{}", err.message))
                }
            }
        }
    }
}

fn fresh_cached_token() -> Option<String> {
    let state = TOKEN_STATE.lock().unwrap();
    let token = state.token.clone()?;
    match state.refresh_at {
        Some(at) if Instant::now() < at => Some(token),
        _ => None,
    }
}

fn cached_token() -> Option<String> {
    TOKEN_STATE.lock().unwrap().token.clone()
}

fn backoff_remaining() -> Option<Duration> {
    let state = TOKEN_STATE.lock().unwrap();
    let until = state.retry_not_before?;
    let now = Instant::now();
    (now < until).then(|| until - now)
}

/// Why a refresh failed, plus when the endpoint asked us to come back.
struct TokenFetchError {
    message: String,
    retry_after: Option<Duration>,
}

async fn request_vidsrc_token(http: &reqwest::Client) -> Result<String, TokenFetchError> {
    let res = http
        .get(VIDSRC2_TOKEN_URL)
        .header("Referer", VIDSRC2_REFERER)
        .header("Accept", "text/plain, */*")
        .timeout(TOKEN_REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|e| TokenFetchError { message: format!("request failed: {e}"), retry_after: None })?;

    let status = res.status();
    if !status.is_success() {
        return Err(TokenFetchError { message: format!("HTTP {status}"), retry_after: retry_after(&res) });
    }

    let token = res
        .text()
        .await
        .map_err(|e| TokenFetchError { message: format!("failed reading body: {e}"), retry_after: None })?
        .trim()
        .to_string();

    if token.is_empty() {
        return Err(TokenFetchError { message: "empty token body".to_string(), retry_after: None });
    }

    Ok(token)
}

/// `Retry-After` in seconds, when the endpoint sends one (a 429 usually does),
/// clamped so a single header can't park the companion for hours.
fn retry_after(res: &reqwest::Response) -> Option<Duration> {
    let seconds: u64 = res.headers().get(reqwest::header::RETRY_AFTER)?.to_str().ok()?.trim().parse().ok()?;
    Some(Duration::from_secs(seconds).min(TOKEN_FAILURE_BACKOFF_MAX))
}

/// How long a token is safe to reuse: its own `exp` minus a skew so an
/// in-flight segment fetch can't race the expiry, clamped at both ends.
/// Opaque (non-JWT) tokens fall back to the ceiling.
fn token_ttl(token: &str) -> Duration {
    jwt_remaining(token)
        .map(|remaining| remaining.saturating_sub(TOKEN_EXPIRY_SKEW))
        .unwrap_or(TOKEN_MAX_TTL)
        .clamp(TOKEN_MIN_TTL, TOKEN_MAX_TTL)
}

/// `exp - now` for a JWT-shaped token; `None` if it isn't one or has no `exp`.
fn jwt_remaining(token: &str) -> Option<Duration> {
    let payload = token.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload))
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    let exp = claims.get("exp")?.as_u64()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(Duration::from_secs(exp.saturating_sub(now)))
}

/// A `?token=` already on the URL that is still safe to use, normally the one
/// the adapter minted during extraction. Opaque (non-JWT) tokens are trusted;
/// JWTs are only reused while comfortably clear of `exp`.
fn existing_usable_token(target_url: &str) -> Option<String> {
    let url = reqwest::Url::parse(target_url).ok()?;
    let token = url
        .query_pairs()
        .find(|(key, _)| key == "token")
        .map(|(_, value)| value.into_owned())?;

    if token.is_empty() {
        return None;
    }

    match jwt_remaining(&token) {
        Some(remaining) => (remaining > TOKEN_EXPIRY_SKEW).then_some(token),
        None => Some(token),
    }
}

pub fn stamp_vidsrc_token(target_url: &str, token: &str) -> anyhow::Result<String> {
    let mut url = reqwest::Url::parse(target_url)?;

    // Replace rather than append: a URL carrying two `token=` params is read
    // inconsistently by servers, and the stale one can win over the fresh one.
    let retained: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(key, _)| key != "token")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();

    {
        let mut query = url.query_pairs_mut();
        query.clear();
        for (key, value) in retained {
            query.append_pair(&key, &value);
        }
        query.append_pair("token", token);
    }

    Ok(url.to_string())
}

/// Resolves the token for one vidsrc request.
///
/// A URL that already carries a usable token is passed through untouched. The
/// adapter mints one during extraction; it's IP-bound and good for ~20–30 min,
/// and the token endpoint rate-limits hard, so re-minting per playlist request
/// is both unnecessary and self-defeating. Segments inherit the playlist's
/// token via `ensure_vidsrc_token_from_playlist`, so this is also what keeps an
/// entire playback session off the endpoint.
pub async fn prepare_vidsrc_target(http: &reqwest::Client, target_url: &str) -> String {
    if !is_vidsrc_cdn_url(target_url) {
        return target_url.to_string();
    }

    if existing_usable_token(target_url).is_some() {
        return target_url.to_string();
    }

    match fetch_vidsrc_token(http).await {
        Ok(token) => stamp_vidsrc_token(target_url, &token).unwrap_or_else(|_| target_url.to_string()),
        Err(error) => {
            eprintln!("[vidsrc] no token for this request ({error}); the CDN will likely 401");
            target_url.to_string()
        }
    }
}

/// Copy `?token=` from the playlist URL onto segment URLs missing it.
pub fn ensure_vidsrc_token_from_playlist(media_url: &str, playlist_url: &str) -> String {
    let Ok(mut media) = reqwest::Url::parse(media_url) else {
        return media_url.to_string();
    };
    let Ok(playlist) = reqwest::Url::parse(playlist_url) else {
        return media_url.to_string();
    };

    let playlist_token = playlist
        .query_pairs()
        .find(|(k, _)| k == "token")
        .map(|(_, v)| v.to_string());
    let has_token = media.query_pairs().any(|(k, _)| k == "token");

    if let Some(token) = playlist_token {
        if !has_token {
            media.query_pairs_mut().append_pair("token", &token);
            return media.to_string();
        }
    }

    media_url.to_string()
}

pub fn read_cookie_jar(host: &str) -> Option<String> {
    COOKIE_JAR.lock().unwrap().get(host).cloned()
}

pub fn store_cookies(host: &str, headers: &reqwest::header::HeaderMap) {
    let cookies: Vec<String> = headers
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .collect();
    if cookies.is_empty() {
        return;
    }

    let merged = cookies
        .iter()
        .filter_map(|c| c.split(';').next())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("; ");

    let mut jar = COOKIE_JAR.lock().unwrap();
    let entry = jar.entry(host.to_string()).or_default();
    *entry = if entry.is_empty() {
        merged
    } else {
        format!("{entry}; {merged}")
    };
}

pub fn merge_cookie_header(host: &str, headers: &mut HashMap<String, String>) {
    if let Some(cookie) = read_cookie_jar(host) {
        headers.insert("Cookie".to_string(), cookie);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_html_on_vidsrc_hosts_is_a_vidsrc_segment() {
        assert!(is_vidsrc_segment_url(
            "https://moon.peakstorm.top/pl/x/content/page-3.html?token=abc"
        ));
    }

    #[test]
    fn page_html_on_other_embed_cdns_is_not_vidsrc() {
        assert!(!is_vidsrc_segment_url(
            "https://cube.dolphin-d55.workers.dev/file1/abc/480p/page-0.html"
        ));
    }

    #[test]
    fn content_path_is_a_vidsrc_segment_on_any_host() {
        assert!(is_vidsrc_segment_url("https://cdn.example.com/content/seg.ts"));
    }

    /// The point of deriving the TTL from `exp`: a ~25 min token is reused for
    /// nearly its whole life, instead of being re-minted every 60s.
    #[test]
    fn token_ttl_follows_the_jwts_own_expiry() {
        let ttl = token_ttl(&jwt_expiring_in(25 * 60));

        assert!(ttl > Duration::from_secs(20 * 60), "got {ttl:?}");
        assert!(ttl <= Duration::from_secs(25 * 60), "got {ttl:?}");
    }

    #[test]
    fn token_ttl_clamps_a_long_lived_token_to_the_ceiling() {
        assert_eq!(token_ttl(&jwt_expiring_in(4 * 3600)), TOKEN_MAX_TTL);
    }

    #[test]
    fn token_ttl_clamps_an_expired_token_to_the_floor() {
        assert_eq!(token_ttl(&jwt_expiring_in(0)), TOKEN_MIN_TTL);
    }

    #[test]
    fn token_ttl_falls_back_to_the_ceiling_for_opaque_tokens() {
        assert_eq!(token_ttl("opaque-non-jwt-value"), TOKEN_MAX_TTL);
    }

    /// Regression: stamping used to `append_pair`, so a URL that already
    /// carried a token left the endpoint with two `token=` params.
    #[test]
    fn stamp_replaces_an_existing_token_instead_of_adding_a_second() {
        let stamped =
            stamp_vidsrc_token("https://cdn.example.com/pl/x/master.m3u8?token=stale", "fresh").unwrap();

        assert_eq!(stamped.matches("token=").count(), 1, "{stamped}");
        assert!(stamped.contains("token=fresh"), "{stamped}");
        assert!(!stamped.contains("stale"), "{stamped}");
    }

    #[test]
    fn stamp_preserves_other_query_params() {
        let stamped =
            stamp_vidsrc_token("https://cdn.example.com/pl/x/master.m3u8?quality=1080&token=stale", "fresh")
                .unwrap();

        assert!(stamped.contains("quality=1080"), "{stamped}");
        assert_eq!(stamped.matches("token=").count(), 1, "{stamped}");
    }

    /// The core of reuse: a URL the adapter already tokenised must not trigger
    /// a fresh mint, so a whole playback session never touches the
    /// rate-limited endpoint.
    #[test]
    fn existing_usable_token_reuses_a_live_jwt() {
        let url =
            format!("https://cdn.example.com/pl/x/master.m3u8?token={}", jwt_expiring_in(25 * 60));

        assert!(existing_usable_token(&url).is_some());
    }

    #[test]
    fn existing_usable_token_ignores_a_token_near_or_past_expiry() {
        let soon = format!("https://cdn.example.com/pl/x/master.m3u8?token={}", jwt_expiring_in(30));
        let expired = format!("https://cdn.example.com/pl/x/master.m3u8?token={}", jwt_expiring_in(0));

        assert!(existing_usable_token(&soon).is_none());
        assert!(existing_usable_token(&expired).is_none());
    }

    #[test]
    fn existing_usable_token_trusts_opaque_tokens() {
        let url = "https://cdn.example.com/pl/x/master.m3u8?token=opaque-value";

        assert_eq!(existing_usable_token(url).as_deref(), Some("opaque-value"));
    }

    #[test]
    fn existing_usable_token_is_none_without_a_value() {
        assert!(existing_usable_token("https://cdn.example.com/pl/x/master.m3u8").is_none());
        assert!(existing_usable_token("https://cdn.example.com/pl/x/master.m3u8?token=").is_none());
    }

    fn jwt_expiring_in(seconds: u64) -> String {
        let exp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + seconds;
        let payload = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            format!(r#"{{"exp":{exp}}}"#).as_bytes(),
        );
        format!("eyJhbGciOiJIUzI1NiJ9.{payload}.sig")
    }
}
