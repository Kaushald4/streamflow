import Image from "next/image"
import type { TmdbCastMember } from "@/lib/tmdb"
import { tmdbProfile } from "@/lib/tmdb-image"
import { DetailSection } from "@/components/tmdb/section"

type CastRowProps = {
  cast: TmdbCastMember[]
  limit?: number
}

export function CastRow({ cast, limit = 18 }: CastRowProps) {
  const members = cast.slice(0, limit)
  if (members.length === 0) return null

  return (
    <DetailSection title="Cast" subtitle={`Top ${members.length} billed actors`}>
      <div className="flex gap-4 overflow-x-auto pb-2 [-ms-overflow-style:none] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        {members.map((person) => {
          const photo = tmdbProfile(person.profile_path)
          return (
            <div
              key={`${person.id}-${person.order}`}
              className="w-28 shrink-0 space-y-2 text-center sm:w-32"
            >
              <div className="relative mx-auto aspect-[2/3] w-full overflow-hidden rounded-lg border border-border bg-card">
                {photo ? (
                  <Image
                    src={photo}
                    alt={person.name}
                    fill
                    sizes="128px"
                    className="object-cover"
                  />
                ) : (
                  <div className="flex h-full items-center justify-center text-xs text-muted-foreground">
                    No photo
                  </div>
                )}
              </div>
              <div>
                <p className="line-clamp-2 text-sm font-medium leading-tight">
                  {person.name}
                </p>
                <p className="mt-0.5 line-clamp-2 text-xs text-muted-foreground">
                  {person.character}
                </p>
              </div>
            </div>
          )
        })}
      </div>
    </DetailSection>
  )
}
