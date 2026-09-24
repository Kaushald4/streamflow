import type { Capabilities } from "stream-resolver"

export type PluginStatus = "stable" | "beta" | "experimental"
export type PluginSource = "builtin" | "external"

export type StreamPlugin = {
  id: string
  name: string
  summary: string
  description: string
  version: string
  author: string
  status: PluginStatus
  adapterClass: "site-adapter" | "network-adapter"
  transport: string
  decryption: string
  metadataBinding: string
  capabilities: Capabilities
  accent: string
  idHint: "imdb" | "tmdb" | "either"
  source: PluginSource
}

/** Placeholder until companion `/api/adapters` responds. */
export const BUILTIN_PLUGINS: StreamPlugin[] = []

/** Fresh installs start with nothing enabled, catalog comes from the companion. */
export const DEFAULT_INSTALLED_PLUGINS: string[] = []

const ID_HINT_LABEL: Record<StreamPlugin["idHint"], string> = {
  tmdb: "TMDB metadata required",
  imdb: "IMDb metadata required",
  either: "TMDB or IMDb metadata",
}

export function getPlugin(
  id: string,
  catalog: StreamPlugin[] = BUILTIN_PLUGINS,
): StreamPlugin | undefined {
  return catalog.find((p) => p.id === id)
}

export function getPluginLabel(
  id: string,
  catalog: StreamPlugin[] = BUILTIN_PLUGINS,
): string {
  return getPlugin(id, catalog)?.name ?? `Adapter ${id}`
}

export function getMetadataHintLabel(hint: StreamPlugin["idHint"]): string {
  return ID_HINT_LABEL[hint]
}

export function pluginsForMedia(
  catalog: StreamPlugin[],
  mediaType: "movie" | "episode",
  installed: string[],
): StreamPlugin[] {
  return catalog.filter((plugin) => {
    if (!installed.includes(plugin.id)) return false
    if (mediaType === "movie") return plugin.capabilities.movie
    return plugin.capabilities.episodes
  })
}
