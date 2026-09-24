/**
 * Playback resume points, keyed by title or episode.
 *
 * A position at the very start or the very end is not worth restoring: one
 * means the title was barely opened, the other means it finished. Both start
 * from the beginning next time.
 */

const STORAGE_KEY = "streamflow.resume"
const MIN_RESUMABLE_SECONDS = 15
const FINISHED_RATIO = 0.95
const MAX_ENTRIES = 100

type ResumeEntry = {
  seconds: number
  duration?: number
  updatedAt: number
}

type ResumeMap = Record<string, ResumeEntry>

function read(): ResumeMap {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return {}
    const parsed: unknown = JSON.parse(raw)
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {}
    return parsed as ResumeMap
  } catch {
    return {}
  }
}

function write(map: ResumeMap): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(map))
  } catch {
    // Storage disabled, resume points simply don't survive.
  }
}

function isWorthRestoring(seconds: number, duration?: number): boolean {
  if (!Number.isFinite(seconds) || seconds < MIN_RESUMABLE_SECONDS) return false
  if (duration && duration > 0 && seconds / duration > FINISHED_RATIO) return false
  return true
}

function prune(map: ResumeMap): ResumeMap {
  const keys = Object.keys(map)
  if (keys.length <= MAX_ENTRIES) return map

  const kept: ResumeMap = {}
  const ordered = keys.sort((a, b) => (map[b]?.updatedAt ?? 0) - (map[a]?.updatedAt ?? 0))
  for (const key of ordered.slice(0, MAX_ENTRIES)) {
    const entry = map[key]
    if (entry) kept[key] = entry
  }
  return kept
}

/** Seconds to resume from, or null when playback should start at the top. */
export function readResumePoint(titleKey: string): number | null {
  const entry = read()[titleKey]
  if (!entry) return null
  return isWorthRestoring(entry.seconds, entry.duration) ? entry.seconds : null
}

export function writeResumePoint(titleKey: string, seconds: number, duration?: number): void {
  const map = read()

  // Rewinding to the start, or reaching the end, clears the entry rather than
  // storing a position that the next read would immediately discard.
  if (!isWorthRestoring(seconds, duration)) {
    delete map[titleKey]
    write(map)
    return
  }

  map[titleKey] = { seconds, duration, updatedAt: Date.now() }
  write(prune(map))
}

export function clearResumePoint(titleKey: string): void {
  const map = read()
  delete map[titleKey]
  write(map)
}

/** `1:05:07` or `12:34`, for the resume notice. */
export function formatPlaybackTime(totalSeconds: number): string {
  const total = Math.max(0, Math.floor(totalSeconds))
  const hours = Math.floor(total / 3600)
  const minutes = Math.floor((total % 3600) / 60)
  const seconds = total % 60
  const pad = (value: number) => String(value).padStart(2, "0")
  return hours > 0 ? `${hours}:${pad(minutes)}:${pad(seconds)}` : `${minutes}:${pad(seconds)}`
}
