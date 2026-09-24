/**
 * The Streamflow Companion, a small background process users run locally,
 * only if they want to stream from third-party adapters (see
 * `companion/cli`). It exposes `/api/adapters*`, `/api/extract`,
 * and `/api/stream` on `127.0.0.1`, so the *browser* (not the hosted
 * server) makes those calls, keeping video bandwidth off the host.
 */
export const COMPANION_BASE =
  process.env.NEXT_PUBLIC_COMPANION_URL ?? "http://127.0.0.1:4310"

export function companionUrl(path: string): string {
  const normalized = path.startsWith("/") ? path : `/${path}`
  return `${COMPANION_BASE}${normalized}`
}

const TOKEN_STORAGE_KEY = "streamflow.companionToken"

function readCachedToken(): string | null {
  try {
    return localStorage.getItem(TOKEN_STORAGE_KEY)
  } catch {
    return null
  }
}

function writeCachedToken(token: string): void {
  try {
    localStorage.setItem(TOKEN_STORAGE_KEY, token)
  } catch {
    // Private browsing / storage disabled, just re-pair every call.
  }
}

/**
 * Pairs with the companion, fetching its per-install secret from `/api/pair`.
 * That endpoint is intentionally unauthenticated (see the comment on `pair`
 * in `companion/cli/src/main.rs`), CORS already stops a cross-origin page
 * from reading its response, which is the only thing that needs protecting
 * here. Caches the result so this only round-trips once per browser.
 */
async function fetchPairingToken(): Promise<string | null> {
  try {
    const res = await fetch(companionUrl("/api/pair"), {
      cache: "no-store",
      signal: AbortSignal.timeout(1500),
    })
    if (!res.ok) return null
    const data = (await res.json()) as { token?: string }
    if (!data.token) return null
    writeCachedToken(data.token)
    return data.token
  } catch {
    return null
  }
}

/**
 * Also used directly by `lib/stream/client.ts`: `/api/stream` is loaded by
 * a native `<video src>`/`<img src>`, which can't send the
 * `X-Companion-Token` header, so that endpoint takes the token as a query
 * param instead, this is what supplies it.
 */
export async function getCompanionToken(): Promise<string | null> {
  return readCachedToken() ?? fetchPairingToken()
}

function throwIfAborted(signal: AbortSignal | null | undefined): void {
  if (signal?.aborted) {
    throw signal.reason ?? new DOMException("Aborted", "AbortError")
  }
}

/**
 * Fetch against the companion, attaching `X-Companion-Token` for non-GET
 * routes (`require_token_for_mutations` in `companion/cli/src/main.rs`).
 * Retries once with a freshly paired token if the companion returns 401.
 */
export async function companionFetch(path: string, init: RequestInit = {}): Promise<Response> {
  const withToken = (token: string | null): RequestInit => ({
    ...init,
    headers: { ...(init.headers ?? {}), ...(token ? { "X-Companion-Token": token } : {}) },
  })

  const token = await getCompanionToken()
  throwIfAborted(init.signal)
  const res = await fetch(companionUrl(path), withToken(token))
  if (res.status !== 401) return res

  // Stale pairing token: re-pair once. Pointless if the caller has already
  // given up (a superseded extraction, an unmounted panel), so bail instead.
  throwIfAborted(init.signal)
  const freshToken = await fetchPairingToken()
  throwIfAborted(init.signal)
  return fetch(companionUrl(path), withToken(freshToken))
}
