"use client";

import { usePathname, useSearchParams } from "next/navigation";
import { useCallback, useEffect, useState } from "react";

type Value = string | number | boolean | null | undefined;

/**
 * List filters live in the URL so views can be shared and survive reloads.
 * `set({ search: "agra" })` replaces the history entry and, by default, resets
 * pagination. It uses `history.replaceState`, which Next.js syncs into
 * `useSearchParams` without a server round trip, and does nothing once the
 * user has navigated to another page (a late debounced search must not drag
 * them back).
 */
export function useUrlState() {
  const searchParams = useSearchParams();
  const pathname = usePathname();

  const get = useCallback((key: string) => searchParams.get(key) ?? undefined, [searchParams]);

  const set = useCallback(
    (updates: Record<string, Value>, { resetPage = true }: { resetPage?: boolean } = {}) => {
      if (window.location.pathname !== pathname) return;
      const next = new URLSearchParams(window.location.search);
      for (const [key, value] of Object.entries(updates)) {
        if (value === undefined || value === null || value === "" || value === false) next.delete(key);
        else next.set(key, String(value));
      }
      if (resetPage && !("page" in updates)) next.delete("page");
      const query = next.toString();
      window.history.replaceState(null, "", query ? `${pathname}?${query}` : pathname);
    },
    [pathname],
  );

  const page = Math.max(1, Number(searchParams.get("page")) || 1);

  return { get, set, page, searchParams };
}

export function useDebouncedValue<T>(value: T, delay = 250): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const id = setTimeout(() => setDebounced(value), delay);
    return () => clearTimeout(id);
  }, [value, delay]);
  return debounced;
}
