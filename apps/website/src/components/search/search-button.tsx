"use client";

import { useSyncExternalStore } from "react";

import { SearchIcon } from "@/components/icons";

export const OPEN_SEARCH_EVENT = "navree:open-search";

const subscribe = () => () => {};
const isMac = () => /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

/** Opens the search dialog. Shows the shortcut (⌘K or Ctrl K) on wide screens. */
export function SearchButton() {
  const mac = useSyncExternalStore(subscribe, isMac, () => true);
  return (
    <button
      type="button"
      onClick={() => window.dispatchEvent(new Event(OPEN_SEARCH_EVENT))}
      className="group inline-flex h-10 items-center gap-2.5 rounded-full border border-line-2 bg-ink-1 pr-2 pl-3 text-sm text-fg-2 transition-colors hover:border-fg-3 hover:text-fg max-sm:w-10 max-sm:justify-center max-sm:px-0 sm:w-56"
      aria-label="Search the docs"
      aria-keyshortcuts={mac ? "Meta+K" : "Control+K"}
    >
      <SearchIcon className="size-[1.0625rem] shrink-0" />
      <span className="hidden flex-1 text-left sm:inline">Search the docs</span>
      <span className="hidden items-center gap-1 sm:inline-flex" aria-hidden>
        <kbd className="kbd">{mac ? "⌘" : "Ctrl"}</kbd>
        <kbd className="kbd">K</kbd>
      </span>
    </button>
  );
}
