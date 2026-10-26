"use client";

import MiniSearch, { type SearchResult } from "minisearch";
import { useRouter } from "next/navigation";
import { useCallback, useEffect, useId, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

import { CloseIcon, SearchIcon } from "@/components/icons";
import { SEARCH_INDEX_URL, searchOptions, type SearchDoc } from "@/lib/search-options";

import { OPEN_SEARCH_EVENT } from "./search-button";

type Hit = SearchResult & Omit<SearchDoc, "id">;

const SUGGESTIONS: { label: string; href: string }[] = [
  { label: "Installation", href: "/docs/getting-started/installation/" },
  { label: "Your first app", href: "/docs/getting-started/your-first-app/" },
  { label: "Routing and controllers", href: "/docs/basics/routing/" },
  { label: "Validation", href: "/docs/basics/validation/" },
  { label: "Authentication", href: "/docs/security/authentication/" },
  { label: "Workspaces", href: "/docs/workspaces/introduction/" },
  { label: "Performance", href: "/performance/" },
];

let indexPromise: Promise<MiniSearch<SearchDoc>> | null = null;

function loadIndex(): Promise<MiniSearch<SearchDoc>> {
  indexPromise ??= fetch(SEARCH_INDEX_URL)
    .then((response) => {
      if (!response.ok) throw new Error(`search index: HTTP ${response.status}`);
      return response.text();
    })
    .then((json) => MiniSearch.loadJSON<SearchDoc>(json, searchOptions))
    .catch((error) => {
      indexPromise = null;
      throw error;
    });
  return indexPromise;
}

/** The text around the first match, with every matched term marked. */
function snippet(text: string, terms: string[]): ReactNode {
  if (!text) return null;
  const lower = text.toLowerCase();
  const positions = terms.map((t) => lower.indexOf(t.toLowerCase())).filter((i) => i >= 0);
  const first = positions.length ? Math.min(...positions) : 0;
  const start = Math.max(0, first - 50);
  const window = (start > 0 ? "…" : "") + text.slice(start, start + 170) + (start + 170 < text.length ? "…" : "");
  if (!terms.length) return window;
  const escaped = terms.map((t) => t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).sort((a, b) => b.length - a.length);
  const parts = window.split(new RegExp(`(${escaped.join("|")})`, "gi"));
  return parts.map((part, i) => (i % 2 === 1 ? <mark key={i}>{part}</mark> : part));
}

/**
 * The docs search (⌘K / Ctrl K, or "/"). The index is built by
 * `scripts/prebuild.ts` and fetched the first time the dialog opens; the
 * search itself runs in the browser.
 */
export function SearchDialog() {
  const router = useRouter();
  const dialog = useRef<HTMLDialogElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const [index, setIndex] = useState<MiniSearch<SearchDoc> | null>(null);
  const [failed, setFailed] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const listId = useId();

  const open = useCallback(() => {
    const el = dialog.current;
    if (!el || el.open) return;
    el.showModal();
    input.current?.select();
    loadIndex().then(
      (loaded) => {
        setIndex(loaded);
        setFailed(false);
      },
      () => setFailed(true),
    );
  }, []);

  const close = useCallback(() => dialog.current?.close(), []);

  useEffect(() => {
    function onKey(event: globalThis.KeyboardEvent) {
      const target = event.target as HTMLElement | null;
      const typing = target?.closest("input, textarea, select, [contenteditable='true']");
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        if (dialog.current?.open) close();
        else open();
      } else if (event.key === "/" && !typing && !dialog.current?.open) {
        event.preventDefault();
        open();
      }
    }
    window.addEventListener("keydown", onKey);
    window.addEventListener(OPEN_SEARCH_EVENT, open);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener(OPEN_SEARCH_EVENT, open);
    };
  }, [open, close]);

  const hits = useMemo<Hit[]>(() => {
    const q = query.trim();
    if (!index || q.length < 2) return [];
    let results = index.search(q) as Hit[];
    if (!results.length) results = index.search(q, { combineWith: "OR" }) as Hit[];
    return results.slice(0, 12);
  }, [index, query]);

  function go(href: string) {
    close();
    router.push(href);
  }

  function onInputKey(event: KeyboardEvent<HTMLInputElement>) {
    const count = query.trim().length >= 2 ? hits.length : SUGGESTIONS.length;
    if (!count) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const next = event.key === "ArrowDown" ? (active + 1) % count : (active - 1 + count) % count;
      setActive(next);
      list.current?.querySelector(`[data-index="${next}"]`)?.scrollIntoView({ block: "nearest" });
    } else if (event.key === "Enter") {
      event.preventDefault();
      const href = query.trim().length >= 2 ? hits[active]?.href : SUGGESTIONS[active]?.href;
      if (href) go(href);
    }
  }

  const searching = query.trim().length >= 2;
  const optionId = (i: number) => `${listId}-option-${i}`;

  return (
    <dialog
      ref={dialog}
      aria-label="Search the docs"
      className="search-dialog"
      onClick={(event) => {
        if (event.target === dialog.current) close();
      }}
      onClose={() => {
        setQuery("");
        setActive(0);
      }}
    >
      <div className="flex max-h-[min(36rem,calc(100dvh-6rem))] flex-col">
        <div className="flex items-center gap-3 border-b border-line px-4">
          <SearchIcon className="size-5 shrink-0 text-fg-3" />
          <input
            ref={input}
            type="search"
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setActive(0);
            }}
            onKeyDown={onInputKey}
            placeholder="Search the docs"
            aria-label="Search the docs"
            role="combobox"
            aria-expanded="true"
            aria-controls={listId}
            aria-autocomplete="list"
            aria-activedescendant={(searching ? hits.length : SUGGESTIONS.length) ? optionId(active) : undefined}
            autoComplete="off"
            spellCheck={false}
            className="h-14 min-w-0 flex-1 bg-transparent text-base text-fg outline-none placeholder:text-fg-3 [&::-webkit-search-cancel-button]:hidden"
          />
          <button
            type="button"
            onClick={close}
            className="inline-flex size-8 items-center justify-center rounded-md text-fg-3 hover:bg-ink-3 hover:text-fg"
            aria-label="Close search"
          >
            <CloseIcon className="size-[1.125rem]" />
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain p-2">
          {searching ? (
            hits.length ? (
              <ul ref={list} id={listId} role="listbox" aria-label="Results">
                {hits.map((hit, i) => (
                  <li
                    key={hit.id}
                    id={optionId(i)}
                    data-index={i}
                    role="option"
                    aria-selected={i === active}
                    onMouseMove={() => setActive(i)}
                    onClick={() => go(hit.href)}
                    className="search-hit"
                  >
                    <span className="block text-xs text-fg-3">
                      {hit.section}
                      {hit.heading && hit.page !== hit.section ? ` / ${hit.page}` : ""}
                    </span>
                    <span className="mt-0.5 block font-medium text-fg">{hit.heading || hit.page}</span>
                    <span className="mt-1 line-clamp-2 block text-sm leading-relaxed text-fg-2">{snippet(hit.text, hit.terms)}</span>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="px-3 py-10 text-center text-sm text-fg-2" role="status">
                {failed
                  ? "The search index didn't load. Check your connection and try again."
                  : index
                    ? `Nothing matches “${query.trim()}”. Try fewer words or another spelling.`
                    : "Loading the index…"}
              </p>
            )
          ) : (
            <>
              <p className="px-3 pt-2 pb-1.5 text-xs text-fg-3">Popular pages</p>
              <ul ref={list} id={listId} role="listbox" aria-label="Popular pages">
                {SUGGESTIONS.map((item, i) => (
                  <li
                    key={item.href}
                    id={optionId(i)}
                    data-index={i}
                    role="option"
                    aria-selected={i === active}
                    onMouseMove={() => setActive(i)}
                    onClick={() => go(item.href)}
                    className="search-hit"
                  >
                    <span className="font-medium text-fg">{item.label}</span>
                  </li>
                ))}
              </ul>
            </>
          )}
        </div>

        <div className="hidden items-center gap-4 border-t border-line px-4 py-2.5 text-xs text-fg-3 sm:flex">
          <span className="inline-flex items-center gap-1.5">
            <kbd className="kbd">↑</kbd>
            <kbd className="kbd">↓</kbd> to move
          </span>
          <span className="inline-flex items-center gap-1.5">
            <kbd className="kbd">Enter</kbd> to open
          </span>
          <span className="inline-flex items-center gap-1.5">
            <kbd className="kbd">Esc</kbd> to close
          </span>
        </div>
      </div>
    </dialog>
  );
}
