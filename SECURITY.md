# Security: Streamflow Companion

The companion (`companion/cli`) is a small local process. You only run it if you want adapters to extract and the player to proxy streams. The website itself does not scrape or stream, the browser talks to `127.0.0.1` for those two jobs so the host is not paying the bandwidth.

This note is about what that local process actually defends against, where that lives in the code, and what it does **not** try to cover. Everything below is based on the current companion / client code, not a wishlist.

## What we are worried about

The companion binds **`127.0.0.1` only** (`main.rs`). It is not reachable from your LAN or the public internet. Only something on the same machine (your browser, or another local process) can hit it.

So the realistic threat is: **another tab you have open tries to use your companion without you meaning to** (read the pairing token, install an adapter, kick off extraction, or turn `/api/stream` into a free fetch relay).

Malware already running as your OS user is out of scope. It can read `~/.streamflow/token` the same way it can read browser storage. No loopback server fixes that.

## Layer 1: CORS origin allowlist

**Where:** `companion/cli/src/main.rs` → `allowed_origins()` / `build_app()` (`CorsLayer`)

Browsers cannot forge `Origin`. The companion only allows listed origins to **read** responses. Defaults are local dev:

- `http://localhost:3000`
- `http://127.0.0.1:3000`

For a real hosted UI, set `COMPANION_ALLOWED_ORIGINS` (comma-separated) to your domain(s). The default is local-only on purpose.

That is why `GET /api/pair` and `GET /api/adapters` stay unauthenticated: they are read-only. The sensitive part is the response body, and CORS already stops a random site from reading it. See the comment on `pair` in `main.rs`.

## Layer 2: pairing token on every non-GET

**Where:** `require_token_for_mutations()` in `main.rs`

CORS has a blind spot. A plain HTML `<form>` POST still gets sent and acted on. The attacker does not need to read the response; it only needs the side effect (upload an adapter, run extract, register a stream session). That is real CSRF; CORS alone does not stop it. The middleware comment in `main.rs` spells this out.

On first run, `load_or_create_pairing_token()`:

- generates 32 random bytes (hex-encoded)
- writes them to `~/.streamflow/token` (or `$STREAMFLOW_DATA_DIR/token`)
- on Unix, sets mode `0600`

Every **non-GET** request must send that secret as `X-Companion-Token`. The check is layered on the whole router (including merged adapter/extract routes), so it is not limited to routes declared before `.layer()`.

The web app pairs once via `GET /api/pair`, caches the token in `localStorage` (`lib/api/companion.ts`), and retries once with a fresh pair if the companion returns 401 (e.g. after a data-dir reset).

Non-GET routes that exist today (all covered by this middleware):

- `POST /api/adapters/upload`
- `DELETE /api/adapters/{id}`
- `POST /api/extract`
- `POST /api/stream/register`

## Layer 2b: same token, as `?token=`, for stream GETs

**Where:** `stream_proxy::handler` and `stream_session::session_handler`

`/api/stream` and `/api/stream/s/{id}` are **GET**. Native `<video src>` / `<img src>` cannot set custom headers, so Layer 2’s header check does not apply.

Left open, that would be a real hole: any page could embed a media tag pointed at the companion with an arbitrary public `target=` URL (no JS, no CORS bypass needed) and use your machine as a relay. The comment at the top of `handler()` in `stream_proxy.rs` is exactly that.

Fix: require the same pairing token as `?token=` on those handlers, checked first.

HLS playlists are rewritten so every segment/key goes back through the proxy (`rewrite_hls_playlist` / `build_proxy_query`, and the session variants). Those generated URLs must carry `token=` too, otherwise the manifest loads and every segment 401s. The unit tests for rewrite assert the token is present for that reason.

## Layer 3: SSRF (separate from “who is asking”)

**Where:** `companion/core/src/ssrf.rs`

Layers 1–2b answer *who* can ask the companion to do something. This layer answers *what* outbound targets are allowed, including a fully authenticated call, and including adapter JS, which gets real HTTP on purpose (CDNs are arbitrary hosts).

Two pieces, both required (module docs in `ssrf.rs`):

1. **`SsrfSafeResolver`**, custom `reqwest` DNS resolver. Only public resolved addresses are used for the connection, so there is no “checked then re-resolved” window for DNS rebinding.
2. **`check_target_url()`**, blocks literal non-public IP hosts in the URL (`http://127.0.0.1/…`, `http://[::1]/`, etc.). Literal IPs never hit DNS, so the resolver alone would miss them. There is a unit test for literal private IPs, and an integration test that `/api/stream` refuses `http://127.0.0.1:…`.

Non-public v4/v6 ranges (private, loopback, link-local, and related) are rejected via `is_public_ip`.

Wired on the outbound paths that matter:

- adapter HTTP bridge in `js-host` (`do_http_request` / client built in `extract` with `SsrfSafeResolver`)
- stream proxy client in `build_state()` (same resolver), used by upstream fetch paths, including vidsrc token fetch via `state.http`

## Routes (as registered in `build_app`)

| Route | Method | Auth |
|---|---|---|
| `/api/health` | GET | none |
| `/api/pair` | GET | none (bootstrap; confidentiality is CORS) |
| `/api/adapters` | GET | none (read-only; confidentiality is CORS) |
| `/api/adapters/upload` | POST | `X-Companion-Token` |
| `/api/adapters/{id}` | DELETE | `X-Companion-Token` |
| `/api/extract` | POST | `X-Companion-Token` |
| `/api/stream/register` | POST | `X-Companion-Token` |
| `/api/stream` | GET | `?token=` |
| `/api/stream/s/{id}` | GET | `?token=` |

## What this does **not** cover

Straight from how the code behaves:

- **Installed adapter JS is semi-trusted.** Once it loads, it gets SSRF-filtered access to the public internet. That is the point of adapters, not an accident. Auth does not make the adapter’s logic “safe”, you are choosing to trust the zip, same idea as a browser extension or an npm package.
- **Adapter zips are not signed.** Upload accepts whatever `js_host::load_package` accepts (manifest parse, UTF-8 entry, bundle validation / web-only reject). There is no publisher signature check in the companion.
- **This repo does not code-sign the companion binary.** Trust is whatever you built or downloaded, same as any other local tool you compile yourself.
- **Packed adapters are obfuscated** (`stream-resolver/scripts/pack-adapter.mjs` via `javascript-obfuscator`). That makes casual eyeballing of installed `index.js` hard, for you and for the companion.
- **Same-user local processes** can read `~/.streamflow/token` (or `$STREAMFLOW_DATA_DIR/token`) and impersonate a paired client. Same class of problem as reading `localStorage`.

## Why these layers look the way they do

The comments in `main.rs`, `stream_proxy.rs`, and `ssrf.rs` are the short version:

1. CORS alone does not stop a `<form>` POST from mutating state → pairing token on non-GET.
2. Adapter / proxy HTTP without SSRF filtering could reach the LAN → `SsrfSafeResolver` + `check_target_url`.
3. Stream GETs cannot use the header, and an open proxy would be an open relay → `?token=` on `/api/stream` and session play URLs.
4. Playlist rewrite must thread that token, or segments 401 after a successful manifest.
5. Literal IP targets bypass DNS → `check_target_url` is not optional next to the resolver.

If you change the companion, keep these. They are not decorative.
