/**
 * Browser helpers for companion stream sessions.
 *
 * Playback uses opaque URLs (`/api/stream/s/{id}?token=…`). Headers are stored
 * on the companion via `POST /api/stream/register`, not in the query string.
 */

import { COMPANION_BASE } from "@/lib/api/companion"

export type StreamProxyHeaders = {
  referer?: string
  origin?: string
  userAgent?: string
  /** Parent playlist URL, used as Referer for HLS segments. */
  parent?: string
  /** When true, do not send Origin (some CDNs 403 on cross-origin). */
  omitOrigin?: boolean
  /** Keep extractor referer on HLS segments (embed / federated scraper CDNs). */
  siteReferer?: boolean
}

const WRAPPER_PARAMS = ["url", "uri", "src", "file", "link", "v", "stream"] as const

/**
 * Query keys that are proxy/player metadata, never part of the upstream CDN
 * URL.
 *
 * `token` is deliberately *not* here. Adapters mint a per-CDN `?token=` during
 * extraction (vidsrc2's JWT, nxsha's auth params) and the companion needs it to
 * survive to playback: re-minting it per playlist request hits a rate-limited
 * endpoint (see `prepare_vidsrc_target`), and it's a per-session bearer value
 * the extractor deliberately keeps. Our own pairing token is still dropped,
 * `cleanStreamTargetUrl` below handles that case by URL origin.
 */
const STRIP_QUERY_KEYS = new Set([
  "referer",
  "ref",
  "referer_url",
  "origin",
  "siteReferer",
  "site_referer",
  "omitOrigin",
  "omit_origin",
  "parent",
  "rand",
])

/** Origin of our own companion, or `null` if the base URL isn't parseable. */
const COMPANION_ORIGIN = (() => {
  try {
    return new URL(COMPANION_BASE).origin
  } catch {
    return null
  }
})()

function safeDecode(value: string): string {
  try {
    return decodeURIComponent(value)
  } catch {
    return value
  }
}

function unwrapProxyTarget(input: string): string {
  let current = input.trim()
  for (let depth = 0; depth < 5; depth++) {
    let parsed: URL
    try {
      parsed = new URL(current)
    } catch {
      break
    }
    let nested: string | null = null
    for (const key of WRAPPER_PARAMS) {
      const value = parsed.searchParams.get(key)
      if (!value) continue
      const decoded = safeDecode(value)
      if (decoded.startsWith("http://") || decoded.startsWith("https://")) {
        nested = decoded
        break
      }
    }
    if (!nested) break
    current = nested
  }
  return current
}

/**
 * Strip proxy/player query noise before handing a URL to companion `target`.
 *
 * A CDN's own `?token=` is preserved (see `STRIP_QUERY_KEYS`). It is only
 * dropped when the URL is one of our own companion URLs, where `token` is the
 * pairing secret, that must never be forwarded to a third-party CDN.
 */
export function cleanStreamTargetUrl(raw: string): string {
  let url = unwrapProxyTarget(raw)
  try {
    const parsed = new URL(url)
    const isCompanionUrl = COMPANION_ORIGIN != null && parsed.origin === COMPANION_ORIGIN
    for (const key of [...parsed.searchParams.keys()]) {
      const lowered = key.toLowerCase()
      if (
        STRIP_QUERY_KEYS.has(key) ||
        STRIP_QUERY_KEYS.has(lowered) ||
        (isCompanionUrl && lowered === "token")
      ) {
        parsed.searchParams.delete(key)
      }
    }
    url = parsed.toString()
  } catch {
    // keep unwrapped raw
  }
  return url
}

export type RegisterStreamBody = {
  target: string
  referer?: string
  origin?: string
  siteReferer?: boolean
  omitOrigin?: boolean
}

/** Opaque play URL, no CDN URL in query string (PlayerJS-safe). */
export function buildSessionPlayPath(sessionId: string, token: string): string {
  return `/api/stream/s/${encodeURIComponent(sessionId)}?token=${encodeURIComponent(token)}`
}
