/**
 * Reads the latest published release of the Streamflow Companion
 * (built + uploaded by `.github/workflows/companion-release.yml`) from
 * GitHub's public API, server-side. Set `COMPANION_REPO` (e.g.
 * "owner/streamflow") once the repo exists on GitHub and a
 * `companion-v*` tag has been released; until then this returns `null`
 * and the UI falls back to source-build instructions.
 */
export type CompanionDownloads = {
  version: string
  macArm64?: string
  macX64?: string
  windowsX64?: string
  linuxX64?: string
}

const ASSET_MAP: Record<string, keyof Omit<CompanionDownloads, "version">> = {
  "streamflow-companion-macos-arm64.tar.gz": "macArm64",
  "streamflow-companion-macos-x64.tar.gz": "macX64",
  "streamflow-companion-windows-x64.zip": "windowsX64",
  "streamflow-companion-linux-x64.tar.gz": "linuxX64",
}

export async function getLatestCompanionRelease(): Promise<CompanionDownloads | null> {
  const repo = process.env.COMPANION_REPO
  if (!repo) return null

  try {
    const res = await fetch(`https://api.github.com/repos/${repo}/releases/latest`, {
      headers: { Accept: "application/vnd.github+json" },
      next: { revalidate: 3600 },
    })
    if (!res.ok) return null

    const data = (await res.json()) as {
      tag_name?: string
      assets?: { name: string; browser_download_url: string }[]
    }

    const downloads: CompanionDownloads = { version: data.tag_name ?? "latest" }
    for (const asset of data.assets ?? []) {
      const key = ASSET_MAP[asset.name]
      if (key) downloads[key] = asset.browser_download_url
    }
    return downloads
  } catch {
    return null
  }
}
