"use client";

import { useSyncExternalStore } from "react";

import { SearchIcon } from "@/components/icons";
import { OPEN_SEARCH_EVENT } from "@/components/search/search-button";
import { cn } from "@/lib/utils";

const subscribe = () => () => {};
const isMac = () => /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

/**
 * The header's search trigger: an icon button on phones, a compact field
 * with the shortcut from `sm` up. Opens the site search dialog.
 */
export function HeaderSearch({ className }: { className?: string }) {
  const mac = useSyncExternalStore(subscribe, isMac, () => true);
  return (
    <button
      type="button"
      onClick={() => window.dispatchEvent(new Event(OPEN_SEARCH_EVENT))}
      className={cn(
        "group inline-flex h-9 cursor-pointer items-center gap-2 rounded-full border border-line-2 bg-ink-1/80 text-sm text-fg-3 transition-colors duration-200 hover:border-fg-3 hover:text-fg",
        "w-9 justify-center max-sm:border-transparent max-sm:bg-transparent max-sm:text-fg-1 sm:w-44 sm:justify-start sm:pr-1.5 sm:pl-3 xl:w-56",
        className,
      )}
      aria-label="Search the docs"
      aria-keyshortcuts={mac ? "Meta+K" : "Control+K"}
    >
      <SearchIcon className="size-4 shrink-0" />
      <span className="hidden flex-1 text-left sm:inline">Search</span>
      <span className="hidden items-center gap-0.5 sm:inline-flex" aria-hidden>
        <kbd className="kbd">{mac ? "⌘" : "Ctrl"}</kbd>
        <kbd className="kbd">K</kbd>
      </span>
    </button>
  );
}
