import { Download, Terminal } from "lucide-react"
import { Button } from "@/components/ui/button"
import type { CompanionDownloads } from "@/lib/api/companion-release"

type Props = { release: CompanionDownloads | null }

const PLATFORMS: { label: string; key: keyof Omit<CompanionDownloads, "version"> }[] = [
  { label: "macOS (Apple Silicon)", key: "macArm64" },
  { label: "macOS (Intel)", key: "macX64" },
  { label: "Windows", key: "windowsX64" },
  { label: "Linux", key: "linuxX64" },
]

/** Shown on /plugins whenever the companion isn't detected. Real download
 * links once a `companion-v*` release exists (see
 * `.github/workflows/companion-release.yml`). Until then: a plain "not
 * ready yet" message for real visitors, or `cargo run` instructions when
 * running locally in development, the latter is a dev convenience, never
 * something a live-site visitor without Rust installed should see. */
export function CompanionDownloadPanel({ release }: Props) {
  const hasAnyAsset = release && PLATFORMS.some((p) => release[p.key])
  const isDev = process.env.NODE_ENV === "development"

  return (
    <div className="rounded-2xl border border-dashed border-border bg-card p-6 text-sm text-muted-foreground">
      <p className="font-medium text-foreground">Streamflow Companion not detected</p>
      <p className="mt-1 max-w-2xl leading-relaxed">
        Installing and running third-party adapters, and streaming from them, happens
        locally on your machine, not on our servers, this keeps your bandwidth (not
        ours) doing the work. Download and run the companion, then refresh this page.
      </p>

      {hasAnyAsset ? (
        <div className="mt-4 flex flex-wrap gap-2">
          {PLATFORMS.filter((p) => release?.[p.key]).map((p) => (
            <a key={p.key} href={release?.[p.key]}>
              <Button variant="outline" size="sm">
                <Download data-icon="inline-start" className="size-3.5" />
                {p.label}
              </Button>
            </a>
          ))}
        </div>
      ) : isDev ? (
        <div className="mt-4 flex items-start gap-2 rounded-lg border border-border bg-secondary/40 px-4 py-3">
          <Terminal className="mt-0.5 size-4 shrink-0" />
          <div>
            <p className="font-medium text-foreground">No published build yet (dev only)</p>
            <p className="mt-1">
              Run it from source for now:{" "}
              <code className="text-xs">cargo run -p companion</code> from the{" "}
              <code className="text-xs">companion/</code> directory. This message only
              shows in development.
            </p>
          </div>
        </div>
      ) : (
        <p className="mt-4 text-muted-foreground">
          Companion downloads aren&apos;t available yet. Check back soon.
        </p>
      )}
    </div>
  )
}
