import type { Options } from "minisearch";

/**
 * One searchable chunk: a page's introduction, or one of its `##`/`###`
 * sections. `scripts/prebuild.ts` builds the index from these; the search
 * dialog loads it with the same options.
 */
export type SearchDoc = {
  id: string;
  /** Where the chunk is: the page URL plus `#anchor` for a section. */
  href: string;
  /** The page title. */
  page: string;
  /** The docs section ("The basics"), or the page group ("Performance"). */
  section: string;
  /** The heading of the chunk; empty for a page's introduction. */
  heading: string;
  /** Plain text of the chunk. */
  text: string;
};

export const searchOptions: Options<SearchDoc> = {
  fields: ["page", "heading", "text"],
  storeFields: ["href", "page", "section", "heading", "text"],
  searchOptions: {
    boost: { heading: 3, page: 2 },
    prefix: true,
    fuzzy: 0.15,
    combineWith: "AND",
  },
};

/** Where the prebuilt index is served from. */
export const SEARCH_INDEX_URL = "/data/search-index.json";
