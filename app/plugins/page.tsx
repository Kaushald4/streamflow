import { Package } from "lucide-react"
import { PluginsRegistry } from "@/components/plugins-registry"
import { Badge } from "@/components/ui/badge"
import { getLatestCompanionRelease } from "@/lib/api/companion-release"

export default async function PluginsPage() {
  const companionRelease = await getLatestCompanionRelease()

  return (
    <div className="space-y-8 pb-16">
      <section className="rounded-2xl border border-border bg-card p-8 sm:p-10">
        <div className="mb-3 flex flex-wrap items-center gap-2">
          <Package className="size-5 text-amber-500" />
          <Badge>Adapter registry</Badge>
          <Badge>Extraction engine v1</Badge>
        </div>
        <h1 className="font-heading text-3xl font-medium tracking-tight sm:text-4xl">
          Stream resolution adapters
        </h1>
        <p className="mt-3 max-w-2xl leading-relaxed text-muted-foreground">
          Adapters run in the local companion (QuickJS). Upload a packed{" "}
          <code className="text-xs">.zip</code> from the extractor repo, enable
          it here, then extract from the watch page.
        </p>
        <div className="mt-5 grid gap-3 sm:grid-cols-3">
          {[
            { label: "Where", value: "Companion on 127.0.0.1" },
            { label: "Runtime", value: "QuickJS + native hooks" },
            { label: "Output", value: "HLS / MP4 + headers" },
          ].map((item) => (
            <div
              key={item.label}
              className="rounded-lg border border-border bg-secondary/40 px-4 py-3"
            >
              <p className="text-[10px] uppercase tracking-wider text-muted-foreground">
                {item.label}
              </p>
              <p className="mt-1 text-sm font-medium">{item.value}</p>
            </div>
          ))}
        </div>
      </section>

      <PluginsRegistry companionRelease={companionRelease} />
    </div>
  )
}
