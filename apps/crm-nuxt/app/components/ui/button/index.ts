import type { VariantProps } from "class-variance-authority";
import { cva } from "class-variance-authority";

export { default as Button } from "./Button.vue";

/**
 * Navree app buttons (DESIGN.md §9): 4px corners, no pills and no glow.
 * `default` is the green primary action (one per view), everything else is
 * ink and line. Focus uses the global green outline.
 */
export const buttonVariants = cva(
  "inline-flex shrink-0 items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-[background-color,border-color,color,box-shadow,transform] duration-150 ease-(--ease-out-quint) select-none disabled:pointer-events-none disabled:opacity-45 active:scale-[0.98] [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4 aria-invalid:border-danger/70",
  {
    variants: {
      variant: {
        default: "bg-accent font-semibold text-accent-ink hover:bg-accent/90",
        destructive: "bg-danger font-semibold text-[#1d0606] hover:bg-danger/90",
        outline: "border border-line-2 bg-transparent text-fg hover:border-fg-3/60 hover:bg-ink-2",
        secondary: "border border-line bg-ink-2 text-fg hover:border-line-2 hover:bg-ink-3",
        ghost: "text-fg-2 hover:bg-ink-2 hover:text-fg",
        link: "h-auto! rounded-sm px-0! text-accent underline-offset-4 hover:underline",
      },
      size: {
        default: "h-9 px-3.5 has-[>svg]:px-3",
        xs: "h-7 gap-1 px-2.5 text-xs has-[>svg]:px-2 [&_svg:not([class*='size-'])]:size-3.5",
        sm: "h-8 gap-1.5 px-3 text-[13px] has-[>svg]:px-2.5",
        lg: "h-10 px-5 has-[>svg]:px-4",
        icon: "size-9",
        "icon-xs": "size-7 [&_svg:not([class*='size-'])]:size-3.5",
        "icon-sm": "size-8",
        "icon-lg": "size-10",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);
export type ButtonVariants = VariantProps<typeof buttonVariants>;
