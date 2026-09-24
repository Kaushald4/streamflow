"use client"

import { PluginCard } from "@/components/plugin-card"
import { AdapterUploadPanel } from "@/components/adapter-upload"
import { CompanionDownloadPanel } from "@/components/companion-download"
import { usePlugins } from "@/components/plugin-provider"
import { companionFetch } from "@/lib/api/companion"
import type { CompanionDownloads } from "@/lib/api/companion-release"
import { readJsonResponse } from "@/lib/api/client"

type Props = { companionRelease: CompanionDownloads | null }

export function PluginsRegistry({ companionRelease }: Props) {
  const { catalog, companionAvailable, refreshCatalog, uninstall } = usePlugins()

  async function removePackage(id: string) {
    const res = await companionFetch(`/api/adapters/${id}`, { method: "DELETE" })
    const data = await readJsonResponse<{ error?: string }>(res)
    if (!res.ok) throw new Error(data.error ?? "Remove failed")
    uninstall(id)
    await refreshCatalog()
  }

  return (
    <div className="space-y-8">
      {!companionAvailable && <CompanionDownloadPanel release={companionRelease} />}
      <AdapterUploadPanel />
      <div className="grid gap-5 md:grid-cols-2">
        {catalog.map((plugin) => (
          <PluginCard
            key={plugin.id}
            plugin={plugin}
            onRemovePackage={
              plugin.source === "external"
                ? () => removePackage(plugin.id)
                : undefined
            }
          />
        ))}
      </div>
    </div>
  )
}
