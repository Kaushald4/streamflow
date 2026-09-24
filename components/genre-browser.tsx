"use client"

import * as React from "react"
import { Loader2 } from "lucide-react"
import { useInView } from "react-intersection-observer"
import { Button } from "@/components/ui/button"
import { MediaCard } from "@/components/media-card"
import { Skeleton } from "@/components/ui/skeleton"
import { readJsonResponse } from "@/lib/api/client"
import { mediaTitle, mediaYear, type DiscoverResult, type TmdbGenre, type TmdbMediaType, type TmdbSearchResult } from "@/lib/tmdb"

/** TMDB never reports more than 500 pages, regardless of a genre's real result count. */
const MAX_PAGE = 500

const STORAGE_KEY = "streamflow.genreBrowser"

type StoredState = {
  type: TmdbMediaType
  genreId: number | null
  items: TmdbSearchResult[]
  page: number
  totalPages: number
  scrollY: number
}

/** Only sessionStorage (not localStorage): this is "restore where I was in
 * this browsing session," not something that should persist across tabs or
 * days, a week-old cached result list would just be stale. */
function readStored(): StoredState | null {
  if (typeof window === "undefined") return null
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY)
    return raw ? (JSON.parse(raw) as StoredState) : null
  } catch {
    return null
  }
}

function writeStored(state: StoredState) {
  try {
    sessionStorage.setItem(STORAGE_KEY, JSON.stringify(state))
  } catch {
    // Storage full/disabled, filters just won't survive a back-navigation.
  }
}

type GenreBrowserProps = {
  movieGenres: TmdbGenre[]
  tvGenres: TmdbGenre[]
}

export function GenreBrowser({ movieGenres, tvGenres }: GenreBrowserProps) {
  // Read once, synchronously, before first render, restoring via a lazy
  // initializer (not a useEffect) means the previously-loaded cards are
  // already there on first paint, with no flash of empty state and no
  // wasted re-fetch of pages we already had.
  const [restored] = React.useState(() => readStored())
  const skipNextFetch = React.useRef(Boolean(restored && restored.items.length > 0))
  const pendingScrollY = React.useRef(restored?.scrollY ?? null)

  const [type, setType] = React.useState<TmdbMediaType>(restored?.type ?? "movie")
  const genres = type === "movie" ? movieGenres : tvGenres

  const [genreId, setGenreId] = React.useState<number | null>(restored?.genreId ?? movieGenres[0]?.id ?? null)
  const [items, setItems] = React.useState<TmdbSearchResult[]>(restored?.items ?? [])
  const [page, setPage] = React.useState(restored?.page ?? 1)
  const [totalPages, setTotalPages] = React.useState(restored?.totalPages ?? 1)
  const [loading, setLoading] = React.useState(false)

  const { ref: sentinelRef, inView } = useInView({ rootMargin: "600px" })

  function selectType(next: TmdbMediaType) {
    if (next === type) return
    setType(next)
    setGenreId((next === "movie" ? movieGenres : tvGenres)[0]?.id ?? null)
    setItems([])
    setPage(1)
    setTotalPages(1)
  }

  function selectGenre(id: number) {
    if (id === genreId) return
    setGenreId(id)
    setItems([])
    setPage(1)
    setTotalPages(1)
  }

  // Fetches whichever page is current for the active type/genre, appending
  // beyond page 1 (infinite scroll) or replacing on page 1 (fresh genre/type).
  React.useEffect(() => {
    if (!genreId) return
    if (skipNextFetch.current) {
      skipNextFetch.current = false
      return
    }

    let cancelled = false
    setLoading(true)

    fetch(`/api/tmdb/discover?type=${type}&genre=${genreId}&page=${page}`)
      .then((res) => readJsonResponse<DiscoverResult & { error?: string }>(res))
      .then((data) => {
        if (cancelled) return
        if ("error" in data && data.error) throw new Error(data.error)
        setItems((prev) => (page === 1 ? data.results : [...prev, ...data.results]))
        setTotalPages(data.totalPages)
      })
      .catch(() => {
        if (!cancelled && page === 1) setItems([])
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })

    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [type, genreId, page])

  const hasMore = page < Math.min(totalPages, MAX_PAGE)

  // Load the next page once the sentinel below the grid scrolls into view.
  React.useEffect(() => {
    if (inView && !loading && hasMore && items.length > 0) {
      setPage((p) => p + 1)
    }
  }, [inView, loading, hasMore, items.length])

  // Persist on every change so a Link click away (which unmounts this
  // component immediately, before any "leaving" event fires) never loses
  // the latest state.
  React.useEffect(() => {
    if (items.length === 0) return
    writeStored({ type, genreId, items, page, totalPages, scrollY: window.scrollY })
  }, [type, genreId, items, page, totalPages])

  // Scroll position itself changes far more often than the state above, so
  // it's tracked separately (also written into the same storage entry),
  // otherwise every scroll tick would re-serialize the entire item list.
  React.useEffect(() => {
    function onScroll() {
      const current = readStored()
      if (current) writeStored({ ...current, scrollY: window.scrollY })
    }
    window.addEventListener("scroll", onScroll, { passive: true })
    return () => window.removeEventListener("scroll", onScroll)
  }, [])

  // Restore scroll position once, after the restored cards have had a
  // chance to lay out (their aspect-ratio containers reserve height
  // immediately, before poster images load, so one frame is enough). Taking
  // manual control of scroll restoration first stops the browser's own
  // automatic attempt (which runs at an unpredictable time relative to this
  // component's async content) from fighting with this one.
  React.useEffect(() => {
    if ("scrollRestoration" in window.history) {
      window.history.scrollRestoration = "manual"
    }
    if (pendingScrollY.current == null) return
    const target = pendingScrollY.current
    pendingScrollY.current = null
    requestAnimationFrame(() => requestAnimationFrame(() => window.scrollTo(0, target)))
  }, [])

  return (
    <div className="space-y-6">
      <div className="flex gap-2">
        <Button variant={type === "movie" ? "default" : "outline"} onClick={() => selectType("movie")}>
          Movies
        </Button>
        <Button variant={type === "tv" ? "default" : "outline"} onClick={() => selectType("tv")}>
          TV Shows
        </Button>
      </div>

      <div className="flex flex-wrap gap-2">
        {genres.map((genre) => (
          <Button
            key={genre.id}
            size="sm"
            variant={genre.id === genreId ? "default" : "outline"}
            onClick={() => selectGenre(genre.id)}
          >
            {genre.name}
          </Button>
        ))}
      </div>

      {items.length === 0 && loading ? (
        <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6">
          {Array.from({ length: 12 }).map((_, i) => (
            <Skeleton key={i} className="aspect-2/3 w-full rounded-lg" />
          ))}
        </div>
      ) : items.length > 0 ? (
        <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6">
          {items.map((item, i) => (
            <MediaCard
              key={`${item.id}-${i}`}
              id={item.id}
              type={item.media_type}
              title={mediaTitle(item)}
              posterPath={item.poster_path}
              rating={item.vote_average}
              year={mediaYear(item)}
            />
          ))}
        </div>
      ) : (
        <p className="py-16 text-center text-sm text-muted-foreground">No results for this genre.</p>
      )}

      {items.length > 0 && (
        <div ref={sentinelRef} className="flex justify-center py-6 text-sm text-muted-foreground">
          {hasMore ? <Loader2 className="size-5 animate-spin" /> : "You've reached the end."}
        </div>
      )}
    </div>
  )
}
