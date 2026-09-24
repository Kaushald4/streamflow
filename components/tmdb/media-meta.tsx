import Link from "next/link"
import { MediaCard } from "@/components/media-card"
import type { TmdbRecommendation } from "@/lib/tmdb"
import { mediaTitle } from "@/lib/tmdb"
import { DetailSection } from "@/components/tmdb/section"

type RecommendationsRowProps = {
  items: TmdbRecommendation[]
  mediaType: "movie" | "tv"
}

export function RecommendationsRow({ items, mediaType }: RecommendationsRowProps) {
  const filtered = items
    .filter((item) => item.poster_path)
    .slice(0, 12)

  if (filtered.length === 0) return null

  return (
    <DetailSection title="More like this">
      <div className="flex gap-4 overflow-x-auto pb-2 [-ms-overflow-style:none] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        {filtered.map((item) => {
          const type = item.media_type ?? mediaType
          const title = mediaTitle(item)
          return (
            <MediaCard
              key={item.id}
              id={item.id}
              type={type}
              title={title}
              posterPath={item.poster_path}
              rating={item.vote_average}
              className="w-[140px] sm:w-[160px]"
            />
          )
        })}
      </div>
    </DetailSection>
  )
}

type FactItem = {
  label: string
  value: string
}

type MediaFactsProps = {
  facts: FactItem[]
}

export function MediaFacts({ facts }: MediaFactsProps) {
  const visible = facts.filter((f) => f.value)
  if (visible.length === 0) return null

  return (
    <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
      {visible.map((fact) => (
        <div
          key={fact.label}
          className="rounded-lg border border-border bg-card px-4 py-3"
        >
          <p className="text-xs uppercase tracking-wide text-muted-foreground">
            {fact.label}
          </p>
          <p className="mt-1 text-sm font-medium">{fact.value}</p>
        </div>
      ))}
    </div>
  )
}

type SeasonTabsProps = {
  tvId: string
  seasons: { id: number; season_number: number; name: string; poster_path: string | null }[]
  activeSeason: number
}

export function SeasonTabs({ tvId, seasons, activeSeason }: SeasonTabsProps) {
  return (
    <div className="flex gap-3 overflow-x-auto pb-2 [-ms-overflow-style:none] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
      {seasons.map((season) => {
        const active = season.season_number === activeSeason
        return (
          <Link
            key={season.id}
            href={`/tv/${tvId}?season=${season.season_number}`}
            className={`shrink-0 rounded-lg border px-4 py-2.5 text-sm font-medium transition ${
              active
                ? "border-amber-500/40 bg-amber-500/10 text-foreground"
                : "border-border bg-card hover:border-foreground/25"
            }`}
          >
            {season.name || `Season ${season.season_number}`}
          </Link>
        )
      })}
    </div>
  )
}
