import Image from "next/image"
import Link from "next/link"
import { Star } from "lucide-react"
import { tmdbPoster } from "@/lib/tmdb-image"
import { cn } from "@/lib/utils"

type MediaCardProps = {
  id: number
  type: "movie" | "tv"
  title: string
  posterPath?: string | null
  rating?: number
  year?: string
  className?: string
}

export function MediaCard({
  id,
  type,
  title,
  posterPath,
  rating,
  year,
  className,
}: MediaCardProps) {
  const href = type === "movie" ? `/movie/${id}` : `/tv/${id}`
  const poster = tmdbPoster(posterPath)

  return (
    <Link
      href={href}
      className={cn(
        "group relative block shrink-0 overflow-hidden rounded-lg border border-border bg-card transition duration-300 hover:-translate-y-1 hover:border-foreground/25",
        className,
      )}
    >
      <div className="relative aspect-[2/3] w-full bg-muted/20">
        {poster ? (
          <Image
            src={poster}
            alt={title}
            fill
            sizes="(max-width: 768px) 40vw, 200px"
            className="object-cover transition duration-500 group-hover:scale-105"
          />
        ) : (
          <div className="flex h-full items-center justify-center text-xs text-muted-foreground">
            No poster
          </div>
        )}
        <div className="absolute inset-0 bg-gradient-to-t from-black/80 via-transparent to-transparent opacity-80" />
        <div className="absolute right-2 bottom-2 left-2">
          <p className="line-clamp-2 text-sm font-medium text-white">{title}</p>
          <div className="mt-1 flex items-center gap-2 text-xs text-white/70">
            {year && <span>{year}</span>}
            {rating != null && (
              <span className="inline-flex items-center gap-1">
                <Star className="size-3 fill-amber-400 text-amber-400" />
                {rating.toFixed(1)}
              </span>
            )}
          </div>
        </div>
      </div>
    </Link>
  )
}
