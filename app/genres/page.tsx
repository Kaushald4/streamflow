import { Compass } from "lucide-react"
import { Badge } from "@/components/ui/badge"
import { GenreBrowser } from "@/components/genre-browser"
import { getGenres } from "@/lib/tmdb"

export default async function GenresPage() {
  const [movieGenres, tvGenres] = await Promise.all([
    getGenres("movie").catch(() => []),
    getGenres("tv").catch(() => []),
  ])

  return (
    <div className="space-y-8 pb-16">
      <section className="rounded-2xl border border-border bg-card p-8 sm:p-10">
        <div className="mb-3 flex flex-wrap items-center gap-2">
          <Compass className="size-5 text-amber-500" />
          <Badge>TMDB genres</Badge>
        </div>
        <h1 className="font-heading text-3xl font-medium tracking-tight sm:text-4xl">Browse by genre</h1>
        <p className="mt-3 max-w-2xl leading-relaxed text-muted-foreground">
          Pick a genre to explore movies and TV shows, sorted by popularity.
        </p>
      </section>

      <GenreBrowser movieGenres={movieGenres} tvGenres={tvGenres} />
    </div>
  )
}
