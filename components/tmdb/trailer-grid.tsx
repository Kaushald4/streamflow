"use client"

import * as React from "react"
import Image from "next/image"
import { Play, X } from "lucide-react"
import type { TmdbVideo } from "@/lib/tmdb"
import { DetailSection } from "@/components/tmdb/section"
import { cn } from "@/lib/utils"

type TrailerGridProps = {
  trailers: TmdbVideo[]
}

const INITIAL_VISIBLE = 3

function youtubeThumb(key: string): string {
  return `https://img.youtube.com/vi/${key}/hqdefault.jpg`
}

function TrailerCard({
  video,
  onPlay,
}: {
  video: TmdbVideo
  onPlay: (video: TmdbVideo) => void
}) {
  return (
    <button
      type="button"
      onClick={() => onPlay(video)}
      className="group overflow-hidden rounded-lg border border-border bg-card text-left transition hover:border-amber-500/30"
    >
      <div className="relative aspect-video overflow-hidden bg-black">
        <Image
          src={youtubeThumb(video.key)}
          alt={video.name}
          fill
          sizes="(max-width: 768px) 100vw, 33vw"
          className="object-cover transition duration-300 group-hover:scale-105"
        />
        <div className="absolute inset-0 flex items-center justify-center bg-black/30 transition group-hover:bg-black/20">
          <span className="flex size-12 items-center justify-center rounded-full bg-amber-600 text-white shadow-lg">
            <Play className="size-5 fill-current" />
          </span>
        </div>
        {video.official && (
          <span className="absolute top-2 left-2 rounded-full bg-black/60 px-2 py-0.5 text-[10px] font-medium uppercase tracking-wide text-white">
            Official
          </span>
        )}
      </div>
      <div className="space-y-1 p-3">
        <p className="line-clamp-2 text-sm font-medium">{video.name}</p>
        <p className="text-xs text-muted-foreground">{video.type}</p>
      </div>
    </button>
  )
}

export function TrailerGrid({ trailers }: TrailerGridProps) {
  const [active, setActive] = React.useState<TmdbVideo | null>(null)
  const [expanded, setExpanded] = React.useState(false)

  if (trailers.length === 0) return null

  const hasMore = trailers.length > INITIAL_VISIBLE
  const visible = expanded ? trailers : trailers.slice(0, INITIAL_VISIBLE)

  return (
    <>
      <DetailSection
        title="Trailers & videos"
        subtitle={`${trailers.length} clip${trailers.length === 1 ? "" : "s"} available`}
      >
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {visible.map((video) => (
            <TrailerCard key={video.id} video={video} onPlay={setActive} />
          ))}
        </div>
        {hasMore && (
          <button
            type="button"
            onClick={() => setExpanded((prev) => !prev)}
            className="text-sm font-medium text-amber-600 transition hover:text-amber-500 dark:text-amber-400 dark:hover:text-amber-300"
          >
            {expanded
              ? "Show less"
              : `Show ${trailers.length - INITIAL_VISIBLE} more`}
          </button>
        )}
      </DetailSection>

      {active && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 p-4 backdrop-blur-sm"
          onClick={() => setActive(null)}
          onKeyDown={(e) => e.key === "Escape" && setActive(null)}
          role="dialog"
          aria-modal="true"
          aria-label={active.name}
        >
          <div
            className="relative w-full max-w-4xl overflow-hidden rounded-lg border border-white/10 bg-black shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between border-b border-white/10 px-4 py-3">
              <p className="truncate pr-4 text-sm font-medium">{active.name}</p>
              <button
                type="button"
                onClick={() => setActive(null)}
                className={cn(
                  "rounded-lg p-1.5 text-muted-foreground transition hover:bg-white/10 hover:text-white",
                )}
                aria-label="Close trailer"
              >
                <X className="size-5" />
              </button>
            </div>
            <div className="aspect-video">
              <iframe
                src={`https://www.youtube.com/embed/${active.key}?autoplay=1&rel=0`}
                title={active.name}
                allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture"
                allowFullScreen
                className="h-full w-full"
              />
            </div>
          </div>
        </div>
      )}
    </>
  )
}
