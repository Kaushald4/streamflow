import Link from "next/link"
import Image from "next/image"
import { Calendar, Clock, Play, Star } from "lucide-react"
import type { TmdbEpisode } from "@/lib/tmdb"
import { formatDate, formatRuntime } from "@/lib/tmdb-format"
import { tmdbStill } from "@/lib/tmdb-image"

type EpisodeCardProps = {
  episode: TmdbEpisode
  tvId: string | number
  seasonNumber: number
}

export function EpisodeCard({ episode, tvId, seasonNumber }: EpisodeCardProps) {
  const still = tmdbStill(episode.still_path)
  const runtime = formatRuntime(episode.runtime)
  const airDate = formatDate(episode.air_date)

  return (
    <Link
      href={`/watch/tv/${tvId}/${seasonNumber}/${episode.episode_number}`}
      className="group overflow-hidden rounded-lg border border-border bg-card transition hover:border-amber-500/30"
    >
      <div className="relative aspect-video overflow-hidden bg-muted/20">
        {still ? (
          <Image
            src={still}
            alt={episode.name}
            fill
            sizes="(max-width: 768px) 100vw, 33vw"
            className="object-cover transition duration-500 group-hover:scale-105"
          />
        ) : (
          <div className="flex h-full items-center justify-center text-xs text-muted-foreground">
            No preview
          </div>
        )}
        <div className="absolute inset-0 bg-gradient-to-t from-black/80 via-black/20 to-transparent" />
        <div className="absolute right-3 bottom-3 left-3 flex items-end justify-between gap-2">
          <span className="rounded-lg bg-black/60 px-2 py-1 text-xs font-medium text-white">
            E{episode.episode_number}
          </span>
          <span className="flex size-9 items-center justify-center rounded-full bg-amber-600 text-white opacity-0 transition group-hover:opacity-100">
            <Play className="size-4 fill-current" />
          </span>
        </div>
      </div>
      <div className="space-y-2 p-4">
        <h3 className="line-clamp-1 font-medium">{episode.name}</h3>
        <div className="flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
          {airDate && (
            <span className="inline-flex items-center gap-1">
              <Calendar className="size-3" />
              {airDate}
            </span>
          )}
          {runtime && (
            <span className="inline-flex items-center gap-1">
              <Clock className="size-3" />
              {runtime}
            </span>
          )}
          {episode.vote_average != null && episode.vote_average > 0 && (
            <span className="inline-flex items-center gap-1">
              <Star className="size-3 fill-amber-400 text-amber-400" />
              {episode.vote_average.toFixed(1)}
            </span>
          )}
        </div>
        {episode.overview && (
          <p className="line-clamp-3 text-sm leading-relaxed text-muted-foreground">
            {episode.overview}
          </p>
        )}
      </div>
    </Link>
  )
}
