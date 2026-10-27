import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/** Merges Tailwind classes, later ones winning (shadcn-vue's `cn`). */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
