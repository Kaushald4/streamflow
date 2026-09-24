"use client"

import * as React from "react"
import Script from "next/script"
import { Captions } from "lucide-react"
import { formatPlayerjsSubtitles } from "@/lib/playerjs/subtitles"
import { cn } from "@/lib/utils"
import type { PlayerjsInstance } from "@/types/playerjs"

type SubtitleTrack = {
  url: string
  language?: string
  label?: string
}

export type StreamPlayerHandle = {
  /** Jump to a position, in seconds. */
  seek: (seconds: number) => void
}

type StreamPlayerProps = {
  src: string | null
  poster?: string | null
  title?: string
  subtitles?: SubtitleTrack[]
  defaultSubtitle?: string
  className?: string
  /** Copy for the idle state, so the caller can say what it's waiting on. */
  emptyMessage?: string
  /**
   * Called when playback fails, so the caller can fail over to another source.
   * When provided, the player doesn't render its own error, the caller owns
   * that decision (it knows the remaining candidates).
   */
  onPlaybackError?: (message: string) => void
  /** Seconds to jump to once the player is ready, for resuming playback. */
  resumeAt?: number | null
  /** Reported periodically so the caller can persist the playback position. */
  onProgress?: (seconds: number, duration: number) => void
  ref?: React.Ref<StreamPlayerHandle>
}

type PlayerjsEventHandler = (event: string, id: string, data?: unknown) => void

/**
 * How often playback position is sampled. Deliberately frequent: sampling is
 * two cheap postMessages, and a coarse interval loses exactly the case that
 * matters, where you seek somewhere and then reload before the next tick.
 */
const PROGRESS_POLL_MS = 1_000
/** How often the sampled position is written to storage. */
const PROGRESS_PERSIST_MS = 5_000
/** Last-resort resume, in case the player's ready hook never fires. */
const RESUME_FALLBACK_MS = 3_000
/**
 * Bounded retries while the player refuses to hold the resume position.
 * `timeupdate` fires several times a second, so attempts are also spaced out:
 * otherwise the budget burns out in under a second, often before the media is
 * seekable at all.
 */
const MAX_RESUME_ATTEMPTS = 8
const RESUME_RETRY_INTERVAL_MS = 1_500
/** How far off the requested position still counts as "not resumed yet". */
const RESUME_TOLERANCE_SECONDS = 10

/**
 * Pull a playback position out of a PlayerJS `timeupdate` payload. The
 * documented shape is `{ seconds, duration }`, but the bridge is a black box,
 * so a bare number or `{ currentTime }` is accepted too.
 */
function toPosition(data: unknown): { seconds: number; duration: number } | null {
  if (!data || typeof data !== "object") {
    const seconds = toSeconds(data)
    return seconds == null ? null : { seconds, duration: 0 }
  }

  const record = data as { seconds?: unknown; duration?: unknown; currentTime?: unknown }
  const seconds = toSeconds(record.seconds) ?? toSeconds(record.currentTime)
  if (seconds == null) return null

  return { seconds, duration: toSeconds(record.duration) ?? 0 }
}

/** PlayerJS hands back a number, a numeric string, or `{ seconds }`. */
function toSeconds(value: unknown): number | null {
  if (typeof value === "number") return Number.isFinite(value) ? value : null
  if (typeof value === "string") {
    const parsed = Number(value)
    return Number.isFinite(parsed) ? parsed : null
  }
  if (value && typeof value === "object") {
    const seconds = (value as { seconds?: unknown }).seconds
    if (typeof seconds === "number") return Number.isFinite(seconds) ? seconds : null
  }
  return null
}

/**
 * PlayerJS exposes exactly one global event slot (`window.PlayerjsEvents`), so
 * install a dispatcher once for the whole page and let each mounted player
 * subscribe. Reassigning the slot per mount, the obvious approach, stacks
 * handlers when two players briefly overlap during a source switch.
 */
const playerEventListeners = new Set<PlayerjsEventHandler>()
let eventsInstalled = false

function ensurePlayerjsEventsInstalled(): void {
  if (eventsInstalled || typeof window === "undefined") return

  const previous = window.PlayerjsEvents
  window.PlayerjsEvents = (event, id, data) => {
    previous?.(event, id, data)
    for (const listener of [...playerEventListeners]) listener(event, id, data)
  }
  eventsInstalled = true
}

/** Removes whatever player DOM PlayerJS left behind under `root`. */
function removePlayerNodes(root: HTMLElement | null): void {
  if (!root) return
  for (const node of root.querySelectorAll("iframe, video")) node.remove()
}

/**
 * The element actually playing media.
 *
 * Position can be read and written through the PlayerJS bridge, but that leans
 * on its message plumbing forwarding commands and callbacks as documented,
 * which is not something we can verify. The media element itself is exact and
 * synchronous, and works no matter how the player renders.
 *
 * Scoped to the player's own subtree first, so an unrelated `<video>` elsewhere
 * on the page can never be mistaken for it.
 */
function findMediaElement(wrapper: HTMLElement | null): HTMLMediaElement | null {
  const scopes: HTMLElement[] = []
  if (wrapper) scopes.push(wrapper)
  if (wrapper?.parentElement) scopes.push(wrapper.parentElement)

  for (const scope of scopes) {
    const direct = scope.querySelector<HTMLMediaElement>("video, audio")
    if (direct) return direct

    for (const frame of Array.from(scope.querySelectorAll("iframe"))) {
      try {
        const inner = frame.contentDocument?.querySelector<HTMLMediaElement>("video, audio")
        if (inner) return inner
      } catch {
        // Cross-origin frame: not reachable, and not ours to touch.
      }
    }
  }

  return null
}

export function StreamPlayer({
  src,
  poster,
  title,
  subtitles,
  defaultSubtitle,
  className,
  emptyMessage,
  onPlaybackError,
  resumeAt,
  onProgress,
  ref,
}: StreamPlayerProps) {
  const wrapperRef = React.useRef<HTMLDivElement>(null)
  const playerRef = React.useRef<PlayerjsInstance | null>(null)
  const signatureRef = React.useRef<string | null>(null)
  const resumeAttemptsRef = React.useRef(0)
  const resumeLastAttemptRef = React.useRef(0)
  /** Newest sampled position, kept in memory so a refresh can still save it. */
  const latestPositionRef = React.useRef<{ seconds: number; duration: number } | null>(null)

  // PlayerJS is handed the id to mount into, so the id must be unique per
  // mount, a shared constant lets a stale player and a fresh one collide.
  const playerId = `streamflow-player-${React.useId().replace(/[^a-zA-Z0-9_-]/g, "")}`

  // PlayerJS is told about readiness through a *global function name*, so that
  // has to be unique per mount as well.
  const readyHandlerName = `__streamflowPlayerReady_${playerId.replace(/[^a-zA-Z0-9]/g, "_")}`

  const [scriptReady, setScriptReady] = React.useState(
    () => typeof window !== "undefined" && typeof window.Playerjs !== "undefined",
  )
  const [error, setError] = React.useState<string | null>(null)

  const subtitleConfig = React.useMemo(
    () =>
      formatPlayerjsSubtitles(subtitles, {
        defaultLabel: defaultSubtitle,
      }),
    [subtitles, defaultSubtitle],
  )

  const destroyPlayer = React.useCallback(() => {
    const player = playerRef.current
    playerRef.current = null
    signatureRef.current = null
    resumeAttemptsRef.current = 0
    resumeLastAttemptRef.current = 0
    latestPositionRef.current = null

    delete (window as unknown as Record<string, unknown>)[readyHandlerName]

    try {
      player?.api("stop")
    } catch {
      // ignore
    }

    // PlayerJS may mount its iframe inside the element it was given, or replace
    // that element outright. Clearing the wrapper covers the first case; the
    // parent sweep covers the second. Either way the old player, and the
    // segment requests it still had in flight, is gone before a new one boots.
    const wrapper = wrapperRef.current
    if (!wrapper) return
    wrapper.replaceChildren()
    removePlayerNodes(wrapper.parentElement)
  }, [readyHandlerName])

  // Held in refs so the player's own callbacks (the ready hook, the progress
  // poll) always read current values without rebuilding the player.
  const onPlaybackErrorRef = React.useRef(onPlaybackError)
  const resumeAtRef = React.useRef(resumeAt)
  const onProgressRef = React.useRef(onProgress)

  React.useEffect(() => {
    onPlaybackErrorRef.current = onPlaybackError
  }, [onPlaybackError])

  React.useEffect(() => {
    resumeAtRef.current = resumeAt
  }, [resumeAt])

  React.useEffect(() => {
    onProgressRef.current = onProgress
  }, [onProgress])

  React.useImperativeHandle(
    ref,
    () => ({
      seek: (seconds: number) => {
        try {
          playerRef.current?.api("seek", seconds)
        } catch {
          // The player may not be ready yet; nothing useful to do about it.
        }
      },
    }),
    [],
  )

  /**
   * Ask the player to jump to the saved position. Retried a bounded number of
   * times, because a seek issued before the media has loaded can be discarded
   * outright, and it stops as soon as the sampler sees us near the target.
   */
  const issueResumeSeek = React.useCallback(() => {
    const target = resumeAtRef.current
    if (!target || target <= 0) return
    if (resumeAttemptsRef.current >= MAX_RESUME_ATTEMPTS) return

    const now = Date.now()
    if (now - resumeLastAttemptRef.current < RESUME_RETRY_INTERVAL_MS) return
    resumeLastAttemptRef.current = now
    resumeAttemptsRef.current += 1

    // Preferred: move the media element itself. Setting `currentTime` before
    // metadata has loaded is ignored, so that case falls through and the
    // sampler retries a moment later.
    const media = findMediaElement(wrapperRef.current)
    if (media && media.readyState >= 1) {
      try {
        media.currentTime = target
        return
      } catch {
        // fall through to the bridge
      }
    }

    try {
      playerRef.current?.api("seek", target)
    } catch {
      // ignore
    }
  }, [])

  /**
   * Single place where a freshly observed position lands, whichever source it
   * came from (the `timeupdate` event, or the bridge sampler as a fallback).
   */
  const handlePosition = React.useCallback(
    (seconds: number, duration: number) => {
      latestPositionRef.current = { seconds, duration }

      // A seek issued before the media finished loading can be dropped, so keep
      // asking until we actually land near the saved position.
      const target = resumeAtRef.current
      if (target && target > 0 && seconds < target - RESUME_TOLERANCE_SECONDS) {
        issueResumeSeek()
      }
    },
    [issueResumeSeek],
  )

  const handlePositionRef = React.useRef(handlePosition)

  React.useEffect(() => {
    handlePositionRef.current = handlePosition
  }, [handlePosition])

  React.useEffect(() => {
    ensurePlayerjsEventsInstalled()

    const listener: PlayerjsEventHandler = (event, _id, data) => {
      // PlayerJS forwards the media element's own events, so position arrives
      // here without polling anything.
      if (event === "timeupdate") {
        const position = toPosition(data)
        if (position) handlePositionRef.current(position.seconds, position.duration)
        return
      }

      if (event !== "loaderror" && event !== "error") return

      const message =
        typeof data === "string" && data
          ? data
          : "Playback failed, stream segments may be blocked (403)"

      // The caller knows the remaining candidates, so it decides whether this
      // is fatal or a cue to try the next source.
      if (onPlaybackErrorRef.current) {
        onPlaybackErrorRef.current(message)
        return
      }

      setError(message)
    }

    playerEventListeners.add(listener)
    return () => {
      playerEventListeners.delete(listener)
    }
  }, [])

  React.useEffect(() => {
    if (!scriptReady || typeof window.Playerjs === "undefined" || !src) {
      destroyPlayer()
      return
    }

    // Subtitles are deliberately *not* part of the signature: switching track
    // is handled by `api("subtitle", …)` below, so it must not rebuild the
    // player (which would restart playback from zero).
    const signature = `${src}::${subtitleConfig.tracks ?? ""}`
    if (playerRef.current && signatureRef.current === signature) return

    setError(null)
    destroyPlayer()

    const frame = requestAnimationFrame(() => {
      const wrapper = wrapperRef.current
      if (!wrapper) return

      // Hand PlayerJS a node React does not own. If it replaces that node, React
      // can still remove the wrapper without colliding with a stale reference.
      const mount = document.createElement("div")
      mount.id = playerId
      mount.style.width = "100%"
      mount.style.height = "100%"
      wrapper.appendChild(mount)

      // PlayerJS calls a named global once the player is ready, which is the
      // first moment a `seek` is actually honoured. Commands sent before that
      // are dropped, so the resume point can't just be issued at construction.
      const globals = window as unknown as Record<string, unknown>
      globals[readyHandlerName] = issueResumeSeek

      try {
        playerRef.current = new window.Playerjs({
          id: playerId,
          file: src,
          poster: poster ?? undefined,
          title: title ?? undefined,
          subtitle: subtitleConfig.tracks,
          default_subtitle: subtitleConfig.defaultLabel,
          autoplay: 1,
          ready: readyHandlerName,
        })
        signatureRef.current = signature
        // Belt and braces: if the ready hook never fires on this build, still
        // resume rather than silently starting over.
        window.setTimeout(issueResumeSeek, RESUME_FALLBACK_MS)
      } catch (err) {
        setError(err instanceof Error ? err.message : "Failed to initialize player")
      }
    })

    return () => {
      cancelAnimationFrame(frame)
    }
  }, [
    scriptReady,
    src,
    poster,
    title,
    subtitleConfig.tracks,
    subtitleConfig.defaultLabel,
    playerId,
    readyHandlerName,
    issueResumeSeek,
    destroyPlayer,
  ])

  // Live subtitle switching: keeps the current player (and playback position)
  // instead of tearing it down for a new `default_subtitle`.
  React.useEffect(() => {
    const player = playerRef.current
    const label = subtitleConfig.defaultLabel
    if (!player || !src || !label) return
    try {
      player.api("subtitle", label)
    } catch {
      // The initial `default_subtitle` already covers first load.
    }
  }, [subtitleConfig.defaultLabel, src])

  // Track playback position and hand it to the caller to persist.
  //
  // The media element is the source of truth; the PlayerJS bridge is only
  // consulted if it isn't reachable. This interval doubles as the writer: the
  // newest value is kept in memory and flushed on a slower cadence, on unmount,
  // and when the page is being hidden. That last one is the important one,
  // since a reload is exactly when the position has to be current, and an async
  // round-trip cannot finish during unload.
  React.useEffect(() => {
    if (!src) return

    const flush = () => {
      const latest = latestPositionRef.current
      if (!latest) return
      onProgressRef.current?.(latest.seconds, latest.duration)
    }

    const sample = () => {
      // Preferred: read the media element directly.
      const media = findMediaElement(wrapperRef.current)
      if (media && Number.isFinite(media.duration) && media.duration > 0) {
        handlePosition(media.currentTime, media.duration)
        return
      }

      // Fallback: ask the player over the bridge.
      const player = playerRef.current
      if (!player) return
      try {
        player.api("getCurrentTime", (value: unknown) => {
          const seconds = toSeconds(value)
          if (seconds == null) return
          player.api("getDuration", (raw: unknown) => {
            handlePosition(seconds, toSeconds(raw) ?? 0)
          })
        })
      } catch {
        // Not ready yet; the next tick tries again.
      }
    }

    const sampleInterval = window.setInterval(sample, PROGRESS_POLL_MS)
    const flushInterval = window.setInterval(flush, PROGRESS_PERSIST_MS)
    window.addEventListener("pagehide", flush)
    document.addEventListener("visibilitychange", flush)

    return () => {
      window.clearInterval(sampleInterval)
      window.clearInterval(flushInterval)
      window.removeEventListener("pagehide", flush)
      document.removeEventListener("visibilitychange", flush)
      flush()
    }
  }, [src, handlePosition])

  React.useEffect(() => () => destroyPlayer(), [destroyPlayer])

  const hasSubtitles = Boolean(subtitles?.length)

  return (
    <>
      <Script
        src="/playerjs.js"
        strategy="afterInteractive"
        onLoad={() => setScriptReady(true)}
        onError={() => setError("Failed to load PlayerJS")}
      />

      <div
        className={cn(
          "relative aspect-video overflow-hidden rounded-xl border border-white/10 bg-black shadow-lg",
          className,
        )}
      >
        {src ? (
          <div
            ref={wrapperRef}
            className="absolute inset-0 h-full w-full [&_iframe]:h-full [&_iframe]:w-full"
          />
        ) : (
          <div className="absolute inset-0 flex items-center justify-center px-6 text-center text-sm text-muted-foreground">
            {emptyMessage ?? "Select a stream to start playback"}
          </div>
        )}

        {hasSubtitles && src && !error && (
          <div className="pointer-events-none absolute right-3 top-3 z-10 flex items-center gap-1.5 rounded-md bg-black/60 px-2 py-1 text-xs text-white/90 backdrop-blur-sm">
            <Captions className="size-3.5" />
            {subtitles!.length} subtitle track{subtitles!.length === 1 ? "" : "s"}
          </div>
        )}

        {error && (
          <div className="absolute inset-x-4 bottom-4 z-20 rounded-xl bg-destructive/20 px-4 py-2 text-sm text-red-200">
            {error}
          </div>
        )}
      </div>
    </>
  )
}
