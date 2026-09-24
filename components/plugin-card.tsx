"use client"

import * as React from "react"
import { Check, Download, Trash2 } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { usePlugins } from "@/components/plugin-provider"
import {
  getMetadataHintLabel,
  type StreamPlugin,
} from "@/lib/plugins/catalog"

type PluginCardProps = {
  plugin: StreamPlugin
  onRemovePackage?: () => Promise<void> | void
}

function SpecRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-border bg-secondary/40 px-3 py-2">
      <p className="text-[10px] uppercase tracking-wider text-muted-foreground">{label}</p>
      <p className="mt-0.5 text-xs font-medium text-foreground/90">{value}</p>
    </div>
  )
}

export function PluginCard({ plugin, onRemovePackage }: PluginCardProps) {
  const { isInstalled, toggle } = usePlugins()
  const installed = isInstalled(plugin.id)
  const [removing, setRemoving] = React.useState(false)

  return (
    <Card className="overflow-hidden transition hover:border-foreground/25">

      <CardHeader>
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0 space-y-1">
            <div className="flex flex-wrap items-center gap-2">
              <CardTitle className="text-lg">{plugin.name}</CardTitle>
              <Badge className="font-mono text-[10px] uppercase">
                {plugin.status}
              </Badge>
              {plugin.source === "external" && (
                <Badge className="text-[10px]">external</Badge>
              )}
            </div>
            <p className="font-mono text-[11px] text-muted-foreground">
              adapter/{plugin.id} · v{plugin.version}
            </p>
            <CardDescription className="mt-2 text-sm leading-relaxed">
              {plugin.summary}
            </CardDescription>
          </div>
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        <p className="text-sm leading-relaxed text-muted-foreground">{plugin.description}</p>

        <div className="grid grid-cols-2 gap-2">
          <SpecRow label="Class" value={plugin.adapterClass} />
          <SpecRow label="Transport" value={plugin.transport} />
          <SpecRow label="Decryption" value={plugin.decryption} />
          <SpecRow label="Metadata" value={plugin.metadataBinding} />
        </div>

        <div className="flex flex-wrap gap-2">
          {plugin.capabilities.movie && (
            <Badge className="font-normal">VOD · Film</Badge>
          )}
          {plugin.capabilities.episodes && (
            <Badge className="font-normal">Serialized · Episodes</Badge>
          )}
          {plugin.capabilities.subtitles && (
            <Badge className="font-normal">WebVTT</Badge>
          )}
          {plugin.capabilities.multipleQualities && (
            <Badge className="font-normal">ABR ladder</Badge>
          )}
        </div>

        <p className="text-xs text-muted-foreground">
          {getMetadataHintLabel(plugin.idHint)}
        </p>

        <Button
          className="w-full"
          variant={installed ? "secondary" : "default"}
          onClick={() => toggle(plugin.id)}
        >
          {installed ? (
            <>
              <Check data-icon="inline-start" />
              Adapter enabled
            </>
          ) : (
            <>
              <Download data-icon="inline-start" />
              Enable adapter
            </>
          )}
        </Button>
        {installed && (
          <Button
            variant="ghost"
            size="sm"
            className="w-full text-muted-foreground"
            onClick={() => toggle(plugin.id)}
          >
            <Trash2 data-icon="inline-start" />
            Disable adapter
          </Button>
        )}
        {plugin.source === "external" && onRemovePackage && (
          <Button
            variant="ghost"
            size="sm"
            className="w-full text-destructive hover:text-destructive"
            disabled={removing}
            onClick={async () => {
              setRemoving(true)
              try {
                await onRemovePackage()
              } finally {
                setRemoving(false)
              }
            }}
          >
            <Trash2 data-icon="inline-start" />
            {removing ? "Removing package..." : "Remove package"}
          </Button>
        )}
      </CardContent>
    </Card>
  )
}
