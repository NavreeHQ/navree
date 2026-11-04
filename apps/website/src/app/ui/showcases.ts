/**
 * The example apps, for "Build anything" on `/ui/` and for `/showcase/`. One
 * entry per app; the pages draw every entry the same way, so adding an app is
 * adding an entry here, its pictures (`scripts/showcase-images.ts`) and its
 * page under `/showcase/<slug>/`.
 *
 * - `showcases`: apps drawn with Navree UI. Each gets a card at the top of
 *   "Build anything" and a block below it: its screenshot, the code of its
 *   hardest part, what it proves, its numbers and its screens to try.
 * - `otherFrontEnds`: apps whose screens another front end draws. Each gets a
 *   card only.
 *
 * Text is plain, with `code` in backticks and links as `[words](/path/)`.
 * Numbers are read from the files that own them, never typed in.
 */
import store from "@content/showcase/store.json";
import toolkit from "@content/ui/toolkit.json";

import type { Excerpt, ExcerptPart } from "@/lib/excerpt";
import { uiRenderCeilings } from "@/lib/ui-page-data";

export type ShowcaseView = {
  /** The ui-docs example the preview draws: "build-anything/<slug>". */
  example: string;
  /** Its tab: short, sentence case. */
  label: string;
  /** What the preview shows, for assistive technology (the frame's title). */
  title: string;
  /** One or two sentences under the preview. Says plainly what is replayed. */
  caption: string;
  /** The app's file the view's Rust comes from, relative to `apps/<app>/`. */
  file: string;
  /** True when the preview can be used (a drawer scrolls, tabs switch). Links and forms still go nowhere. */
  interactive?: boolean;
};

/** What every app has: enough for its card. */
export type AppCard = {
  /** The anchor on `/ui/` (`#<id>`) and the key. */
  id: string;
  /** The page: `/showcase/<slug>/`. */
  slug: string;
  /** What it is called. */
  name: string;
  /** What kind of app it is, in a few words. */
  kind: string;
  /** What draws its screens. */
  ui: string;
  /** Its single hardest fact, on one line. */
  hardest: string;
  /** The picture on its card (a key of `public/images/showcase/manifest.json`). */
  still: string;
};

export type Showcase = AppCard & {
  /** The crate, which is also its folder under `apps/`. */
  app: string;
  /** The heading of its block: what you could build, sentence case. */
  title: string;
  /** Two or three sentences: what you get, then how. */
  lead: string;
  /** One or two sentences for its card on `/showcase/`. */
  summary: string;
  /** Three to five lines, each a hard thing you can check in the screens or the code, linked to its docs. */
  proves: string[];
  /** Exactly four: data and scale, live, input, screens. */
  facts: { label: string; value: string }[];
  /** One line of measured numbers, the file they come from and when they were taken. */
  numbers: { text: string; source: string; date: string };
  /** The app running, as captured, and an optional close-up (keys of the picture manifest). */
  screenshot: { image: string; address: string; closeUp?: string };
  /** The code of its hardest part, with a sentence on what it does. */
  code: Excerpt & { caption: string };
  /** One to four ui-docs examples drawn from its views; the first shows before anyone clicks. */
  views: ShowcaseView[];
  /** The command that runs it. */
  run: string;
};

/** 2,00,000: Indian digit grouping. */
export const indian = (n: number) => n.toLocaleString("en-IN");
const seconds = (ms: number) => `${(ms / 1000).toFixed(2)} s`;
const kb = (bytes: number) => `${(bytes / 1024).toFixed(1)} KB`;

const orders = store.pages.find((p) => p.name === "Orders");
if (!orders) throw new Error("showcases: content/showcase/store.json has no Orders page");

export const showcases: Showcase[] = [
  {
    id: "store",
    slug: "store",
    app: "store-demo",
    name: "Charkha Supply Co.",
    kind: "A store's back office",
    ui: "Navree UI",
    hardest: `${indian(store.rows.orders)} orders, paged by cursor on the server`,
    still: "store-dashboard",
    title: `A store's back office, at ${indian(store.rows.orders)} orders`,
    lead: "Orders, refunds, stock and a live feed for an invented shop in Punjab, in rupees and IST. The table pages by cursor on the server, an order opens in a drawer over its page, and the server checks every refund and cancel again.",
    summary: `Orders, refunds, stock and a live feed for an invented shop in Punjab: ${indian(store.rows.orders)} orders paged by cursor, an order in a drawer over its page, and every refund and cancel checked again on the server.`,
    proves: [
      `A [DataTable](/docs/ui/data-table/) over ${indian(store.rows.orders)} orders: filters, search, sort and keyset pages, all on the server`,
      "An order in a [Sheet](/docs/components/sheet/) 60% wide over its table, whose history [loads after it opens](/docs/ui/regions/)",
      "Refund, cancel and resend in [confirm dialogs](/docs/components/alert-dialog/), refused by the server once the order has moved on",
      "An [activity feed](/docs/components/activity-feed/) over server-sent events that fetches only what is new",
      `Stock for ${indian(store.rows.products)} products, with a restock form in each row`,
    ],
    facts: [
      { label: "Data", value: `SQLite, ${indian(store.rows.orders)} orders` },
      { label: "Live", value: "Server-sent events" },
      { label: "Input", value: "Refund, cancel, restock" },
      { label: "Screens", value: "Orders, drawer, stock, dashboard" },
    ],
    numbers: {
      text: `Orders in ${seconds(orders.desktop.lcp_ms)} to the first screen, ${seconds(orders.phone.lcp_ms)} on a slow phone; a drawer in ${store.drawer.open_ms} ms from one answer of ${kb(store.drawer.bytes)}.`,
      source: "content/showcase/store.json",
      date: store.measured_on,
    },
    screenshot: { image: "store-drawer", address: "127.0.0.1:8095/orders", closeUp: "store-drawer-phone" },
    code: {
      file: "apps/store-demo/src/views/order.rs",
      parts: [
        { from: "let history: Node = match parts {", to: "};" },
        { from: "let others: Node = match parts {", to: "};" },
        { from: "/// The drawer a row of the table opens", to: "}" },
      ],
      caption: `The drawer is a \`Sheet\` 60% wide. Its history and the customer's other orders are regions that load once it is open, so it opens on one answer of ${kb(store.drawer.bytes)} in ${store.drawer.open_ms} ms, and they follow by ${store.drawer.parts_ms} ms.`,
    },
    run: "cargo run --release -p store-demo",
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

/** Apps on Navree whose screens another front end draws: a card each, linking to their page. */
export const otherFrontEnds: AppCard[] = [
  {
    id: "crm",
    slug: "crm",
    name: "Navree CRM",
    kind: "A sales team's CRM",
    ui: "Next.js · Nuxt",
    hardest: "One Navree API under two front ends, with roles and live updates",
    still: "crm-pipeline",
  },
];

/** Every app with a page of its own, in the order the cards show them. */
export const appCards: AppCard[] = [...showcases, ...otherFrontEnds];

/**
 * One hard component on its own: a picture, a line on what is hard, a few
 * lines of the code that draws it, and the app that uses it.
 */
export type HardPart = {
  id: string;
  title: string;
  /** One or two sentences: what is hard, and how Navree UI does it. */
  hard: string;
  /** The picture (a key of the picture manifest). */
  image: string;
  /** Its docs page. */
  docs: string;
  /** The `id` of the app in `showcases` that uses it, if one does. */
  seenIn?: string;
  /**
   * Where its code comes from: a ui-docs example's code (`example`, from the
   * catalogue the toolkit writes) or a file's lines. `file` is the path shown,
   * from the repository's root.
   */
  code: { file: string; example?: string; parts?: ExcerptPart[]; more?: boolean };
};

const ceilings = uiRenderCeilings();

export const hardParts: HardPart[] = [
  {
    id: "data-table",
    title: "DataTable at scale",
    hard: `One struct says what each column can do, and the server runs the search, the filters, the sort and the keyset pages. A page of 1,000 rows renders in under ${ceilings.thousandRows} ms.`,
    image: "tile-table",
    docs: "/docs/ui/data-table/",
    seenIn: "store",
    code: {
      file: "apps/store-demo/src/views/orders.rs",
      parts: [{ from: "#[derive(Clone, TableRow)]", to: "pub struct OrderRow {" }],
      more: true,
    },
  },
  {
    id: "live",
    title: "Live updates",
    hard: "A server-sent event nudges the feed, and it asks its server only for the rows after its newest. The page changes in place, and focus and typed text stay put.",
    image: "tile-live",
    docs: "/docs/ui/regions/",
    seenIn: "store",
    code: {
      file: "apps/store-demo/src/views/dashboard.rs",
      parts: [
        { from: 'ActivityFeed::new("activity", "Order activity")', to: ".live()" },
        { from: ".in_region(", to: '.live_event("activity")' },
      ],
    },
  },
  {
    id: "dates",
    title: "Date and range pickers",
    hard: "A calendar, a date picker, a range picker with presets and a time picker, with locales, day rules and clock changes.",
    image: "tile-dates",
    docs: "/docs/ui/dates/",
    code: { file: "apps/ui-docs/src/examples/dates.rs", example: "date-range-picker/presets" },
  },
  {
    id: "phone",
    title: "Phone input, from scratch",
    hard: `Its own parser on each country's numbering plan, a country list, formatting as you type, and E.164 when it is saved. A page with one renders in under ${ceilings.phoneInput} ms.`,
    image: "tile-phone",
    docs: "/docs/components/phone-input/",
    code: { file: "apps/ui-docs/src/examples/phone.rs", example: "phone-input/saved" },
  },
  {
    id: "icons-flags",
    title: "Icons and flags",
    hard: `${indian(toolkit.icons)} icons and ${indian(toolkit.flags)} flags, each a constant, so only the ones an app uses end up in its binary.`,
    image: "tile-flags",
    docs: "/docs/ui/flags/",
    seenIn: "store",
    code: { file: "apps/ui-docs/src/examples/icons.rs", example: "flags/usage" },
  },
  {
    id: "command",
    title: "Command palette and Combobox",
    hard: `Keyboard first, with groups, shortcuts and recent items; a Combobox can ask the server as you type. Ten thousand options render in under ${ceilings.tenThousandOptions} ms.`,
    image: "tile-command",
    docs: "/docs/components/command/",
    code: {
      file: "apps/ui-docs/src/examples/pickers.rs",
      example: "command/default",
      parts: [{ from: 'Command::new("palette")', to: '.shortcut("alt+d"),' }],
      more: true,
    },
  },
];

for (const part of hardParts) {
  if (part.seenIn && !showcases.some((app) => app.id === part.seenIn)) {
    throw new Error(`showcases: the hard part "${part.id}" is seen in "${part.seenIn}", which is not an app in showcases`);
  }
}
