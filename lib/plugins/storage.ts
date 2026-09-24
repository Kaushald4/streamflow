export const INSTALLED_PLUGINS_KEY = "streamflow:installed-plugins"

export function readInstalledPlugins(): string[] {
  if (typeof window === "undefined") return []
  try {
    const raw = localStorage.getItem(INSTALLED_PLUGINS_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw) as unknown
    return Array.isArray(parsed)
      ? parsed.filter((id): id is string => typeof id === "string")
      : []
  } catch {
    return []
  }
}

export function writeInstalledPlugins(ids: string[]): void {
  localStorage.setItem(INSTALLED_PLUGINS_KEY, JSON.stringify(ids))
}
