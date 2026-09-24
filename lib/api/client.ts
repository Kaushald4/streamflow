/** Parse JSON bodies without Safari's opaque "expected pattern" SyntaxError. */
export async function readJsonResponse<T>(res: Response): Promise<T> {
  const text = await res.text()
  if (!text.trim()) {
    if (!res.ok) throw new Error(`Request failed (${res.status})`)
    return {} as T
  }

  try {
    return JSON.parse(text) as T
  } catch {
    throw new Error(
      res.ok
        ? "Server returned an invalid response"
        : `Request failed (${res.status})`,
    )
  }
}
