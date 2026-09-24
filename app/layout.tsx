import type { Metadata } from "next"
import { Fraunces, Geist_Mono, Inter, Sulphur_Point } from "next/font/google"

import "./globals.css"
import { SiteHeader } from "@/components/site-header"
import { PluginProvider } from "@/components/plugin-provider"
import { ThemeProvider } from "@/components/theme-provider"
import { cn } from "@/lib/utils"

const inter = Inter({ subsets: ["latin"], variable: "--font-sans" })

const fraunces = Sulphur_Point({
  subsets: ["latin"],
  weight: ["700"],
  variable: "--font-display",
  // axes: ["opsz"],
})

const fontMono = Geist_Mono({
  subsets: ["latin"],
  variable: "--font-mono",
})

export const metadata: Metadata = {
  title: "Streamflow · Modular stream resolution",
  description:
    "Browse TMDB catalog and resolve playback through installable extraction adapters.",
}

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html
      lang="en"
      suppressHydrationWarning
      className={cn(
        "dark antialiased",
        fontMono.variable,
        "font-sans",
        inter.variable,
        fraunces.variable,
      )}
    >
      <body className="min-h-svh bg-background text-foreground">
        <ThemeProvider>
          <PluginProvider>
            <div className="relative min-h-svh">
              <SiteHeader />
              <main className="relative mx-auto max-w-7xl px-4 py-8 sm:px-6">
                {children}
              </main>
            </div>
          </PluginProvider>
        </ThemeProvider>
      </body>
    </html>
  )
}
