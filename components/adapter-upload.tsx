"use client"

import * as React from "react"
import { Upload } from "lucide-react"
import { Button } from "@/components/ui/button"
import { usePlugins } from "@/components/plugin-provider"
import { companionFetch } from "@/lib/api/companion"
import { readJsonResponse } from "@/lib/api/client"

function zipFileName(file: File): string {
  const base = file.name.split(/[/\\]/).pop() ?? "adapter.zip"
  return base.toLowerCase().endsWith(".zip") ? base : `${base}.zip`
}

export function AdapterUploadPanel() {
  const { refreshCatalog, install } = usePlugins()
  const inputRef = React.useRef<HTMLInputElement>(null)
  const [uploading, setUploading] = React.useState(false)
  const [message, setMessage] = React.useState<string | null>(null)
  const [error, setError] = React.useState<string | null>(null)

  async function handleFile(file: File) {
    setUploading(true)
    setMessage(null)
    setError(null)

    try {
      const bytes = await file.arrayBuffer()
      if (bytes.byteLength === 0) {
        throw new Error(
          "Could not read the selected file. Move it out of iCloud/Downloads and try again.",
        )
      }

      const form = new FormData()
      const blob = new Blob([bytes], {
        type: file.type || "application/zip",
      })
      form.append("package", blob, zipFileName(file))

      const res = await companionFetch("/api/adapters/upload", {
        method: "POST",
        body: form,
      })

      const data = await readJsonResponse<{
        ok?: boolean
        adapter?: { id: string; name: string }
        error?: string
      }>(res)

      if (!res.ok) throw new Error(data.error ?? "Upload failed")

      await refreshCatalog()
      if (data.adapter?.id) install(data.adapter.id)
      setMessage(
        data.adapter
          ? `Installed ${data.adapter.name} (adapter/${data.adapter.id})`
          : "Adapter installed",
      )
    } catch (err) {
      setError(err instanceof Error ? err.message : "Upload failed")
    } finally {
      setUploading(false)
      if (inputRef.current) inputRef.current.value = ""
    }
  }

  return (
    <section className="rounded-2xl border border-dashed border-border bg-card p-6 sm:p-8">
      <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
        <div className="space-y-1">
          <h2 className="font-heading text-lg font-medium">Install standalone adapter</h2>
          <p className="max-w-xl text-sm text-muted-foreground">
            Upload a <code className="text-xs">.zip</code> package containing{" "}
            <code className="text-xs">manifest.json</code> and a compiled{" "}
            <code className="text-xs">index.js</code> entry module. Adapters load at
            runtime without rebuilding Streamflow.
          </p>
        </div>
        <div className="flex shrink-0 flex-col gap-2">
          <input
            ref={inputRef}
            type="file"
            accept=".zip"
            className="hidden"
            onChange={(e) => {
              const file = e.target.files?.[0]
              if (file) void handleFile(file)
            }}
          />
          <Button
            type="button"
            disabled={uploading}
            onClick={() => inputRef.current?.click()}
          >
            <Upload data-icon="inline-start" />
            {uploading ? "Installing..." : "Upload adapter package"}
          </Button>
        </div>
      </div>
      {message && (
        <p className="mt-4 text-sm text-emerald-600 dark:text-emerald-400">{message}</p>
      )}
      {error && (
        <p className="mt-4 text-sm text-destructive">{error}</p>
      )}
    </section>
  )
}
