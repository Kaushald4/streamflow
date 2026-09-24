"use client"

import { useSearchParams } from "next/navigation"
import { MediaRow } from "@/components/media-row"
import type { TmdbSearchResult } from "@/lib/tmdb"

type Section = { title: string; items: TmdbSearchResult[] }

type HomeSectionsProps = {
  movieSections: Section[]
  tvSections: Section[]
}

/**
 * Ordering movie/TV sections by the `?tab=` nav is a pure client-side
 * concern, both sets are already fetched server-side, so there's no need
 * to round-trip to the server just to reorder them. Reading the tab via
 * `useSearchParams()` here (rather than relying on the server component
 * re-rendering when the nav `<Link>` changes the URL) sidesteps Next's
 * client router cache reusing the previous render for what it considers
 * the "same" page.
 */
export function HomeSections({ movieSections, tvSections }: HomeSectionsProps) {
  const searchParams = useSearchParams()
  const showTvFirst = searchParams.get("tab") === "tv"
  const ordered = showTvFirst ? [...tvSections, ...movieSections] : [...movieSections, ...tvSections]

  return (
    <div className="space-y-10">
      {ordered.map((section) => (
        <MediaRow key={section.title} title={section.title} items={section.items} />
      ))}
    </div>
  )
}
