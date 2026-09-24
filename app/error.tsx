"use client"

import { useEffect } from "react"
import { Button } from "@/components/ui/button"

export default function AppError({
  error,
  reset,
}: {
  error: Error & { digest?: string }
  reset: () => void
}) {
  useEffect(() => {
    console.error(error)
  }, [error])

  return (
    <div className="mx-auto flex min-h-[50vh] max-w-lg flex-col items-center justify-center gap-4 px-6 text-center">
      <h1 className="font-heading text-2xl font-medium tracking-tight">
        Something went wrong
      </h1>
      <p className="text-sm text-muted-foreground">
        Catalog data failed to load. This is usually temporary. Try again.
      </p>
      <Button onClick={reset}>Try again</Button>
    </div>
  )
}
