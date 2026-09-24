"use client"

import * as React from "react"
import Link from "next/link"
import { AlertCircle, Captions, Check, ChevronRight, Loader2, Plug, RefreshCw } from "lucide-react"
import type { Stream } from "stream-resolver"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { usePlugins } from "@/components/plugin-provider"
import { StreamPlayer, type StreamPlayerHandle } from "@/components/stream-player"
import { pluginsForMedia, getMetadataHintLabel, getPluginLabel } from "@/lib/plugins/catalog"
import { subtitleDisplayLabel } from "@/lib/subtitles/labels"
import { companionFetch } from "@/lib/api/companion"
import { registerStreamPlayback, type StreamPlayback } from "@/lib/stream/client"
import {
  preferredSource,
  recordStreamFailure,
  recordStreamSuccess,
  rememberPreferredSource,
  sortStreamsByHealth,
  streamScore,
} from "@/lib/stream/health"
import {
  clearResumePoint,
  formatPlaybackTime,
  readResumePoint,
  writeResumePoint,
} from "@/lib/stream/resume"
import { cn } from "@/lib/utils"

/**
 * How many automatic source switches one playback attempt may make. Without a
 * cap, a run of dead links would walk the entire candidate list silently.
 */
const MAX_FAILOVERS = 5

type SourceStatus = "pending" | "ok" | "empty" | "error"

type SourceGroup = {
  extractorId: string
  status: SourceStatus
  /** Raw (unproxied) candidates, health-sorted, best-known first. */
  streams: Stream[]
  error?: string
  durationMs?: number
}

type WatchPanelProps = {
  kind: "movie" | "episode"
  tmdbId: number
  imdbId?: string
  season?: number
  episode?: number
  title: string
  poster?: string | null
}

function streamDebugLabel(stream: Stream | null | undefined): string | undefined {
  if (!stream) return undefined
  const header = stream.headers?.xDebug
  if (typeof header === "string" && header.length > 0) return header
  return stream.label
}

/** Identity of a *candidate*, raw URL, not the session URL it becomes. */
function candidateKey(stream: Stream): string {
  return `${stream.source?.extractor ?? "?"}::${stream.url}`
}

function candidateLabel(stream: Stream): string {
  return (
    streamDebugLabel(stream) ??
    stream.quality ??
    stream.type ??
    stream.source?.extractor ??
    "source"
  )
}

export function WatchPanel({
  kind,
  tmdbId,
  imdbId,
  season,
  episode,
  title,
  poster,
}: WatchPanelProps) {
  const { installed, ready, catalog, companionAvailable } = usePlugins()

  const [groups, setGroups] = React.useState<Record<string, SourceGroup>>({})
  const [selectedPlugins, setSelectedPlugins] = React.useState<string[]>([])
  const [active, setActive] = React.useState<Stream | null>(null)
  const [playback, setPlayback] = React.useState<StreamPlayback | null>(null)
  const [preparing, setPreparing] = React.useState(false)
  const [failoverNotice, setFailoverNotice] = React.useState<string | null>(null)
  const [fatalError, setFatalError] = React.useState<string | null>(null)
  const [subtitleLabel, setSubtitleLabel] = React.useState<string | undefined>()
  const [adaptersOpen, setAdaptersOpen] = React.useState(false)
  const [resumeFrom, setResumeFrom] = React.useState<number | null>(null)

  const playerHandleRef = React.useRef<StreamPlayerHandle | null>(null)
  const runRef = React.useRef<AbortController | null>(null)
  const playbackRef = React.useRef<AbortController | null>(null)
  const activeRef = React.useRef<Stream | null>(null)
  const attemptedRef = React.useRef<Set<string>>(new Set())
  const failoverCountRef = React.useRef(0)
  const selectionInitialisedRef = React.useRef(false)
  const handleFailureRef = React.useRef<(stream: Stream, message: string) => void>(() => {})

  const available = pluginsForMedia(catalog, kind, installed)
  const titleKey = `${kind}-${tmdbId}-${season ?? ""}-${episode ?? ""}`

  const groupsList = React.useMemo(() => Object.values(groups), [groups])
  const candidates = React.useMemo(
    () => sortStreamsByHealth(groupsList.flatMap((group) => group.streams)),
    [groupsList],
  )
  const running = groupsList.some((group) => group.status === "pending")

  const startPlayback = React.useCallback(
    async (stream: Stream) => {
      playbackRef.current?.abort()
      const controller = new AbortController()
      playbackRef.current = controller

      setActive(stream)
      activeRef.current = stream
      setPlayback(null)
      setPreparing(true)

      try {
        const next = await registerStreamPlayback(stream, controller.signal)
        if (controller.signal.aborted) return

        if (!next) {
          handleFailureRef.current(stream, "could not prepare this source")
          return
        }

        setPlayback(next)
        setFailoverNotice(null)
        setFatalError(null)
        setResumeFrom(readResumePoint(titleKey))
        recordStreamSuccess(stream)
        if (stream.source?.extractor) {
          rememberPreferredSource(titleKey, stream.source.extractor)
        }
      } catch (err) {
        if (controller.signal.aborted) return
        handleFailureRef.current(
          stream,
          err instanceof Error ? err.message : "could not prepare this source",
        )
      } finally {
        if (playbackRef.current === controller) {
          playbackRef.current = null
          setPreparing(false)
        }
      }
    },
    [titleKey],
  )

  /**
   * A source that won't play is routine here (CDNs rate-limit, mirrors die), so
   * this records the outcome and moves to the next candidate rather than
   * stopping at the first failure. Only an exhausted list is an error.
   */
  const handleFailure = React.useCallback(
    (stream: Stream, message: string) => {
      recordStreamFailure(stream)
      attemptedRef.current.add(candidateKey(stream))

      const next = candidates.find((c) => !attemptedRef.current.has(candidateKey(c))) ?? null
      const withinBudget = failoverCountRef.current < MAX_FAILOVERS

      if (next && withinBudget) {
        failoverCountRef.current += 1
        setFailoverNotice(
          `${candidateLabel(stream)} failed (${message}), switching to ${candidateLabel(next)}…`,
        )
        void startPlayback(next)
        return
      }

      setPlayback(null)
      setFailoverNotice(null)
      setFatalError(
        attemptedRef.current.size > 1
          ? `Tried ${attemptedRef.current.size} sources; none played. Last error: ${message}`
          : `Playback failed: ${message}`,
      )
    },
    [candidates, startPlayback],
  )

  React.useEffect(() => {
    handleFailureRef.current = handleFailure
  }, [handleFailure])

  // Persist the position as it plays, so coming back resumes here.
  const handleProgress = React.useCallback(
    (seconds: number, duration: number) => {
      writeResumePoint(titleKey, seconds, duration || undefined)
    },
    [titleKey],
  )

  const startOver = React.useCallback(() => {
    clearResumePoint(titleKey)
    playerHandleRef.current?.seek(0)
    setResumeFrom(null)
  }, [titleKey])

  const runExtraction = React.useCallback(
    async (extractorIds?: string[]) => {
      const targets = extractorIds ?? selectedPlugins
      if (targets.length === 0 || !companionAvailable) return

      // One request per adapter, all in flight together: the fast resolver
      // shows up in about a second and the slow federated one fills in later.
      // A fresh run also supersedes anything still in flight.
      runRef.current?.abort()
      const controller = new AbortController()
      runRef.current = controller

      attemptedRef.current = new Set()
      failoverCountRef.current = 0
      setFailoverNotice(null)
      setFatalError(null)

      setGroups((prev) => {
        const next = { ...prev }
        for (const id of targets) {
          // Keep whatever this source already produced visible while it
          // re-resolves, re-running must not blank the page under the player.
          next[id] = {
            ...(prev[id] ?? { extractorId: id, streams: [] }),
            extractorId: id,
            status: "pending",
            error: undefined,
          }
        }
        return next
      })

      await Promise.all(
        targets.map(async (id) => {
          const startedAt = Date.now()
          try {
            const res = await companionFetch("/api/extract", {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({
                kind,
                tmdbId,
                imdbId,
                season,
                episode,
                extractors: [id],
              }),
              signal: controller.signal,
            })

            const data = (await res.json()) as {
              streams?: Stream[]
              errors?: { message?: string }[]
              error?: string
            }
            if (controller.signal.aborted) return
            if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`)

            const streams = sortStreamsByHealth(data.streams ?? [])
            setGroups((prev) => ({
              ...prev,
              [id]: {
                extractorId: id,
                status: streams.length > 0 ? "ok" : "empty",
                streams,
                error:
                  streams.length > 0
                    ? undefined
                    : (data.errors?.[0]?.message ?? "no playable streams"),
                durationMs: Date.now() - startedAt,
              },
            }))

            // Whichever source answers first starts playing; nothing waits for
            // the slow ones. A source that worked for this title last time wins
            // the tie.
            if (!activeRef.current && streams.length > 0) {
              const remembered = preferredSource(titleKey)
              const pick =
                streams.find((s) => s.source?.extractor === remembered) ?? streams[0]
              if (pick) void startPlayback(pick)
            }
          } catch (err) {
            if (controller.signal.aborted) return
            setGroups((prev) => ({
              ...prev,
              [id]: {
                extractorId: id,
                status: "error",
                streams: prev[id]?.streams ?? [],
                error: err instanceof Error ? err.message : "extraction failed",
                durationMs: Date.now() - startedAt,
              },
            }))
          }
        }),
      )
    },
    [
      selectedPlugins,
      companionAvailable,
      kind,
      tmdbId,
      imdbId,
      season,
      episode,
      startPlayback,
      titleKey,
    ],
  )

  React.useEffect(() => () => {
    runRef.current?.abort()
    playbackRef.current?.abort()
  }, [])

  React.useEffect(() => {
    const first = playback?.subtitles?.[0]
    setSubtitleLabel(first ? subtitleDisplayLabel(first) : undefined)
  }, [playback])

  // Default to every enabled adapter the first time they become known, so the
  // page is usable without hunting through the registry first.
  React.useEffect(() => {
    if (!ready) return
    if (!selectionInitialisedRef.current && available.length > 0) {
      selectionInitialisedRef.current = true
      setSelectedPlugins(available.map((plugin) => plugin.id))
      return
    }
    setSelectedPlugins((prev) => {
      const allowed = new Set(available.map((plugin) => plugin.id))
      return prev.filter((id) => allowed.has(id))
    })
  }, [ready, installed.join(","), kind, catalog])

  const activeKey = active ? candidateKey(active) : null
  const subtitleTracks = playback?.subtitles ?? active?.subtitles

  const playerMessage = preparing
    ? "Preparing source…"
    : running
      ? "Resolving sources…"
      : fatalError
        ? "No source would play, try again from the panel"
        : "Choose a source from the panel to start playback"

  return (
    <div className="lg:grid lg:grid-cols-[minmax(0,1fr)_368px] lg:items-start lg:gap-6">
      {/* ── Player ─────────────────────────────────────────────────────── */}
      <div className="space-y-3 lg:sticky lg:top-24">
        <StreamPlayer
          key={playback?.url ?? "no-stream"}
          ref={playerHandleRef}
          src={playback?.url ?? null}
          poster={poster}
          title={title}
          subtitles={playback?.subtitles}
          defaultSubtitle={subtitleLabel}
          emptyMessage={playerMessage}
          resumeAt={resumeFrom}
          onProgress={handleProgress}
          onPlaybackError={(message) => {
            const stream = activeRef.current
            if (stream) handleFailure(stream, message)
          }}
        />

        {failoverNotice && (
          <div className="flex items-center gap-2 rounded-lg border border-amber-500/25 bg-amber-500/5 px-3 py-2 text-xs text-amber-200">
            <Loader2 className="size-3 shrink-0 animate-spin" />
            <span className="truncate">{failoverNotice}</span>
          </div>
        )}

        {resumeFrom != null && playback && (
          <div className="flex items-center justify-between gap-3 rounded-lg border border-border/60 bg-secondary/40 px-3 py-2 text-xs text-muted-foreground">
            <span className="truncate">
              Resumed from {formatPlaybackTime(resumeFrom)}
            </span>
            <button
              type="button"
              onClick={startOver}
              className="shrink-0 underline-offset-2 hover:text-foreground hover:underline"
            >
              Start over
            </button>
          </div>
        )}

        {fatalError && (
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-red-500/25 bg-red-500/5 px-3 py-2.5 text-xs text-red-200">
            <span className="flex min-w-0 items-center gap-2">
              <AlertCircle className="size-3.5 shrink-0" />
              <span className="truncate">{fatalError}</span>
            </span>
            <Button
              variant="outline"
              size="sm"
              className="h-7 shrink-0 px-2 text-xs"
              onClick={() => void runExtraction()}
            >
              <RefreshCw data-icon="inline-start" className="size-3" />
              Retry
            </Button>
          </div>
        )}

        <div className="space-y-2.5 rounded-2xl border border-border/60 bg-card/40 p-4">
          <div className="flex flex-wrap items-center gap-2">
            <h1 className="font-heading text-xl font-medium tracking-tight sm:text-2xl">
              {title}
            </h1>
            {active?.quality && <Badge>{active.quality}</Badge>}
            {active?.type && <Badge className="uppercase">{active.type}</Badge>}
            {active?.source?.extractor && (
              <Badge>{getPluginLabel(active.source.extractor, catalog)}</Badge>
            )}
            {active?.subtitles && active.subtitles.length > 0 && (
              <Badge>
                <Captions data-icon="inline-start" className="size-3" />
                {active.subtitles.length}
              </Badge>
            )}
          </div>

          {streamDebugLabel(active) && (
            <p className="truncate font-mono text-[11px] text-muted-foreground">
              {streamDebugLabel(active)}
            </p>
          )}

          {subtitleTracks && subtitleTracks.length > 0 && (
            <div className="flex flex-wrap items-center gap-1.5">
              <span className="mr-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
                Subtitles
              </span>
              {subtitleTracks.map((track) => {
                const label = subtitleDisplayLabel(track)
                const isActive = label === subtitleLabel
                return (
                  <button
                    key={`${track.url}-${label}`}
                    type="button"
                    onClick={() => setSubtitleLabel(label)}
                    className={cn(
                      "rounded-full border px-2.5 py-1 text-xs transition",
                      isActive
                        ? "border-amber-500/40 bg-amber-500/10 text-foreground"
                        : "border-border/70 text-muted-foreground hover:border-foreground/25 hover:text-foreground",
                    )}
                  >
                    {label}
                  </button>
                )
              })}
            </div>
          )}
        </div>
      </div>

      {/* ── Sources ────────────────────────────────────────────────────── */}
      <aside className="mt-6 space-y-3 lg:mt-0 lg:sticky lg:top-24 lg:max-h-[calc(100svh-8rem)] lg:overflow-y-auto lg:pr-1">
        <div className="overflow-hidden rounded-2xl border border-border/60 bg-card/40 backdrop-blur">
          <div className="flex items-center justify-between gap-2 border-b border-border/60 px-3.5 py-2.5">
            <div className="flex items-center gap-2">
              <Plug className="size-3.5 text-amber-500" />
              <span className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
                Sources
              </span>
              {candidates.length > 0 && (
                <span className="rounded-full bg-secondary/70 px-1.5 py-0.5 text-[10px] text-muted-foreground">
                  {candidates.length}
                </span>
              )}
            </div>
            <Button
              variant="ghost"
              size="sm"
              className="h-7 gap-1.5 px-2 text-xs"
              onClick={() => void runExtraction()}
              disabled={running || selectedPlugins.length === 0 || !companionAvailable}
            >
              <RefreshCw className={cn("size-3.5", running && "animate-spin")} />
              {running ? "Resolving" : "Resolve"}
            </Button>
          </div>

          {!companionAvailable ? (
            <div className="space-y-3 p-4 text-sm text-muted-foreground">
              <p>
                Streamflow Companion not detected. Streaming from adapters runs locally
                on your machine. Install and run the companion to
                enable it.
              </p>
              <Link href="/plugins">
                <Button className="w-full">Get the Streamflow Companion</Button>
              </Link>
            </div>
          ) : available.length === 0 ? (
            <div className="space-y-3 p-4 text-sm text-muted-foreground">
              <p>No adapters enabled. Configure adapters in the registry.</p>
              <Link href="/plugins">
                <Button className="w-full">Open adapter registry</Button>
              </Link>
            </div>
          ) : (
            <>
              {/* Adapter selection is configuration, not content: kept behind a
                  disclosure so the panel body belongs to results. */}
              <button
                type="button"
                onClick={() => setAdaptersOpen((open) => !open)}
                className="flex w-full items-center justify-between gap-2 border-b border-border/60 px-3.5 py-2 text-left text-xs text-muted-foreground transition hover:text-foreground"
              >
                <span className="flex items-center gap-1.5">
                  <ChevronRight
                    className={cn("size-3.5 transition-transform", adaptersOpen && "rotate-90")}
                  />
                  Adapters
                </span>
                <span className="font-mono text-[11px]">
                  {selectedPlugins.length}/{available.length}
                </span>
              </button>

              {adaptersOpen && (
                <div className="space-y-0.5 border-b border-border/60 px-2 py-1.5">
                  {available.map((plugin) => {
                    const checked = selectedPlugins.includes(plugin.id)
                    return (
                      <button
                        key={plugin.id}
                        type="button"
                        title={getMetadataHintLabel(plugin.idHint)}
                        onClick={() =>
                          setSelectedPlugins((prev) =>
                            checked
                              ? prev.filter((id) => id !== plugin.id)
                              : [...prev, plugin.id],
                          )
                        }
                        className={cn(
                          "flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left text-xs transition",
                          checked
                            ? "text-foreground hover:bg-accent/50"
                            : "text-muted-foreground hover:bg-accent/40 hover:text-foreground",
                        )}
                      >
                        <span
                          className={cn(
                            "flex size-3.5 shrink-0 items-center justify-center rounded border transition",
                            checked
                              ? "border-amber-500 bg-amber-500 text-background"
                              : "border-border",
                          )}
                        >
                          {checked && <Check className="size-2.5" strokeWidth={3} />}
                        </span>
                        <span className="truncate">{plugin.name}</span>
                        <StatusDot status={groups[plugin.id]?.status} />
                      </button>
                    )
                  })}
                </div>
              )}

              {groupsList.length === 0 && (
                <div className="px-6 py-10 text-center">
                  <p className="text-sm text-muted-foreground">No sources resolved yet</p>
                  <p className="mt-1.5 text-xs text-muted-foreground/70">
                    Resolve to query every enabled adapter in parallel.
                  </p>
                </div>
              )}

              <div className="divide-y divide-border/60">
                {groupsList.map((group) => (
                  <div key={group.extractorId} className="px-3.5 py-3">
                    <div className="flex items-center justify-between gap-2">
                      <div className="flex min-w-0 items-center gap-2">
                        <StatusDot status={group.status} />
                        <span className="truncate text-sm font-medium">
                          {getPluginLabel(group.extractorId, catalog)}
                        </span>
                      </div>
                      <div className="flex shrink-0 items-center gap-2 text-[11px] text-muted-foreground">
                        <GroupSummary group={group} />
                        {(group.status === "error" || group.status === "empty") &&
                          !running && (
                            <button
                              type="button"
                              onClick={() => void runExtraction([group.extractorId])}
                              className="underline-offset-2 hover:text-foreground hover:underline"
                            >
                              retry
                            </button>
                          )}
                      </div>
                    </div>

                    {group.status === "error" && group.error && (
                      <p className="mt-1 pl-[14px] text-[11px] text-amber-400/80">
                        {group.error}
                      </p>
                    )}

                    {group.streams.length > 0 && (
                      <div className="mt-2 space-y-1">
                        {group.streams.map((stream) => {
                          const isActive = candidateKey(stream) === activeKey
                          const debug = streamDebugLabel(stream)
                          const score = streamScore(stream)
                          return (
                            <button
                              key={candidateKey(stream)}
                              type="button"
                              onClick={() => void startPlayback(stream)}
                              className={cn(
                                "flex w-full items-center gap-2 rounded-lg border px-2.5 py-2 text-left transition",
                                isActive
                                  ? "border-amber-500/50 bg-amber-500/10"
                                  : "border-transparent hover:border-border/70 hover:bg-accent/40",
                              )}
                            >
                              <div className="min-w-0 flex-1">
                                <div className="flex items-center gap-1.5">
                                  <span className="truncate text-sm">
                                    {stream.quality ?? stream.type ?? "stream"}
                                  </span>
                                  {stream.quality && stream.type && (
                                    <span className="shrink-0 rounded bg-secondary/70 px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-muted-foreground">
                                      {stream.type}
                                    </span>
                                  )}
                                </div>
                                {debug && (
                                  <p className="truncate font-mono text-[10px] text-muted-foreground">
                                    {debug}
                                  </p>
                                )}
                              </div>
                              {score !== 0 && (
                                <span
                                  className={cn(
                                    "shrink-0 text-[10px]",
                                    score > 0 ? "text-emerald-400" : "text-amber-400",
                                  )}
                                >
                                  {score > 0 ? `✓${score}` : "✕"}
                                </span>
                              )}
                              {isActive && (
                                <Check className="size-3.5 shrink-0 text-amber-500" />
                              )}
                            </button>
                          )
                        })}
                      </div>
                    )}
                  </div>
                ))}
              </div>
            </>
          )}
        </div>
      </aside>
    </div>
  )
}

function StatusDot({ status }: { status?: SourceStatus }) {
  const tone =
    status === "ok"
      ? "bg-emerald-400"
      : status === "pending"
        ? "animate-pulse bg-amber-400"
        : status === "error" || status === "empty"
          ? "bg-red-400/80"
          : "bg-muted-foreground/30"

  return <span className={cn("size-1.5 shrink-0 rounded-full", tone)} />
}

function GroupSummary({ group }: { group: SourceGroup }) {
  if (group.status === "pending") return <span>resolving…</span>
  if (group.status === "empty") return <span>no streams</span>
  if (group.status === "error") return <span>failed</span>
  return (
    <span>
      {group.streams.length}
      {group.durationMs != null ? ` · ${(group.durationMs / 1000).toFixed(1)}s` : ""}
    </span>
  )
}
