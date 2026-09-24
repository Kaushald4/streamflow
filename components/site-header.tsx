"use client"

import * as React from "react"
import Image from "next/image"
import Link from "next/link"
import { useRouter } from "next/navigation"
import { useTheme } from "next-themes"
import {
  Compass,
  Film,
  Moon,
  Package,
  Play,
  Search,
  Star,
  Sun,
  Tv,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { tmdbPoster } from "@/lib/tmdb-image"
import { cn } from "@/lib/utils"

type SearchResult = {
  id: number
  media_type: string
  title?: string
  name?: string
  poster_path?: string | null
  release_date?: string
  first_air_date?: string
  vote_average?: number
}

export function SiteHeader() {
  const router = useRouter()
  const { resolvedTheme, setTheme } = useTheme()
  const [query, setQuery] = React.useState("")
  const [open, setOpen] = React.useState(false)
  const [results, setResults] = React.useState<SearchResult[]>([])

  React.useEffect(() => {
    if (!query.trim()) {
      setResults([])
      return
    }

    const timer = setTimeout(async () => {
      const res = await fetch(`/api/tmdb/search?q=${encodeURIComponent(query)}`)
      const data = await res.json()
      setResults(
        (data.results ?? []).filter(
          (item: SearchResult) =>
            item.media_type === "movie" || item.media_type === "tv",
        ),
      )
    }, 300)

    return () => clearTimeout(timer)
  }, [query])

  function goToResult(item: SearchResult) {
    const href =
      item.media_type === "movie" ? `/movie/${item.id}` : `/tv/${item.id}`
    setOpen(false)
    setQuery("")
    router.push(href)
  }

  return (
    <header className="sticky top-0 z-50 border-b border-border bg-background/80 backdrop-blur-xl">
      <div className="mx-auto flex h-16 max-w-7xl items-center gap-4 px-4 sm:px-6">
        <Link href="/" className="flex items-center gap-2.5 font-heading text-lg font-semibold tracking-tight">
          <span className="flex size-8 items-center justify-center rounded-md bg-foreground text-background">
            <Play className="size-4 fill-current" />
          </span>
          <span className="hidden sm:inline">Streamflow</span>
        </Link>

        <nav className="hidden items-center gap-1 md:flex">
          <Link href="/">
            <Button variant="ghost" size="sm">
              <Film data-icon="inline-start" />
              Movies
            </Button>
          </Link>
          <Link href="/?tab=tv">
            <Button variant="ghost" size="sm">
              <Tv data-icon="inline-start" />
              TV
            </Button>
          </Link>
          <Link href="/genres">
            <Button variant="ghost" size="sm">
              <Compass data-icon="inline-start" />
              Genres
            </Button>
          </Link>
          <Link href="/plugins">
            <Button variant="ghost" size="sm">
              <Package data-icon="inline-start" />
              Adapters
            </Button>
          </Link>
        </nav>

        <div className="relative ml-auto flex-1 max-w-md">
          <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={query}
            onChange={(e) => {
              setQuery(e.target.value)
              setOpen(true)
            }}
            onFocus={() => setOpen(true)}
            placeholder="Search movies & shows..."
            className="pl-10"
          />
          {open && results.length > 0 && (
            <div className="absolute top-full right-0 left-0 mt-2 overflow-hidden rounded-lg border border-border bg-popover/95 shadow-lg backdrop-blur-xl">
              {results.slice(0, 8).map((item) => {
                const title = item.title ?? item.name ?? "Untitled"
                const poster = tmdbPoster(item.poster_path)
                const year = (item.release_date ?? item.first_air_date)?.slice(0, 4)
                return (
                  <button
                    key={`${item.media_type}-${item.id}`}
                    type="button"
                    onClick={() => goToResult(item)}
                    className="flex w-full items-center gap-3 px-3 py-2.5 text-left text-sm transition hover:bg-accent"
                  >
                    <div className="relative size-10 shrink-0 overflow-hidden rounded-md border border-border bg-muted/20">
                      {poster ? (
                        <Image
                          src={poster}
                          alt=""
                          fill
                          sizes="40px"
                          className="object-cover"
                        />
                      ) : item.media_type === "movie" ? (
                        <Film className="absolute inset-0 m-auto size-4 text-muted-foreground" />
                      ) : (
                        <Tv className="absolute inset-0 m-auto size-4 text-muted-foreground" />
                      )}
                    </div>
                    <div className="min-w-0 flex-1">
                      <p className="truncate font-medium">{title}</p>
                      <p className="flex items-center gap-2 text-xs text-muted-foreground">
                        <span>{item.media_type === "movie" ? "Movie" : "TV"}</span>
                        {year && <span>{year}</span>}
                        {item.vote_average != null && item.vote_average > 0 && (
                          <span className="inline-flex items-center gap-0.5">
                            <Star className="size-3 fill-amber-400 text-amber-400" />
                            {item.vote_average.toFixed(1)}
                          </span>
                        )}
                      </p>
                    </div>
                  </button>
                )
              })}
            </div>
          )}
        </div>

        <Button
          variant="ghost"
          size="icon"
          onClick={() => setTheme(resolvedTheme === "dark" ? "light" : "dark")}
          aria-label="Toggle theme"
        >
          <Sun className="size-4 dark:hidden" />
          <Moon className="hidden size-4 dark:inline" />
        </Button>
      </div>
      {open && (
        <button
          type="button"
          aria-label="Close search"
          className={cn("fixed inset-0 z-[-1]")}
          onClick={() => setOpen(false)}
        />
      )}
    </header>
  )
}
