"use client";

import type MiniSearch from "minisearch";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect, useId, useMemo, useState, useSyncExternalStore, type KeyboardEvent } from "react";

import { SearchIcon } from "@/components/icons";
import { findHits, loadIndex, snippet } from "@/components/search/search-index";
import { closestPage, type PageEntry } from "@/lib/closest-page";
import type { SearchDoc } from "@/lib/search-options";

const subscribe = () => () => {};
/** The address the visitor asked for. The page is the 404 for any address, so the server can't know it. */
const askedPath = () => window.location.pathname;

let pagesPromise: Promise<PageEntry[]> | null = null;

/** The site's pages, from the small list the build writes (not the megabytes of the search index). */
function loadPages(): Promise<PageEntry[]> {
  pagesPromise ??= fetch("/data/pages.json").then((response) => {
    if (!response.ok) throw new Error(`pages.json: HTTP ${response.status}`);
    return response.json() as Promise<PageEntry[]>;
  });
  return pagesPromise;
}

/**
 * "Did you mean ...?": the page nearest to the address that was asked for,
 * found in the list of pages once it has loaded. Shows nothing when no page is
 * near, and nothing without JavaScript (the links below it stay).
 */
export function ClosestPage() {
  const asked = useSyncExternalStore(subscribe, askedPath, () => "");
  const [pages, setPages] = useState<PageEntry[] | null>(null);
  useEffect(() => {
    if (asked) loadPages().then(setPages, () => {});
  }, [asked]);
  const match = useMemo(() => (pages && asked ? closestPage(asked, pages) : null), [pages, asked]);
  if (!match) return null;
  return (
    <p className="mt-6 max-w-xl text-lg leading-relaxed text-pretty text-fg-1" role="status">
      Did you mean{" "}
      <Link href={match.href} className="link font-medium">
        {match.title}
      </Link>
      ? <span className="text-fg-3">({match.section})</span>
    </p>
  );
}

/**
 * The docs search, in the page: a field, and the hits listed under it as
 * links. The search index (megabytes) is fetched when the field is first
 * focused, not before. Enter opens the first hit; the arrow keys move
 * between the hits.
 */
export function NotFoundSearch() {
  const router = useRouter();
  const id = useId();
  const [index, setIndex] = useState<MiniSearch<SearchDoc> | null>(null);
  const [failed, setFailed] = useState(false);
  const [query, setQuery] = useState("");
  const hits = useMemo(() => findHits(index, query).slice(0, 6), [index, query]);
  const searching = query.trim().length >= 2;

  function load() {
    loadIndex().then(
      (loaded) => {
        setIndex(loaded);
        setFailed(false);
      },
      () => setFailed(true),
    );
  }

  function onKey(event: KeyboardEvent<HTMLElement>) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const items = [...event.currentTarget.querySelectorAll<HTMLElement>("input, ul a")];
    const at = items.indexOf(document.activeElement as HTMLElement);
    const next = items[at + (event.key === "ArrowDown" ? 1 : -1)];
    if (next) {
      event.preventDefault();
      next.focus();
    }
  }

  return (
    <form
      role="search"
      aria-label="Search the docs"
      className="mt-10 max-w-xl"
      onKeyDown={onKey}
      onSubmit={(event) => {
        event.preventDefault();
        if (hits[0]) router.push(hits[0].href);
      }}
    >
      <label htmlFor={id} className="sr-only">
        Search the docs
      </label>
      <div className="relative">
        <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-4 size-[1.125rem] -translate-y-1/2 text-fg-3" />
        <input
          id={id}
          type="search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onFocus={load}
          placeholder="Search the docs"
          autoComplete="off"
          spellCheck={false}
          enterKeyHint="search"
          className="h-12 w-full rounded-full border border-line-2 bg-ink-1 pr-5 pl-11 text-base text-fg outline-none transition-colors placeholder:text-fg-3 hover:border-fg-3 focus-visible:border-accent"
        />
      </div>
      <div role="status" aria-live="polite">
        {searching ? (
          hits.length ? (
            <ul className="mt-3 grid gap-1 rounded-2xl border border-line bg-ink-1/60 p-2">
              {hits.map((hit) => (
                <li key={hit.id}>
                  <Link href={hit.href} className="block rounded-xl px-3 py-2.5 transition-colors hover:bg-ink-2 focus-visible:bg-ink-2">
                    <span className="block text-xs text-fg-3">
                      {hit.section}
                      {hit.heading && hit.page !== hit.section ? ` / ${hit.page}` : ""}
                    </span>
                    <span className="mt-0.5 block font-medium text-fg">{hit.heading || hit.page}</span>
                    <span className="mt-1 line-clamp-2 block text-sm leading-relaxed text-fg-2">{snippet(hit.text, hit.terms)}</span>
                  </Link>
                </li>
              ))}
            </ul>
          ) : (
            <p className="mt-3 px-1 text-sm text-fg-2">
              {failed
                ? "The search index didn't load. Check your connection and try again."
                : index
                  ? `Nothing matches “${query.trim()}”. Try fewer words or another spelling.`
                  : "Loading the index…"}
            </p>
          )
        ) : null}
      </div>
    </form>
  );
}
