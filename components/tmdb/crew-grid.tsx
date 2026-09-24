import Image from "next/image"
import type { TmdbCrewMember } from "@/lib/tmdb"
import { tmdbProfile } from "@/lib/tmdb-image"
import { DetailSection } from "@/components/tmdb/section"

type CrewGridProps = {
  crew: TmdbCrewMember[]
}

export function CrewGrid({ crew }: CrewGridProps) {
  if (crew.length === 0) return null

  return (
    <DetailSection title="Crew" subtitle="Key creative roles">
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
        {crew.map((person) => {
          const photo = tmdbProfile(person.profile_path)
          return (
            <div
              key={`${person.id}-${person.job}`}
              className="flex items-center gap-3 rounded-lg border border-border bg-card p-3"
            >
              <div className="relative size-12 shrink-0 overflow-hidden rounded-md border border-border bg-muted/20">
                {photo ? (
                  <Image
                    src={photo}
                    alt={person.name}
                    fill
                    sizes="48px"
                    className="object-cover"
                  />
                ) : (
                  <div className="flex h-full items-center justify-center text-[10px] text-muted-foreground">
                    ·
                  </div>
                )}
              </div>
              <div className="min-w-0">
                <p className="truncate text-sm font-medium">{person.name}</p>
                <p className="truncate text-xs text-amber-600 dark:text-amber-400">{person.job}</p>
              </div>
            </div>
          )
        })}
      </div>
    </DetailSection>
  )
}
