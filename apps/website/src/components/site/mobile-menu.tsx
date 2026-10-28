"use client";

import { MenuIcon } from "lucide-react";
import { useState, type ComponentType } from "react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

type Sheet = ComponentType<{ className?: string; defaultOpen?: boolean }>;

let sheet: Promise<Sheet> | null = null;
const loadSheet = () => (sheet ??= import("./mobile-menu-sheet").then((m) => m.MobileMenuSheet));

/**
 * Below `lg` the main navigation lives in a sheet opened from the header.
 * The button is all that ships with the page: the sheet (a Radix dialog)
 * loads when the pointer or focus reaches the button and opens on the click.
 */
export function MobileMenu({ className }: { className?: string }) {
  const [Loaded, setLoaded] = useState<Sheet | null>(null);
  if (Loaded) return <Loaded className={className} defaultOpen />;
  return (
    <Button
      variant="ghost"
      size="icon-sm"
      className={cn("text-fg-1", className)}
      aria-label="Open the menu"
      aria-haspopup="dialog"
      aria-expanded={false}
      onPointerEnter={loadSheet}
      onFocus={loadSheet}
      onClick={() => loadSheet().then((component) => setLoaded(() => component))}
    >
      <MenuIcon className="size-5" />
    </Button>
  );
}
