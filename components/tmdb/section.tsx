import type { ReactNode } from "react"
import { cn } from "@/lib/utils"

type SectionHeadingProps = {
  title: string
  subtitle?: string
  className?: string
}

export function SectionHeading({ title, subtitle, className }: SectionHeadingProps) {
  return (
    <div className={cn("space-y-1", className)}>
      <h2 className="font-heading text-xl font-medium tracking-tight">{title}</h2>
      {subtitle && <p className="text-sm text-muted-foreground">{subtitle}</p>}
    </div>
  )
}

type DetailSectionProps = {
  title: string
  subtitle?: string
  children: ReactNode
  className?: string
}

export function DetailSection({
  title,
  subtitle,
  children,
  className,
}: DetailSectionProps) {
  return (
    <section className={cn("space-y-4", className)}>
      <SectionHeading title={title} subtitle={subtitle} />
      {children}
    </section>
  )
}
