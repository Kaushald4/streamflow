import { cn } from "@/lib/utils"

function Badge({
  className,
  ...props
}: React.ComponentProps<"span">) {
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-full border border-border bg-secondary/70 px-2.5 py-0.5 text-xs font-medium text-foreground/90",
        className,
      )}
      {...props}
    />
  )
}

export { Badge }
