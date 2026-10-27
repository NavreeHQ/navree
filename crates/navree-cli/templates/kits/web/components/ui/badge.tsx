import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { cn } from "@/lib/utils"
import { Slot } from "radix-ui"

const badgeVariants = cva(
  "group/badge inline-flex h-5 w-fit shrink-0 items-center justify-center gap-1 overflow-hidden rounded-md border border-transparent px-1.5 text-xs font-medium whitespace-nowrap transition-colors focus-visible:ring-2 focus-visible:ring-ring/60 has-data-[icon=inline-end]:pr-1 has-data-[icon=inline-start]:pl-1 aria-invalid:border-destructive [&>svg]:pointer-events-none [&>svg]:size-3!",
  {
    variants: {
      variant: {
        // Neutral by default: green is for positive data only (`success`).
        default: "border-line-2 bg-ink-2 text-fg-1 [a]:hover:bg-ink-3",
        secondary: "bg-ink-3 text-fg-1 [a]:hover:bg-ink-3/80",
        success: "border-accent/25 bg-accent/10 text-accent",
        warning: "border-amber/25 bg-amber/10 text-amber",
        destructive: "border-destructive/25 bg-destructive/10 text-destructive",
        outline: "border-line-2 text-fg-1 [a]:hover:bg-ink-2",
        ghost: "text-fg-2 hover:bg-ink-2 hover:text-fg",
        link: "text-fg-1 underline decoration-line-2 underline-offset-4 hover:text-fg",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  }
)

function Badge({
  className,
  variant = "default",
  asChild = false,
  ...props
}: React.ComponentProps<"span"> &
  VariantProps<typeof badgeVariants> & { asChild?: boolean }) {
  const Comp = asChild ? Slot.Root : "span"

  return (
    <Comp
      data-slot="badge"
      data-variant={variant}
      className={cn(badgeVariants({ variant }), className)}
      {...props}
    />
  )
}

export { Badge, badgeVariants }
