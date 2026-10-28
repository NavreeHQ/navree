"use client";

import { useEffect, useState, type ComponentType } from "react";

import { OPEN_SEARCH_EVENT } from "./search-button";

type Dialog = ComponentType<{ openOnMount?: boolean }>;

/** True for the search shortcuts: ⌘K / Ctrl K anywhere, "/" outside text fields. */
export function isSearchShortcut(event: KeyboardEvent): boolean {
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") return true;
  const target = event.target as HTMLElement | null;
  return event.key === "/" && !target?.closest("input, textarea, select, [contenteditable='true']");
}

/**
 * Loads the search dialog (and its search library) the first time someone
 * asks for it, with a shortcut or a search button, instead of shipping it
 * with every page. From then on the dialog handles the shortcuts itself.
 */
export function SearchLauncher() {
  const [Loaded, setLoaded] = useState<Dialog | null>(null);

  useEffect(() => {
    if (Loaded) return;
    let requested = false;
    const load = () => {
      if (requested) return;
      requested = true;
      import("./search-dialog").then((m) => setLoaded(() => m.SearchDialog));
    };
    const onKey = (event: KeyboardEvent) => {
      if (!isSearchShortcut(event)) return;
      event.preventDefault();
      load();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener(OPEN_SEARCH_EVENT, load);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener(OPEN_SEARCH_EVENT, load);
    };
  }, [Loaded]);

  return Loaded ? <Loaded openOnMount /> : null;
}
