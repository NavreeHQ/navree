import type { VariantProps } from "class-variance-authority";
import { cva } from "class-variance-authority";

export { default as Toggle } from "./Toggle.vue";

export const toggleVariants = cva(
  "inline-flex items-center justify-center gap-1.5 whitespace-nowrap rounded-xs text-[13px] font-medium text-fg-2 transition-colors duration-150 hover:text-fg disabled:pointer-events-none disabled:opacity-50 data-[state=on]:bg-ink-3 data-[state=on]:text-fg [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-3.5",
  {
    variants: {
      variant: {
        default: "bg-transparent",
        outline: "bg-transparent",
      },
      size: {
        default: "h-8 min-w-8 px-3",
        sm: "h-7 min-w-7 px-2.5",
        lg: "h-9 min-w-9 px-3.5",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

export type ToggleVariants = VariantProps<typeof toggleVariants>;
