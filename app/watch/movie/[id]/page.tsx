import { notFound } from "next/navigation"
import { WatchPanel } from "@/components/watch-panel"
import { getMovie, isTmdbNotFound } from "@/lib/tmdb"
import { tmdbPoster } from "@/lib/tmdb-image"

export default async function WatchMoviePage({
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

  const imdbId =
    (movie as { external_ids?: { imdb_id?: string } }).external_ids?.imdb_id ??
    movie.imdb_id

  return (
    <WatchPanel
      key={`movie-${movie.id}`}
      kind="movie"
      tmdbId={movie.id}
      imdbId={imdbId}
      title={movie.title}
      poster={tmdbPoster(movie.poster_path)}
    />
  )
}
