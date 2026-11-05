/**
 * What the board example app measured at its full size, on one laptop, over
 * loopback, on a release build (6 November 2026). The first page and the nodes
 * were read in Chromium. The seed's counts were read from the database, and its
 * time is the median of three fresh starts, from the process starting to its
 * first answer, since the app writes the seed before it listens. One place for
 * the figures, so `/showcase/board/`, the board's block on `/ui/` and its cards
 * say the same.
 *
 * The card count, the cards a column draws first and the most it keeps are
 * read from the app's own constants, so they follow the app; the sortable
 * module's size is read from `content/ui/toolkit.json`, which
 * `cargo run -p ui-docs` writes from the runtime bundle. The rest are the
 * figures measured on the running app: measure them again, and edit them here,
 * when the board changes.
 */
import toolkit from "@content/ui/toolkit.json";

import { appConstant } from "./app-constants";

const sortable = toolkit.assets.find((asset) => asset.name === "sortable" && asset.ext === "js");
if (!sortable?.brotli) throw new Error("board-numbers: content/ui/toolkit.json has no brotli size for the sortable module");

export const boardNumbers = {
  /** The day the figures below were measured. */
  measured: "2026-11-06",
  /** Cards the seed writes. */
  cards: appConstant("apps/board-demo/src/data.rs", "CARD_COUNT"),
  /** Comments and lines of history the seed writes with them, to the nearest thousand. */
  comments: 40_000,
  history: 92_000,
  /** About how long the seed takes to write at start, in seconds. */
  seedSeconds: 0.7,
  /** The first page of the board: its HTML in bytes, and the same compressed with brotli in KB of 1,024 bytes. */
  firstPage: { htmlBytes: 158_757, brotliKb: 7.6 },
  /** The sortable module compressed with brotli, in bytes. */
  sortableBrotli: sortable.brotli,
  /** DOM nodes after scrolling two columns of thousands of cards to their cap of 200 each. */
  nodes: 22_651,
  /** Cards a column draws at first, and loads at a time. */
  page: appConstant("apps/board-demo/src/views/board.rs", "PAGE"),
  /** Most cards a column keeps in the page. */
  cap: appConstant("apps/board-demo/src/views/board.rs", "CAP"),
} as const;
