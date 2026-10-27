import type { VariantProps } from "class-variance-authority";
import { cva } from "class-variance-authority";

export { default as Badge } from "./Badge.vue";

export const badgeVariants = cva(
  "inline-flex w-fit shrink-0 items-center justify-center gap-1 overflow-hidden rounded-sm border px-1.5 py-0.5 text-xs font-medium whitespace-nowrap [&>svg]:pointer-events-none [&>svg]:size-3",
  {
    variants: {
      variant: {
        default: "border-accent/25 bg-accent/10 text-accent",
        secondary: "border-line bg-ink-2 text-fg-1",
        outline: "border-line-2 text-fg-1",
        mono: "border-line bg-ink-2 font-mono text-fg-1",
        warning: "border-amber/30 bg-amber/10 text-amber",
        destructive: "border-danger/30 bg-danger/10 text-danger",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  },
);
export type BadgeVariants = VariantProps<typeof badgeVariants>;
