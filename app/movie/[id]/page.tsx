import Image from "next/image"
import Link from "next/link"
import { notFound } from "next/navigation"
import { Clock, Globe, Play, Star } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { CastRow } from "@/components/tmdb/cast-row"
import { CrewGrid } from "@/components/tmdb/crew-grid"
import { MediaFacts, RecommendationsRow } from "@/components/tmdb/media-meta"
import { TrailerGrid } from "@/components/tmdb/trailer-grid"
import {
  getImdbId,
  getKeyCrew,
  getMovie,
  getTrailers,
  isTmdbNotFound,
} from "@/lib/tmdb"
import {
  formatCurrency,
  formatDate,
  formatRuntime,
  formatVoteCount,
} from "@/lib/tmdb-format"
import { tmdbBackdrop, tmdbPoster } from "@/lib/tmdb-image"

export default async function MoviePage({
  params,
}: {
  params: Promise<{ id: string }>
}) {
  const { id } = await params
  let movie
  try {
    movie = await getMovie(id)
  } catch (err) {
    if (isTmdbNotFound(err)) notFound()
    throw err
  }

  const poster = tmdbPoster(movie.poster_path)
  const backdrop = tmdbBackdrop(movie.backdrop_path)
  const year = movie.release_date?.slice(0, 4)
  const imdbId = getImdbId(movie)
  const trailers = getTrailers(movie.videos)
  const keyCrew = getKeyCrew(movie.credits?.crew)

  const facts = [
    { label: "Status", value: movie.status ?? "" },
    { label: "Release date", value: formatDate(movie.release_date) ?? "" },
    { label: "Runtime", value: formatRuntime(movie.runtime) ?? "" },
    { label: "Language", value: movie.original_language?.toUpperCase() ?? "" },
    {
      label: "Countries",
      value:
        movie.production_countries?.map((c) => c.name).join(", ") ?? "",
    },
    {
      label: "Studios",
      value:
        movie.production_companies?.slice(0, 3).map((c) => c.name).join(", ") ??
        "",
    },
    { label: "Budget", value: formatCurrency(movie.budget) ?? "" },
    { label: "Box office", value: formatCurrency(movie.revenue) ?? "" },
    {
      label: "Spoken languages",
      value:
        movie.spoken_languages?.map((l) => l.english_name).join(", ") ?? "",
    },
  ]

  return (
    <div className="space-y-10 pb-16">
      <section className="relative overflow-hidden rounded-2xl border border-border">
        {backdrop && (
          <Image src={backdrop} alt="" fill className="object-cover opacity-40" priority />
        )}
        <div className="absolute inset-0 bg-gradient-to-t from-background via-background/80 to-background/20" />
        <div className="relative grid gap-8 p-6 sm:p-8 lg:grid-cols-[220px_1fr] lg:p-10">
          <div className="relative mx-auto aspect-[2/3] w-full max-w-[220px] overflow-hidden rounded-lg border border-border shadow-lg">
            {poster ? (
              <Image
                src={poster}
                alt={movie.title}
                fill
                priority
                sizes="220px"
                className="object-cover"
              />
            ) : null}
          </div>
          <div className="space-y-5">
            <div className="flex flex-wrap gap-2">
              <Badge>Movie</Badge>
              {year && <Badge>{year}</Badge>}
              {movie.genres?.map((g) => (
                <Badge key={g.id} className="border-amber-500/25 bg-transparent">
                  {g.name}
                </Badge>
              ))}
            </div>
            <h1 className="font-heading text-4xl font-medium tracking-tight sm:text-5xl">{movie.title}</h1>
            {movie.tagline && (
              <p className="text-lg text-muted-foreground italic">{movie.tagline}</p>
            )}
            <div className="flex flex-wrap items-center gap-4 text-sm text-muted-foreground">
              <span className="inline-flex items-center gap-1">
                <Star className="size-4 fill-amber-400 text-amber-400" />
                {movie.vote_average.toFixed(1)}
                {movie.vote_count != null && (
                  <span className="text-muted-foreground/80">
                    ({formatVoteCount(movie.vote_count)} votes)
                  </span>
                )}
              </span>
              {movie.runtime != null && movie.runtime > 0 && (
                <span className="inline-flex items-center gap-1">
                  <Clock className="size-4" />
                  {formatRuntime(movie.runtime)}
                </span>
              )}
              {movie.original_language && (
                <span className="inline-flex items-center gap-1">
                  <Globe className="size-4" />
                  {movie.original_language.toUpperCase()}
                </span>
              )}
            </div>
            <p className="max-w-3xl text-base leading-relaxed text-muted-foreground">
              {movie.overview}
            </p>
            <div className="flex flex-wrap gap-3">
              <Link href={`/watch/movie/${movie.id}`}>
                <Button size="lg">
                  <Play data-icon="inline-start" className="fill-current" />
                  Start playback
                </Button>
              </Link>
              <Link href="/plugins">
                <Button size="lg" variant="outline">
                  Configure adapters
                </Button>
              </Link>
            </div>
            {imdbId && (
              <p className="text-xs text-muted-foreground">IMDb: {imdbId}</p>
            )}
          </div>
        </div>
      </section>

      <MediaFacts facts={facts} />

      <TrailerGrid trailers={trailers} />

      {movie.credits?.cast && <CastRow cast={movie.credits.cast} />}

      <CrewGrid crew={keyCrew} />

      {movie.recommendations?.results && (
        <RecommendationsRow
          items={movie.recommendations.results}
          mediaType="movie"
        />
      )}
    </div>
  )
}
