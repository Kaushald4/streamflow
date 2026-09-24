import type { Stream } from "stream-resolver"
import {
  buildSessionPlayPath,
  cleanStreamTargetUrl,
  type RegisterStreamBody,
  type StreamProxyHeaders,
} from "@/lib/stream/proxy"
import { companionFetch, companionUrl, getCompanionToken } from "@/lib/api/companion"

async function registerSession(
  target: string,
  headers: StreamProxyHeaders | undefined,
  signal?: AbortSignal,
): Promise<string | null> {
  const body: RegisterStreamBody = {
    target: cleanStreamTargetUrl(target),
    referer: headers?.referer,
    origin: headers?.origin,
    siteReferer: headers?.siteReferer,
    omitOrigin: headers?.omitOrigin,
  }

  const res = await companionFetch("/api/stream/register", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
    signal,
  })

  if (!res.ok) return null
  const data = (await res.json()) as { id?: string }
  return data.id ?? null
}

function sessionPlayUrl(sessionId: string, token: string): string {
  return companionUrl(buildSessionPlayPath(sessionId, token))
}

export type StreamPlayback = {
  url: string
  subtitles?: Stream["subtitles"]
}

/**
 * Prepares playback for *one* stream, at the moment it's chosen.
 *
 * Registering lazily rather than for every candidate up front means a slow or
 * dead source is never prepared, and switching sources becomes a single
 * cancellable request instead of a batch of them.
 */
export async function registerStreamPlayback(
  stream: Stream,
  signal?: AbortSignal,
): Promise<StreamPlayback | null> {
  const token = await getCompanionToken()
  // Without a companion there is nothing to proxy through, so hand back the
  // raw URL rather than blocking playback entirely.
  if (!token) return { url: stream.url, subtitles: stream.subtitles }

  const sessionId = await registerSession(stream.url, stream.headers, signal)
  if (!sessionId) return null

  const playUrl = sessionPlayUrl(sessionId, token)
  if (!stream.subtitles?.length) return { url: playUrl }

  const subtitles = await Promise.all(
    stream.subtitles.map(async (track) => {
      const subSession = await registerSession(track.url, stream.headers, signal)
      return {
        ...track,
        url: subSession ? sessionPlayUrl(subSession, token) : track.url,
      }
    }),
  )

  return { url: playUrl, subtitles }
}
