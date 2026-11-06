import MiniSearch, { type SearchResult } from "minisearch";
import type { ReactNode } from "react";

import { SEARCH_INDEX_URL, searchOptions, type SearchDoc } from "@/lib/search-options";

/** A search result: the chunk that matched, with the terms that matched in it. */
export type Hit = SearchResult & Omit<SearchDoc, "id">;

let indexPromise: Promise<MiniSearch<SearchDoc>> | null = null;

/** The docs search index, fetched once and kept; a failed fetch is tried again next time. */
export function loadIndex(): Promise<MiniSearch<SearchDoc>> {
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

/** The best twelve hits for what was typed: every word first, then any word. Nothing for fewer than two characters. */
export function findHits(index: MiniSearch<SearchDoc> | null, query: string): Hit[] {
  const q = query.trim();
  if (!index || q.length < 2) return [];
  let results = index.search(q) as Hit[];
  if (!results.length) results = index.search(q, { combineWith: "OR" }) as Hit[];
  return results.slice(0, 12);
}

/** The text around the first match, with every matched term marked. */
export function snippet(text: string, terms: string[]): ReactNode {
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
