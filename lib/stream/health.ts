import type { Stream } from "stream-resolver"

/**
 * Learned playback health.
 *
 * Sources are volatile: a link that plays today 404s tomorrow, a provider's
 * "orion" mirror works while its siblings don't, and a CDN that rate-limited
 * you an hour ago may be fine now. Rather than encoding which provider is
 * currently good, that changes constantly and hardcoding it would rot, each
 * stream identity is scored from observed outcomes and the ordering follows
 * the evidence.
 */

const STORAGE_KEY = "streamflow.streamHealth"
const PREFERRED_KEY = "streamflow.preferredSource"

/** After this, a failure stops counting, so a source gets another chance. */
const FAILURE_TTL_MS = 30 * 60 * 1000
const MAX_ENTRIES = 200

type HealthEntry = {
  ok: number
  fail: number
  lastOkAt?: number
  lastFailAt?: number
}

type HealthMap = Record<string, HealthEntry>

function readHealth(): HealthMap {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return {}
    const parsed: unknown = JSON.parse(raw)
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {}
    return parsed as HealthMap
  } catch {
    return {}
  }
}

function writeHealth(map: HealthMap): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(map))
  } catch {
    // Storage disabled, ordering simply stays unlearned.
  }
}

function hostOf(url: string): string {
  try {
    return new URL(url).host
  } catch {
    return ""
  }
}

/**
 * Stable identity for a candidate. Always computed from the raw CDN stream,
 * once a stream is picked its `url` becomes an opaque companion session URL,
 * which would make every play look like a brand new candidate.
 */
export function streamKey(stream: Stream): string {
  const debug = stream.headers?.xDebug
  const label = typeof debug === "string" && debug ? debug : (stream.label ?? "")
  return [
    stream.source?.extractor ?? "?",
    hostOf(stream.url),
    stream.quality ?? stream.type ?? "",
    label,
  ].join("|")
}

function lastTouched(entry: HealthEntry): number {
  return Math.max(entry.lastOkAt ?? 0, entry.lastFailAt ?? 0)
}

function prune(map: HealthMap): HealthMap {
  const keys = Object.keys(map)
  if (keys.length <= MAX_ENTRIES) return map

  const kept: HealthMap = {}
  const ordered = keys.sort((a, b) => lastTouched(map[b]!) - lastTouched(map[a]!))
  for (const key of ordered.slice(0, MAX_ENTRIES)) {
    const entry = map[key]
    if (entry) kept[key] = entry
  }
  return kept
}

/** Recent failures count against a stream; older ones are forgiven. */
function recentFails(entry: HealthEntry, now: number): number {
  if (!entry.lastFailAt || now - entry.lastFailAt > FAILURE_TTL_MS) return 0
  return entry.fail
}

export function streamScore(stream: Stream, now = Date.now()): number {
  const entry = readHealth()[streamKey(stream)]
  if (!entry) return 0
  return entry.ok - recentFails(entry, now)
}

/** Known-good first, then unknown, then recently-failed. Ties keep source order. */
export function sortStreamsByHealth(streams: Stream[]): Stream[] {
  const now = Date.now()
  return [...streams].sort((a, b) => streamScore(b, now) - streamScore(a, now))
}

function record(stream: Stream, outcome: "ok" | "fail"): void {
  const key = streamKey(stream)
  const map = readHealth()
  const entry = map[key] ?? { ok: 0, fail: 0 }

  if (outcome === "ok") {
    entry.ok += 1
    entry.lastOkAt = Date.now()
  } else {
    entry.fail += 1
    entry.lastFailAt = Date.now()
  }

  map[key] = entry
  writeHealth(prune(map))
}

export function recordStreamSuccess(stream: Stream): void {
  record(stream, "ok")
}

export function recordStreamFailure(stream: Stream): void {
  record(stream, "fail")
}

/**
 * Which extractor last played this title, so coming back to a movie starts on
 * the source that worked rather than whatever resolves first.
 */
export function rememberPreferredSource(titleKey: string, extractorId: string): void {
  try {
    localStorage.setItem(`${PREFERRED_KEY}.${titleKey}`, extractorId)
  } catch {
    // ignore
  }
}

export function preferredSource(titleKey: string): string | null {
  try {
    return localStorage.getItem(`${PREFERRED_KEY}.${titleKey}`)
  } catch {
    return null
  }
}
