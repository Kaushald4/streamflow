import Image from "next/image"
import Link from "next/link"
import { notFound } from "next/navigation"
import { Calendar, Globe, Play, Star, Tv } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { CastRow } from "@/components/tmdb/cast-row"
import { CrewGrid } from "@/components/tmdb/crew-grid"
import { EpisodeCard } from "@/components/tmdb/episode-card"
import { MediaFacts, RecommendationsRow, SeasonTabs } from "@/components/tmdb/media-meta"
import { DetailSection } from "@/components/tmdb/section"
import { TrailerGrid } from "@/components/tmdb/trailer-grid"
import {
  getImdbId,
  getKeyCrew,
  getSeason,
  getTrailers,
  getTvShow,
  isTmdbNotFound,
} from "@/lib/tmdb"
import { formatDate, formatVoteCount } from "@/lib/tmdb-format"
import { tmdbBackdrop, tmdbPoster } from "@/lib/tmdb-image"

export default async function TvPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>
  searchParams: Promise<{ season?: string }>
}) {
  const { id } = await params
  const { season: seasonParam } = await searchParams

  let show
  try {
    show = await getTvShow(id)
  } catch (err) {
    if (isTmdbNotFound(err)) notFound()
    throw err
  }

  const regularSeasons = show.seasons.filter((s) => s.season_number > 0)
  const seasonNumber = Number(seasonParam ?? regularSeasons[0]?.season_number ?? 1)
  const seasonData = await getSeason(id, seasonNumber).catch((err) => {
    if (isTmdbNotFound(err)) return null
    throw err
  })

  const seasonMeta = regularSeasons.find((s) => s.season_number === seasonNumber)
  const poster = tmdbPoster(show.poster_path)
  const backdrop = tmdbBackdrop(show.backdrop_path)
  const imdbId = getImdbId(show)
  const trailers = getTrailers(show.videos)
  const keyCrew = getKeyCrew(show.credits?.crew)
  const creators =
    show.created_by?.map((c) => c.name).join(", ") ??
    keyCrew
      .filter((c) => c.job === "Creator")
      .map((c) => c.name)
      .join(", ")

  const facts = [
    { label: "Status", value: show.status ?? "" },
    { label: "First aired", value: formatDate(show.first_air_date) ?? "" },
    { label: "Last aired", value: formatDate(show.last_air_date) ?? "" },
    { label: "Seasons", value: String(show.number_of_seasons ?? "") },
    {
      label: "Episodes",
      value: show.number_of_episodes ? String(show.number_of_episodes) : "",
    },
    { label: "Language", value: show.original_language?.toUpperCase() ?? "" },
    { label: "Network", value: show.networks?.map((n) => n.name).join(", ") ?? "" },
    { label: "Created by", value: creators },
    {
      label: "Countries",
      value:
        show.production_countries?.map((c) => c.name).join(", ") ?? "",
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
                alt={show.name}
                fill
                priority
                sizes="220px"
                className="object-cover"
              />
            ) : null}
          </div>
          <div className="space-y-5">
            <div className="flex flex-wrap gap-2">
              <Badge>TV Series</Badge>
              <Badge>{show.number_of_seasons} seasons</Badge>
              {show.genres?.map((g) => (
                <Badge key={g.id} className="border-amber-500/25 bg-transparent">
                  {g.name}
                </Badge>
              ))}
            </div>
            <h1 className="font-heading text-4xl font-medium tracking-tight sm:text-5xl">{show.name}</h1>
            {show.tagline && (
              <p className="text-lg text-muted-foreground italic">{show.tagline}</p>
            )}
            <div className="flex flex-wrap items-center gap-4 text-sm text-muted-foreground">
              <span className="inline-flex items-center gap-1">
                <Star className="size-4 fill-amber-400 text-amber-400" />
                {show.vote_average.toFixed(1)}
                {show.vote_count != null && (
                  <span className="text-muted-foreground/80">
                    ({formatVoteCount(show.vote_count)} votes)
                  </span>
                )}
              </span>
              {show.first_air_date && (
                <span className="inline-flex items-center gap-1">
                  <Calendar className="size-4" />
                  {formatDate(show.first_air_date)}
                </span>
              )}
              {show.original_language && (
                <span className="inline-flex items-center gap-1">
                  <Globe className="size-4" />
                  {show.original_language.toUpperCase()}
                </span>
              )}
              {show.status && (
                <span className="inline-flex items-center gap-1">
                  <Tv className="size-4" />
                  {show.status}
                </span>
              )}
            </div>
            <p className="max-w-3xl text-base leading-relaxed text-muted-foreground">
              {show.overview}
            </p>
            <div className="flex flex-wrap gap-3">
              {seasonData?.episodes[0] && (
                <Link
                  href={`/watch/tv/${id}/${seasonNumber}/${seasonData.episodes[0].episode_number}`}
                >
                  <Button size="lg">
                    <Play data-icon="inline-start" className="fill-current" />
                    Watch S{seasonNumber}E1
                  </Button>
                </Link>
              )}
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

      <DetailSection
        title="Episodes"
        subtitle={
          seasonMeta
            ? `${seasonMeta.name} · ${seasonMeta.episode_count} episodes`
            : `Season ${seasonNumber}`
        }
      >
        <SeasonTabs
          tvId={id}
          seasons={regularSeasons}
          activeSeason={seasonNumber}
        />
        <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
          {seasonData?.episodes.map((ep) => (
            <EpisodeCard
              key={ep.id}
              episode={ep}
              tvId={id}
              seasonNumber={seasonNumber}
            />
          )) ?? (
            <p className="text-sm text-muted-foreground">
              No episodes found for this season.
            </p>
          )}
        </div>
      </DetailSection>

      {show.credits?.cast && <CastRow cast={show.credits.cast} />}

      <CrewGrid crew={keyCrew} />

      {show.recommendations?.results && (
        <RecommendationsRow
          items={show.recommendations.results}
          mediaType="tv"
        />
      )}
    </div>
  )
}
