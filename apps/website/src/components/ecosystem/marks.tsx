import {
  AnchorIcon,
  CompassIcon,
  ContainerIcon,
  LighthouseIcon,
  NotebookTextIcon,
  PackageIcon,
  ShipIcon,
  ShipWheelIcon,
  type LucideIcon,
} from "lucide-react";

import { cn } from "@/lib/utils";

/** The glyph of each tool: nautical, like the hull in the logo. */
const glyphs: Record<string, LucideIcon> = {
  lighthouse: LighthouseIcon,
  fleet: ShipIcon,
  harbor: AnchorIcon,
  helm: ShipWheelIcon,
  logbook: NotebookTextIcon,
  navigator: CompassIcon,
  dock: ContainerIcon,
  "starter-kits": PackageIcon,
};

export function hasMark(slug: string): boolean {
  return slug in glyphs;
}

/**
 * A tool's mark: its glyph, thin-lined, on a small machined tile. Not a
 * coloured circle: an ink tile with a 1px edge and a faint top highlight.
 */
export function ProductMark({ slug, size = "md", className }: { slug: string; size?: "sm" | "md" | "lg"; className?: string }) {
  const Glyph = glyphs[slug] ?? PackageIcon;
  const box = { sm: "size-8 rounded-lg", md: "size-10 rounded-xl", lg: "size-12 rounded-[0.875rem]" }[size];
  const icon = { sm: "size-4", md: "size-5", lg: "size-6" }[size];
  return (
    <span
      aria-hidden
      className={cn(
        "relative inline-flex shrink-0 items-center justify-center border border-line-2 bg-linear-to-b from-ink-3 to-ink-1 text-fg-1 shadow-bezel",
        box,
        className,
      )}
    >
      <Glyph className={icon} strokeWidth={1.5} />
    </span>
  );
}
