const TMDB_BASE = "https://api.themoviedb.org/3"

export type TmdbMediaType = "movie" | "tv"

export type TmdbGenre = { id: number; name: string }

export type TmdbProductionCompany = {
  id: number
  name: string
  logo_path: string | null
  origin_country: string
}

export type TmdbNetwork = {
  id: number
  name: string
  logo_path: string | null
  origin_country: string
}

export type TmdbCastMember = {
  id: number
  name: string
  character: string
  profile_path: string | null
  order: number
}

export type TmdbCrewMember = {
  id: number
  name: string
  job: string
  department: string
  profile_path: string | null
}

export type TmdbCredits = {
  cast: TmdbCastMember[]
  crew: TmdbCrewMember[]
}

export type TmdbVideo = {
  id: string
  key: string
  name: string
  site: string
  type: string
  official: boolean
  published_at?: string
}

export type TmdbVideos = {
  results: TmdbVideo[]
}

export type TmdbRecommendation = {
  id: number
  title?: string
  name?: string
  poster_path: string | null
  backdrop_path: string | null
  vote_average: number
  media_type?: TmdbMediaType
}

export type TmdbRecommendations = {
  results: TmdbRecommendation[]
}

export type TmdbExternalIds = {
  imdb_id?: string | null
  tvdb_id?: number | null
}

export type TmdbMovie = {
  id: number
  title: string
  overview: string
  poster_path: string | null
  backdrop_path: string | null
  release_date: string
  vote_average: number
  vote_count?: number
  popularity?: number
  runtime?: number
  status?: string
  tagline?: string
  original_language?: string
  spoken_languages?: { iso_639_1: string; english_name: string }[]
  production_countries?: { iso_3166_1: string; name: string }[]
  production_companies?: TmdbProductionCompany[]
  genres?: TmdbGenre[]
  budget?: number
  revenue?: number
  imdb_id?: string
  external_ids?: TmdbExternalIds
  credits?: TmdbCredits
  videos?: TmdbVideos
  recommendations?: TmdbRecommendations
}

export type TmdbTvShow = {
  id: number
  name: string
  overview: string
  poster_path: string | null
  backdrop_path: string | null
  first_air_date: string
  last_air_date?: string
  vote_average: number
  vote_count?: number
  popularity?: number
  number_of_seasons: number
  number_of_episodes?: number
  status?: string
  tagline?: string
  original_language?: string
  spoken_languages?: { iso_639_1: string; english_name: string }[]
  production_countries?: { iso_3166_1: string; name: string }[]
  networks?: TmdbNetwork[]
  genres?: TmdbGenre[]
  seasons: TmdbSeason[]
  created_by?: { id: number; name: string; profile_path: string | null }[]
  external_ids?: TmdbExternalIds
  credits?: TmdbCredits
  videos?: TmdbVideos
  recommendations?: TmdbRecommendations
}

export type TmdbSeason = {
  id: number
  season_number: number
  name: string
  episode_count: number
  poster_path: string | null
  overview: string
  air_date?: string
}

export type TmdbEpisode = {
  id: number
  name: string
  overview: string
  still_path: string | null
  episode_number: number
  season_number: number
  runtime: number | null
  air_date: string
  vote_average?: number
}

export type TmdbSearchResult = {
  id: number
  media_type: TmdbMediaType
  title?: string
  name?: string
  overview: string
  poster_path: string | null
  backdrop_path: string | null
  release_date?: string
  first_air_date?: string
  vote_average: number
}

const DETAIL_APPEND = "external_ids,credits,videos,recommendations"
const TMDB_MAX_ATTEMPTS = 3
const TMDB_RETRY_BASE_MS = 250

export class TmdbError extends Error {
  readonly status: number
  readonly path: string

  constructor(path: string, status: number, message?: string) {
    super(message ?? `TMDB ${path} failed: ${status}`)
    this.name = "TmdbError"
    this.path = path
    this.status = status
  }
}

export function isTmdbNotFound(err: unknown): boolean {
  return err instanceof TmdbError && err.status === 404
}

function getApiKey(): string {
  const key =
    process.env.TMDB_API_KEY ?? process.env.NEXT_PUBLIC_TMDB_API_KEY
  if (!key) {
    throw new Error("TMDB API key is missing. Add NEXT_PUBLIC_TMDB_API_KEY to .env")
  }
  return key
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

function shouldRetryTmdb(status: number): boolean {
  return status === 429 || status >= 500
}

async function tmdbFetch<T>(path: string, params: Record<string, string> = {}): Promise<T> {
  const url = new URL(`${TMDB_BASE}${path}`)
  url.searchParams.set("api_key", getApiKey())
  for (const [key, value] of Object.entries(params)) {
    url.searchParams.set(key, value)
  }

  let lastError: unknown
  for (let attempt = 1; attempt <= TMDB_MAX_ATTEMPTS; attempt++) {
    try {
      const res = await fetch(url.toString(), {
        next: { revalidate: 3600 },
      })

      if (res.ok) {
        return res.json() as Promise<T>
      }

      const error = new TmdbError(path, res.status)
      if (!shouldRetryTmdb(res.status) || attempt === TMDB_MAX_ATTEMPTS) {
        throw error
      }
      lastError = error
    } catch (err) {
      lastError = err
      // Network / abort: retry. Real HTTP errors already handled above.
      if (err instanceof TmdbError) {
        if (!shouldRetryTmdb(err.status) || attempt === TMDB_MAX_ATTEMPTS) {
          throw err
        }
      } else if (attempt === TMDB_MAX_ATTEMPTS) {
        throw err
      }
    }

    await sleep(TMDB_RETRY_BASE_MS * attempt)
  }

  throw lastError instanceof Error
    ? lastError
    : new TmdbError(path, 500, "TMDB request failed")
}

export async function getTrending(
  mediaType: "movie" | "tv" | "all" = "all",
  timeWindow: "day" | "week" = "week",
) {
  return tmdbFetch<{ results: TmdbSearchResult[] }>(
    `/trending/${mediaType}/${timeWindow}`,
  )
}

/**
 * Single-media-type list endpoints (popular/top-rated/upcoming/airing) don't
 * include `media_type` in their results the way /trending and /search/multi
 * do, MediaRow/MediaCard/HeroBanner all key off it for routing, so it's
 * stamped on here.
 */
async function tmdbListWithType(
  path: string,
  mediaType: TmdbMediaType,
): Promise<TmdbSearchResult[]> {
  const res = await tmdbFetch<{ results: Omit<TmdbSearchResult, "media_type">[] }>(path)
  return res.results.map((r) => ({ ...r, media_type: mediaType }))
}

export async function getPopularMovies() {
  return tmdbListWithType("/movie/popular", "movie")
}

export async function getPopularTv() {
  return tmdbListWithType("/tv/popular", "tv")
}

export async function getTopRatedMovies() {
  return tmdbListWithType("/movie/top_rated", "movie")
}

export async function getTopRatedTv() {
  return tmdbListWithType("/tv/top_rated", "tv")
}

export async function getAiringTodayTv() {
  return tmdbListWithType("/tv/airing_today", "tv")
}

export async function getOnTheAirTv() {
  return tmdbListWithType("/tv/on_the_air", "tv")
}

export async function getGenres(mediaType: TmdbMediaType) {
  const res = await tmdbFetch<{ genres: TmdbGenre[] }>(`/genre/${mediaType}/list`)
  return res.genres
}

export type DiscoverResult = {
  results: TmdbSearchResult[]
  page: number
  totalPages: number
}

export async function discoverByGenre(
  mediaType: TmdbMediaType,
  genreId: number,
  page = 1,
): Promise<DiscoverResult> {
  const res = await tmdbFetch<{
    results: Omit<TmdbSearchResult, "media_type">[]
    page: number
    total_pages: number
  }>(`/discover/${mediaType}`, {
    with_genres: String(genreId),
    page: String(page),
    sort_by: "popularity.desc",
  })
  return {
    results: res.results.map((r) => ({ ...r, media_type: mediaType })),
    page: res.page,
    totalPages: res.total_pages,
  }
}

export async function searchMulti(query: string) {
  if (!query.trim()) return { results: [] as TmdbSearchResult[] }
  return tmdbFetch<{ results: TmdbSearchResult[] }>("/search/multi", {
    query: query.trim(),
    include_adult: "false",
  })
}

export async function getMovie(id: string) {
  return tmdbFetch<TmdbMovie>(`/movie/${id}`, {
    append_to_response: DETAIL_APPEND,
  })
}

export async function getTvShow(id: string) {
  return tmdbFetch<TmdbTvShow>(`/tv/${id}`, {
    append_to_response: DETAIL_APPEND,
  })
}

export async function getSeason(id: string, season: number) {
  return tmdbFetch<{ episodes: TmdbEpisode[]; name: string; overview: string }>(
    `/tv/${id}/season/${season}`,
  )
}

export function mediaTitle(item: TmdbSearchResult | TmdbRecommendation): string {
  return item.title ?? item.name ?? "Untitled"
}

export function mediaYear(item: TmdbSearchResult | TmdbMovie | TmdbTvShow): string {
  const date =
    "release_date" in item
      ? item.release_date
      : "first_air_date" in item
        ? item.first_air_date
        : ""
  return date ? date.slice(0, 4) : ""
}

export function getTrailers(videos?: TmdbVideos): TmdbVideo[] {
  if (!videos?.results.length) return []
  return videos.results
    .filter((v) => v.site === "YouTube" && (v.type === "Trailer" || v.type === "Teaser"))
    .sort((a, b) => {
      if (a.official !== b.official) return a.official ? -1 : 1
      if (a.type === "Trailer" && b.type !== "Trailer") return -1
      if (b.type === "Trailer" && a.type !== "Trailer") return 1
      return 0
    })
}

const KEY_CREW_JOBS = new Set([
  "Director",
  "Writer",
  "Screenplay",
  "Story",
  "Producer",
  "Executive Producer",
  "Director of Photography",
  "Original Music Composer",
  "Editor",
  "Creator",
])

export function getKeyCrew(crew: TmdbCrewMember[] = []): TmdbCrewMember[] {
  const seen = new Set<string>()
  const result: TmdbCrewMember[] = []
  for (const member of crew) {
    if (!KEY_CREW_JOBS.has(member.job)) continue
    const key = `${member.id}-${member.job}`
    if (seen.has(key)) continue
    seen.add(key)
    result.push(member)
  }
  return result.slice(0, 12)
}

export function getImdbId(
  item: { imdb_id?: string; external_ids?: TmdbExternalIds },
): string | undefined {
  return item.external_ids?.imdb_id ?? item.imdb_id ?? undefined
}
