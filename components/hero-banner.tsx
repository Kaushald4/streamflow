import Image from "next/image"
import Link from "next/link"
import { Play } from "lucide-react"
import { Button } from "@/components/ui/button"
import { tmdbBackdrop } from "@/lib/tmdb-image"
import type { TmdbSearchResult } from "@/lib/tmdb"
import { mediaTitle, mediaYear } from "@/lib/tmdb"

type HeroBannerProps = {
  item: TmdbSearchResult
}

export function HeroBanner({ item }: HeroBannerProps) {
  const title = mediaTitle(item)
  const year = mediaYear(item)
  const backdrop = tmdbBackdrop(item.backdrop_path)
  const href =
    item.media_type === "movie"
      ? `/movie/${item.id}`
      : `/tv/${item.id}`
  const watchHref =
    item.media_type === "movie"
      ? `/watch/movie/${item.id}`
      : `/tv/${item.id}`

  return (
    <section className="relative overflow-hidden rounded-2xl border border-border">
      <div className="absolute inset-0">
        {backdrop ? (
          <Image
            src={backdrop}
            alt={title}
            fill
            priority
            className="object-cover"
          />
        ) : (
          <div className="h-full w-full bg-neutral-900" />
        )}
        <div className="absolute inset-0 bg-gradient-to-r from-black/90 via-black/60 to-black/10" />
        <div className="absolute inset-0 bg-gradient-to-t from-black/80 via-transparent to-transparent" />
      </div>

      <div className="relative flex min-h-[420px] flex-col justify-end gap-5 p-8 sm:p-10 lg:min-h-[520px] lg:p-12">
        <p className="text-xs font-semibold tracking-[0.2em] text-amber-400 uppercase">
          Featured {item.media_type === "movie" ? "film" : "series"}
        </p>

        <div className="max-w-2xl space-y-4">
          <h1 className="font-heading text-4xl font-medium text-white sm:text-5xl lg:text-6xl">
            {title}
          </h1>
          <div className="flex flex-wrap items-center gap-2 text-sm text-white/70">
            {year && <span>{year}</span>}
            {year && <span className="text-white/30">·</span>}
            <span className="capitalize">{item.media_type === "movie" ? "Film" : "TV series"}</span>
          </div>
          <p className="line-clamp-3 text-base leading-relaxed text-white/75 sm:text-lg">
            {item.overview || "Resolve streams through modular extraction adapters."}
          </p>
        </div>

        <div className="flex flex-wrap gap-3">
          <Link href={watchHref}>
            <Button size="lg" className="bg-white text-black hover:bg-white/90">
              <Play data-icon="inline-start" className="fill-current" />
              Watch now
            </Button>
          </Link>
          <Link href={href}>
            <Button size="lg" variant="outline" className="border-white/20 bg-white/5 text-white hover:bg-white/10">
              Details
            </Button>
          </Link>
          <Link href="/plugins">
            <Button size="lg" variant="ghost" className="text-white/80 hover:bg-white/10 hover:text-white">
              Configure adapters
            </Button>
          </Link>
        </div>
      </div>
    </section>
  )
}
