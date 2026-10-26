import { clsx, type ClassValue } from "clsx";
import { extendTailwindMerge } from "tailwind-merge";

/**
 * tailwind-merge needs to know Navree's custom tokens so that, for example,
 * `text-fg-2` and `text-sm` are not treated as the same group and dropped.
 */
const twMerge = extendTailwindMerge({
  extend: {
    classGroups: {
      "font-family": [{ font: ["display", "sans", "mono"] }],
      "font-size": [{ text: ["display-2xl", "display-xl", "display-lg", "display-md", "display-sm", "eyebrow"] }],
    },
  },
});

/** Joins class names and resolves Tailwind conflicts (the last one wins). */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
