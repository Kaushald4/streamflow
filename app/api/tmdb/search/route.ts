import { NextRequest, NextResponse } from "next/server"
import { searchMulti } from "@/lib/tmdb"

export async function GET(request: NextRequest) {
  const q = request.nextUrl.searchParams.get("q") ?? ""
  try {
    const data = await searchMulti(q)
    return NextResponse.json(data)
  } catch (err) {
    return NextResponse.json(
      { error: err instanceof Error ? err.message : "Search failed" },
      { status: 500 },
    )
  }
}
