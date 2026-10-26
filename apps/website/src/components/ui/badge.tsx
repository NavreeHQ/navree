import { cva, type VariantProps } from "class-variance-authority";
import { Slot } from "radix-ui";
import type * as React from "react";

import { cn } from "@/lib/utils";

/**
 * Small labels. `default` is a green tint (use it for the one thing that
 * matters), `secondary` a raised ink chip, `outline` a hairline, `mono` a
 * code-style tag for crate names and commands, `warning` amber.
 */
const badgeVariants = cva(
  "inline-flex w-fit shrink-0 items-center justify-center gap-1 overflow-hidden rounded-full border px-2.5 py-0.5 text-xs font-medium whitespace-nowrap transition-[color,background-color,border-color] [&>svg]:pointer-events-none [&>svg]:size-3",
  {
    variants: {
      variant: {
        default: "border-accent/25 bg-accent/10 text-accent [a&]:hover:bg-accent/15",
        secondary: "border-line bg-ink-2 text-fg-1 [a&]:hover:bg-ink-3",
        outline: "border-line-2 text-fg-2 [a&]:hover:border-fg-3 [a&]:hover:text-fg",
        mono: "rounded-md border-line bg-ink-1 font-mono text-[0.6875rem] text-fg-1",
        warning: "border-amber/30 bg-amber/10 text-amber",
        destructive: "border-destructive/30 bg-destructive/10 text-destructive",
        ghost: "border-transparent text-fg-2 [a&]:hover:bg-ink-2 [a&]:hover:text-fg",
        link: "border-transparent text-accent underline-offset-4 [a&]:hover:underline",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  },
);

function Badge({
  className,
  variant = "default",
  asChild = false,
  ...props
}: React.ComponentProps<"span"> & VariantProps<typeof badgeVariants> & { asChild?: boolean }) {
  const Comp = asChild ? Slot.Root : "span";

  return <Comp data-slot="badge" data-variant={variant} className={cn(badgeVariants({ variant }), className)} {...props} />;
}

export { Badge, badgeVariants };
