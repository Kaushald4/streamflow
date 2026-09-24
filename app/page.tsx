import * as React from "react"
import { HeroBanner } from "@/components/hero-banner"
import { HomeSections } from "@/components/home-sections"
import { Skeleton } from "@/components/ui/skeleton"
import {
  getAiringTodayTv,
  getOnTheAirTv,
  getPopularMovies,
  getPopularTv,
  getTopRatedMovies,
  getTopRatedTv,
  getTrending,
  type TmdbSearchResult,
} from "@/lib/tmdb"

type Rows = {
  trendingMovies: TmdbSearchResult[]
  trendingTv: TmdbSearchResult[]
  popularMovies: TmdbSearchResult[]
  popularTv: TmdbSearchResult[]
  topRatedMovies: TmdbSearchResult[]
  topRatedTv: TmdbSearchResult[]
  airingTodayTv: TmdbSearchResult[]
  onTheAirTv: TmdbSearchResult[]
}

const EMPTY_ROWS: Rows = {
  trendingMovies: [],
  trendingTv: [],
  popularMovies: [],
  popularTv: [],
  topRatedMovies: [],
  topRatedTv: [],
  airingTodayTv: [],
  onTheAirTv: [],
}

async function loadRows(): Promise<Rows> {
  const [trendingMoviesRes, trendingTvRes, popularMovies, popularTv, topRatedMovies, topRatedTv, airingTodayTv, onTheAirTv] =
    await Promise.all([
      getTrending("movie"),
      getTrending("tv"),
      getPopularMovies(),
      getPopularTv(),
      getTopRatedMovies(),
      getTopRatedTv(),
      getAiringTodayTv(),
      getOnTheAirTv(),
    ])

  return {
    trendingMovies: trendingMoviesRes.results.filter((r) => r.media_type === "movie"),
    trendingTv: trendingTvRes.results.filter((r) => r.media_type === "tv"),
    popularMovies,
    popularTv,
    topRatedMovies,
    topRatedTv,
    airingTodayTv,
    onTheAirTv,
  }
}

export default async function HomePage() {
  let rows = EMPTY_ROWS
  try {
    rows = await loadRows()
  } catch {
    // Render an empty state below rather than failing the whole page.
  }

  const featured = rows.trendingMovies[0] ?? rows.trendingTv[0]

  const movieSections = [
    { title: "Trending Movies", items: rows.trendingMovies.slice(0, 12) },
    { title: "Popular Movies", items: rows.popularMovies.slice(0, 12) },
    { title: "Top Rated Movies", items: rows.topRatedMovies.slice(0, 12) },
  ]
  const tvSections = [
    { title: "Trending TV", items: rows.trendingTv.slice(0, 12) },
    { title: "Popular TV", items: rows.popularTv.slice(0, 12) },
    { title: "Top Rated TV", items: rows.topRatedTv.slice(0, 12) },
    { title: "Airing Today", items: rows.airingTodayTv.slice(0, 12) },
    { title: "On The Air", items: rows.onTheAirTv.slice(0, 12) },
  ]

  return (
    <div className="space-y-12 pb-16">
      {featured && <HeroBanner item={featured} />}

      <React.Suspense
        fallback={
          <div className="space-y-10">
            <Skeleton className="h-8 w-48" />
            <Skeleton className="h-[220px] w-full rounded-xl" />
          </div>
        }
      >
        <HomeSections movieSections={movieSections} tvSections={tvSections} />
      </React.Suspense>
    </div>
  )
}
