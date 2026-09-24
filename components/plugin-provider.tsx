"use client"

import * as React from "react"
import {
  readInstalledPlugins,
  writeInstalledPlugins,
} from "@/lib/plugins/storage"
import { companionUrl } from "@/lib/api/companion"
import { readJsonResponse } from "@/lib/api/client"
import {
  BUILTIN_PLUGINS,
  DEFAULT_INSTALLED_PLUGINS,
  type StreamPlugin,
} from "@/lib/plugins/catalog"

const COMPANION_POLL_MS = 5_000

type PluginContextValue = {
  installed: string[]
  catalog: StreamPlugin[]
  ready: boolean
  /** Whether the local Streamflow Companion answered the last catalog fetch. */
  companionAvailable: boolean
  install: (id: string) => void
  uninstall: (id: string) => void
  toggle: (id: string) => void
  isInstalled: (id: string) => boolean
  refreshCatalog: () => Promise<void>
}

const PluginContext = React.createContext<PluginContextValue | null>(null)

export function PluginProvider({ children }: { children: React.ReactNode }) {
  const [installed, setInstalled] = React.useState<string[]>([])
  const [catalog, setCatalog] = React.useState<StreamPlugin[]>(BUILTIN_PLUGINS)
  const [ready, setReady] = React.useState(false)
  const [companionAvailable, setCompanionAvailable] = React.useState(false)

  const refreshCatalog = React.useCallback(async () => {
    try {
      const res = await fetch(companionUrl("/api/adapters"), {
        cache: "no-store",
        signal: AbortSignal.timeout(1500),
      })
      if (!res.ok) {
        setCompanionAvailable(false)
        return
      }
      const data = await readJsonResponse<{ adapters?: StreamPlugin[] }>(res)
      setCompanionAvailable(true)
      // An empty array is a legitimate catalog state (e.g. the last adapter
      // was just removed), only bail out on a malformed/missing response,
      // never on a correctly-empty one.
      if (Array.isArray(data.adapters)) setCatalog(data.adapters)
    } catch {
      // Companion not running, keep last catalog, just mark unavailable.
      setCompanionAvailable(false)
    }
  }, [])

  React.useEffect(() => {
    const saved = readInstalledPlugins()
    const initial =
      saved.length > 0 ? saved : [...DEFAULT_INSTALLED_PLUGINS]
    setInstalled(initial)
    if (saved.length === 0) writeInstalledPlugins(initial)

    void refreshCatalog().finally(() => setReady(true))

    // Poll so the UI reacts if the user starts/stops the companion while
    // the page is open, without requiring a manual refresh.
    const interval = setInterval(() => void refreshCatalog(), COMPANION_POLL_MS)
    return () => clearInterval(interval)
  }, [refreshCatalog])

  const persist = React.useCallback((next: string[]) => {
    setInstalled(next)
    writeInstalledPlugins(next)
  }, [])

  const value = React.useMemo<PluginContextValue>(
    () => ({
      installed,
      catalog,
      ready,
      companionAvailable,
      refreshCatalog,
      install: (id) => {
        if (installed.includes(id)) return
        persist([...installed, id])
      },
      uninstall: (id) => persist(installed.filter((x) => x !== id)),
      toggle: (id) =>
        persist(
          installed.includes(id)
            ? installed.filter((x) => x !== id)
            : [...installed, id],
        ),
      isInstalled: (id) => installed.includes(id),
    }),
    [installed, catalog, persist, ready, companionAvailable, refreshCatalog],
  )

  return (
    <PluginContext.Provider value={value}>{children}</PluginContext.Provider>
  )
}

export function usePlugins() {
  const ctx = React.useContext(PluginContext)
  if (!ctx) throw new Error("usePlugins must be used within PluginProvider")
  return ctx
}
