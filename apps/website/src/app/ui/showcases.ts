/**
 * The example apps "Build anything" on `/ui/` shows, in order. Each is an app in
 * the repository under `apps/<app>`; each view is a ui-docs example
 * (`content/ui/catalog.json`), rendered by navree-ui from that app's own Rust.
 * Add an app by adding an entry: the section draws every entry the same way.
 *
 * Text is plain, with `code` in backticks and links as `[words](/path/)`.
 */

export type ShowcaseView = {
  /** The ui-docs example the preview draws: "build-anything/<slug>". */
  example: string;
  /** Its tab: short, sentence case. */
  label: string;
  /** What the preview shows, for assistive technology (the frame's title). */
  title: string;
  /** One or two sentences under the preview. Says plainly what is replayed. */
  caption: string;
  /** The app's file the Code tab's Rust comes from, relative to `apps/<app>/`. */
  file: string;
  /** True when the preview can be used (a drawer scrolls, tabs switch). Links and forms still go nowhere. */
  interactive?: boolean;
};

export type Showcase = {
  /** The anchor (`/ui/#<id>`) and key. */
  id: string;
  /** The crate, which is also its folder under `apps/`. */
  app: string;
  /** The heading: what you could build, sentence case. */
  title: string;
  /** Two or three sentences: what you get, then how. */
  lead: string;
  /** Three to five short lines, each something you can check in the preview or the code. */
  points: string[];
  /** One to four views; the first shows before anyone clicks. */
  views: ShowcaseView[];
};

export const showcases: Showcase[] = [
  {
    id: "store",
    app: "store-demo",
    title: "A store's back office, at 200,000 orders",
    lead: "Orders, refunds, stock and a live feed for an invented shop on SQLite. The table pages by cursor on the server, an order opens in a drawer over its page, and the server checks every refund and cancel again.",
    points: [
      "A [DataTable](/docs/ui/data-table/) over 200,000 orders: filters, search and keyset pages, all on the server",
      "A [Sheet](/docs/components/sheet/) 60% wide over the table, whose history loads after it opens",
      "Refund, cancel and resend, each confirmed in a dialog, and refused by the server once the order has moved on",
      "An [activity feed](/docs/components/activity-feed/) over server-sent events that fetches only what is new",
    ],
    views: [
      {
        example: "build-anything/store-drawer",
        label: "Order drawer",
        title:
          "An order open in a drawer over the orders table: badges, actions, customer and payment cards, a route with two flags, the items and the order's history",
        caption:
          "The drawer as it opens over the table. Scroll it: the history and the customer's other orders load a moment after it opens, and the server checks each action again.",
        file: "src/views/order.rs",
        interactive: true,
      },
      {
        example: "build-anything/store-orders",
        label: "Orders",
        title: "The orders table: search, five filters, a flag per country and keyset pages",
        caption:
          "One struct says what each column can do. In the app, the filters, the search and the cursor's pages answer from the server, and new orders arrive over the stream.",
        file: "src/views/orders.rs",
      },
      {
        example: "build-anything/store-dashboard",
        label: "Dashboard",
        title: "The store's dashboard: today's figures, the order activity, low stock and top sellers",
        caption:
          "Replayed in your browser: the new rows were drawn by the app's Rust ahead of time. In the app, the server streams them over SSE, and the feed asks only for what came after its newest row.",
        file: "src/views/dashboard.rs",
      },
      {
        example: "build-anything/store-inventory",
        label: "Inventory",
        title: "Inventory: 2,000 products with stock levels, low-stock badges and a restock form in each row",
        caption:
          "Each row has a form of its own. In the app, the server adds the units, refuses a number out of range, and the stream redraws the row.",
        file: "src/views/inventory.rs",
      },
    ],
  },
];
