import { MediaCard } from "@/components/media-card"
import type { TmdbSearchResult } from "@/lib/tmdb"
import { mediaTitle, mediaYear } from "@/lib/tmdb"

type MediaRowProps = {
  title: string
  items: TmdbSearchResult[]
}

export function MediaRow({ title, items }: MediaRowProps) {
  if (items.length === 0) return null

  return (
    <section className="space-y-4">
      <div className="flex items-end justify-between gap-4">
        <h2 className="font-heading text-xl font-medium tracking-tight sm:text-2xl">{title}</h2>
      </div>
      <div className="scrollbar-hide -mx-4 flex gap-4 overflow-x-auto px-4 pb-2 sm:-mx-6 sm:px-6">
        {items.map((item) => (
          <MediaCard
            key={`${item.media_type}-${item.id}`}
            id={item.id}
            type={item.media_type}
            title={mediaTitle(item)}
            posterPath={item.poster_path}
            rating={item.vote_average}
            year={mediaYear(item)}
            className="w-[140px] sm:w-[170px]"
          />
        ))}
      </div>
    </section>
  )
}
