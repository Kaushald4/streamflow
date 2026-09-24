export type ImageSize = "w92" | "w154" | "w185" | "w300" | "w342" | "w500" | "w780" | "original"

export function tmdbImage(
  path: string | null | undefined,
  size: ImageSize = "w500",
): string | null {
  if (!path) return null
  return `https://image.tmdb.org/t/p/${size}${path}`
}

export function tmdbBackdrop(path: string | null | undefined): string | null {
  return tmdbImage(path, "w780")
}

export function tmdbPoster(path: string | null | undefined): string | null {
  return tmdbImage(path, "w342")
}

export function tmdbProfile(path: string | null | undefined): string | null {
  return tmdbImage(path, "w185")
}

export function tmdbStill(path: string | null | undefined): string | null {
  return tmdbImage(path, "w300")
}
