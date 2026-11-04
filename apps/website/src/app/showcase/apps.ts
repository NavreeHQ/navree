/**
 * The example apps the Showcase lists under the CRM, in order. Each is an app in
 * the repository under `apps/<app>`, with a page of its own at `/showcase/<slug>/`,
 * and its card shows one of its screens as ui-docs renders it from the app's Rust.
 * Add an app by adding an entry.
 */

export type ShowcaseApp = {
  /** The page: `/showcase/<slug>/`. */
  slug: string;
  /** The crate under `apps/`. */
  app: string;
  /** The card's heading: what it is, in a few words. */
  name: string;
  /** One or two sentences: what someone can see and do in it. */
  summary: string;
  /** Four short facts, each a label and a value. */
  facts: { label: string; value: string }[];
  /** The ui-docs example the card shows, drawn as a still. */
  preview: string;
  /** That picture's accessible title. */
  previewTitle: string;
};

export const apps: ShowcaseApp[] = [
  {
    slug: "store",
    app: "store-demo",
    name: "A store's back office",
    summary:
      "Orders, refunds, stock and a live feed for an invented shop in Punjab: 200,000 orders paged by cursor, an order in a drawer over its page, and every refund and cancel checked again on the server.",
    facts: [
      { label: "UI", value: "Navree UI" },
      { label: "Data", value: "SQLite, 200,000 orders" },
      { label: "Live", value: "Server-sent events" },
      { label: "Screens", value: "Orders, drawer, stock, dashboard" },
    ],
    preview: "build-anything/store-drawer",
    previewTitle: "An order open in a drawer over the store's orders table",
  },
];
