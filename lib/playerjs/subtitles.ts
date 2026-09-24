import { subtitleDisplayLabel } from "@/lib/subtitles/labels"

type SubtitleLike = {
  url: string
  language?: string
  label?: string
}

function absolutizeUrl(url: string, origin?: string): string {
  if (url.startsWith("http://") || url.startsWith("https://")) return url
  const base =
    origin ?? (typeof window !== "undefined" ? window.location.origin : "")
  if (!base) return url
  return `${base}${url.startsWith("/") ? url : `/${url}`}`
}

/** PlayerJS format: [English]url,[Spanish]url */
export function formatPlayerjsSubtitles(
  subtitles?: SubtitleLike[],
  options?: { origin?: string; defaultLabel?: string },
): { tracks: string | undefined; defaultLabel: string | undefined } {
  if (!subtitles?.length) {
    return { tracks: undefined, defaultLabel: undefined }
  }

  const labeled = subtitles.map((track) => ({
    ...track,
    displayLabel: subtitleDisplayLabel(track),
    url: absolutizeUrl(track.url, options?.origin),
  }))

  const defaultLabel =
    options?.defaultLabel &&
    labeled.some((t) => t.displayLabel === options.defaultLabel)
      ? options.defaultLabel
      : labeled[0]?.displayLabel

  const tracks = labeled
    .map((track) => `[${track.displayLabel}]${track.url}`)
    .join(",")

  return { tracks, defaultLabel }
}
