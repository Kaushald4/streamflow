import { NextRequest, NextResponse } from "next/server"
import { discoverByGenre } from "@/lib/tmdb"

export async function GET(request: NextRequest) {
  const params = request.nextUrl.searchParams
  const type = params.get("type") === "tv" ? "tv" : "movie"
  const genre = Number(params.get("genre"))
  const page = Math.max(1, Number(params.get("page")) || 1)

  if (!genre) {
    return NextResponse.json({ error: "Missing or invalid genre id" }, { status: 400 })
  }

  try {
    const data = await discoverByGenre(type, genre, page)
    return NextResponse.json(data)
  } catch (err) {
    return NextResponse.json(
      { error: err instanceof Error ? err.message : "Failed to load results" },
      { status: 500 },
    )
  }
}
