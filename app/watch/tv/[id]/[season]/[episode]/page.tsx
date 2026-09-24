import { notFound } from "next/navigation"
import { WatchPanel } from "@/components/watch-panel"
import { getSeason, getTvShow, isTmdbNotFound } from "@/lib/tmdb"
import { tmdbPoster } from "@/lib/tmdb-image"

export default async function WatchTvPage({
  params,
}: {
  params: Promise<{ id: string; season: string; episode: string }>
}) {
  const { id, season, episode } = await params

  let show
  let seasonData
  try {
    ;[show, seasonData] = await Promise.all([
      getTvShow(id),
      getSeason(id, Number(season)),
    ])
  } catch (err) {
    if (isTmdbNotFound(err)) notFound()
    throw err
  }

  const episodeData = seasonData.episodes.find(
    (e) => e.episode_number === Number(episode),
  )
  if (!episodeData) notFound()

  return (
    <WatchPanel
      key={`tv-${id}-${season}-${episode}`}
      kind="episode"
      tmdbId={show.id}
      season={Number(season)}
      episode={Number(episode)}
      title={`${show.name} · S${season}E${episode} · ${episodeData.name}`}
      poster={tmdbPoster(episodeData.still_path ?? show.poster_path)}
    />
  )
}
