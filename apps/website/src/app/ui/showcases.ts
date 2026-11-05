/**
 * The example apps, for "Build anything" on `/ui/` and for `/showcase/`. One
 * entry per app; the pages draw every entry the same way, so adding an app is
 * adding an entry here, its pictures (`scripts/showcase-images.ts`) and its
 * page under `/showcase/<slug>/`.
 *
 * - `showcases`: apps drawn with Navree UI. Each gets a card at the top of
 *   "Build anything" and a block below it: its screenshot, the code of its
 *   hardest part, what it proves, its numbers and its screens to try. The same
 *   entries make the cards on `/showcase/` and on the homepage, the answer
 *   about what you can build in the Navree UI questions, and the page list of
 *   `llms.txt`.
 * - `otherFrontEnds`: apps whose screens another front end draws. Each gets a
 *   card only.
 *
 * Text is plain, with `code` in backticks and links as `[words](/path/)`.
 * Numbers are read from the files that own them, never typed in.
 */
import store from "@content/showcase/store.json";
import toolkit from "@content/ui/toolkit.json";

import { appConstant } from "@/components/showcase/app-constants";
import { boardNumbers } from "@/components/showcase/board-numbers";
import { inboxNumbers } from "@/components/showcase/inbox-numbers";
import type { Excerpt, ExcerptPart } from "@/lib/excerpt";
import { indian } from "@/lib/format";
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
  /** The app running, as captured, and an optional close-up on a phone (keys of the picture manifest, and what the close-up shows). */
  screenshot: { image: string; address: string; closeUp?: { image: string; what: string } };
  /** The code of its hardest part, with a sentence on what it does. */
  code: Excerpt & { caption: string };
  /** One to four ui-docs examples drawn from its views; the first shows before anyone clicks. */
  views: ShowcaseView[];
  /** The command that runs it. */
  run: string;
};

const seconds = (ms: number) => `${(ms / 1000).toFixed(2)} s`;
const ms = (n: number) => `${Math.round(n)} ms`;
const kb = (bytes: number) => `${(bytes / 1024).toFixed(1)} KB`;
/** A count the numbers file writes for reading (`5,499,392`), as a number. */
const count = (text: string) => Number(text.replace(/,/g, ""));
/** Conversations on the inbox's full desk: the app's own constant. */
const conversations = appConstant("apps/inbox-demo/src/scale.rs", "FULL");

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
    screenshot: {
      image: "store-drawer",
      address: "127.0.0.1:8095/orders",
      closeUp: { image: "store-drawer-phone", what: "the same drawer on a phone" },
    },
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
  {
    id: "board",
    slug: "board",
    app: "board-demo",
    name: "Rangla Looms",
    kind: "A team's board",
    ui: "Navree UI",
    hardest: `${indian(boardNumbers.cards)} cards moved by mouse, touch or keys, every move checked on the server`,
    still: "board-columns",
    title: `A board your whole team moves cards on, at ${indian(boardNumbers.cards)} cards`,
    lead: `The product team of an invented fabric exporter in Ludhiana plans its next release on five columns of ${indian(boardNumbers.cards)} cards. You move a card with a mouse, a finger or the keys and it lands at once; the server checks the move against the board as it is now, and every other open board slides the card into place.`,
    summary: `A product team's board for an invented fabric exporter in Ludhiana: five columns of ${indian(boardNumbers.cards)} cards moved by mouse, touch or keys, every move checked by the server, and every open board following it live.`,
    proves: [
      "A [Sortable](/docs/components/sortable/) board: a card moves by mouse, finger or keys, and every step is said to screen readers",
      "Moves [checked by the server](/docs/ui/drag-and-drop/) in a transaction the toolkit begins: a stale move, a full column or a card a member may not close is refused and put back",
      `Columns of thousands load a page at a time by a cursor: ${boardNumbers.page} cards first, up to ${boardNumbers.cap} in the page`,
      "Other people's moves arrive live over [server-sent events](/docs/ui/regions/), and each open board slides the card into place",
      "A card opens in a [drawer](/docs/components/sheet/) over the board, with its checklist, comments and history loading as you scroll",
    ],
    facts: [
      { label: "Data", value: `SQLite, ${indian(boardNumbers.cards)} cards` },
      { label: "Live", value: "Server-sent events" },
      { label: "Input", value: "Mouse, touch and keys" },
      { label: "Screens", value: "Board, card drawer, palette" },
    ],
    numbers: {
      text: `The first page is ${boardNumbers.firstPage.brotliKb} KB with brotli (${Math.round(boardNumbers.firstPage.htmlBytes / 1024)} KB of HTML: the first ${boardNumbers.page} cards of each column), the sortable module ${kb(boardNumbers.sortableBrotli)}, and two columns scrolled to their cap of ${boardNumbers.cap} cards hold ${indian(boardNumbers.nodes)} nodes.`,
      source: "src/components/showcase/board-numbers.ts",
      date: boardNumbers.measured,
    },
    screenshot: {
      image: "board-card",
      address: "127.0.0.1:8097/",
      closeUp: { image: "board-card-phone", what: "the same card on a phone" },
    },
    code: {
      file: "apps/board-demo/src/pages.rs",
      parts: [
        { from: "/// `POST /board/moves`", to: "async fn moves(" },
        { from: "// The toolkit's transaction", to: "let reply = mv.refused" },
      ],
      more: true,
      caption:
        "The page moves the card; this handler decides. A drop posts a `Move`, and `Move::transaction` begins it at READ COMMITTED with the two columns locked first, so a stale move, a full column or a card a member may not close is refused in the transaction that would have written it. A refusal puts the card back.",
    },
    run: "cargo run -p board-demo",
    views: [
      {
        example: "build-anything/board-columns",
        label: "Board",
        title: "A product team's board: five columns of cards with labels, due dates, assignees and limits, rendered by navree-ui",
        caption:
          "The app's own page, drawn from its seed. Replayed in your browser: two cards a teammate moves slide into new columns. In the app, the server streams every move over SSE.",
        file: "src/views/board.rs",
      },
      {
        example: "build-anything/board-card",
        label: "Card",
        title: "A card's details: description, checklist, comments, a move control and its history, rendered by navree-ui",
        caption: "What the drawer shows. Its checklist lines move the same way the cards do, and older comments load as you scroll.",
        file: "src/views/card.rs",
        interactive: true,
      },
      {
        example: "build-anything/board-drag",
        label: "Mid-move",
        title: "A card picked up with the keys and carried into Review, the board saying where it is, rendered by navree-ui",
        caption:
          "Part way through a move. With the keys, each step is said: “Position 2 of 2 in Review.” With a mouse or a finger, a copy follows you and the cards make room.",
        file: "src/views/board.rs",
      },
      {
        example: "build-anything/board-sync",
        label: "Two screens",
        title: "Two people's screens of one board, a card arriving on the second, rendered by navree-ui",
        caption:
          "Two viewers. Gurpreet moves a card to Review; on Harleen's screen it slides in, replayed here. In the app, a server-sent event tells her board to fetch itself again.",
        file: "src/views/board.rs",
      },
    ],
  },
  {
    id: "inbox",
    slug: "inbox",
    app: "inbox-demo",
    name: "Sukhna Labs",
    kind: "A support inbox",
    ui: "Navree UI",
    hardest: `${indian(conversations)} conversations, loaded as they are read and live over a stream`,
    still: "inbox-panes",
    title: `A support inbox that stays fast at ${indian(conversations)} conversations`,
    lead: "The help desk of an invented software company in Mohali, in three panes you resize: a thread with notes and files, the customer beside it, and new messages arriving while you type. Every list loads as it is read, so the first screen costs one page whatever the desk holds.",
    summary: `The support desk of an invented software company in Mohali: three panes you resize, a thread with notes and files, the customer beside it, new messages arriving while you type, and ${indian(conversations)} conversations read a page at a time.`,
    proves: [
      "A [SplitPane](/docs/components/split-pane/) dragged or moved with the keys, its sizes kept for each agent on the server",
      "A [LazyList](/docs/components/lazy-list/) that pages by cursor: fifty conversations, thirty messages, older ones above",
      "Live over server-sent events: only what changed is sent, and a row changes where it stands, so your focus stays",
      "Assign through a [Combobox](/docs/components/combobox/) searched on the server, snooze with a [DatePicker](/docs/components/date-picker/), act from the [command palette](/docs/components/command/)",
      "Single keys you can turn off, and every action as a link or a form with JavaScript off",
    ],
    facts: [
      { label: "Data", value: `SQLite, ${indian(conversations)} conversations` },
      { label: "Live", value: "Server-sent events" },
      { label: "Input", value: "Reply, assign, snooze, keys" },
      { label: "Screens", value: "Three panes, thread, palette" },
    ],
    numbers: {
      text: `The first screen paints in ${ms(inboxNumbers.firstScreen.largest_paint_ms)} from ${kb(inboxNumbers.firstScreen.bytes_total)}, a 100-message conversation opens in ${ms(inboxNumbers.openThread.largest_paint_ms)}, and scrolling to ${indian(inboxNumbers.flat.heldTo)} conversations never keeps more than ${indian(inboxNumbers.flat.maxRows)} rows in the page, on a desk of ${indian(count(inboxNumbers.messages))} messages.`,
      source: "src/components/showcase/inbox-numbers.ts",
      date: inboxNumbers.measured,
    },
    screenshot: {
      image: "inbox-panes",
      address: "127.0.0.1:8096/inbox/19",
      closeUp: { image: "inbox-phone", what: "the same conversation on a phone" },
    },
    code: {
      file: "apps/inbox-demo/src/views/list.rs",
      parts: [
        { from: "/// The list: lazy, live", to: ") -> LazyList {" },
        { from: "let mut list = LazyList::new(LIST", to: ".max(300)" },
        { from: "if page.more_after", to: "}" },
      ],
      more: true,
      caption: `The list is a \`LazyList\` that is lazy and live. \`since\` and \`live_from\` say which revision it holds and which stream to follow, \`max(300)\` lets the far end leave the page, and the cursors of its first and last rows ask for the next page, so page 400 costs what page 1 does, with ${indian(conversations)} conversations behind it.`,
    },
    run: "cargo run --release -p inbox-demo",
    views: [
      {
        example: "build-anything/inbox-panes",
        label: "Inbox",
        title:
          "The support inbox: the conversations, an open conversation with an internal note and a file, and the customer's plan, history and tags, rendered by navree-ui",
        caption:
          "Replayed in your browser: a new conversation joins the top and the customer writes again. In the app, the server streams them over SSE.",
        file: "src/views/inbox.rs",
      },
      {
        example: "build-anything/inbox-thread",
        label: "100 messages",
        title: "A conversation of 100 messages opened at its newest thirty, with a link to earlier messages, rendered by navree-ui",
        caption:
          "A long conversation opens at its newest thirty messages; older ones load as you scroll up, and you keep your place. Replayed in your browser: in the app each page comes from the server by cursor.",
        file: "src/views/thread.rs",
      },
      {
        example: "build-anything/inbox-phone",
        label: "On a phone",
        title: "The inbox on a phone: one pane at a time, with a link back to the conversations, rendered by navree-ui",
        caption: "On a phone the panes show one at a time, with links between them.",
        file: "src/views/inbox.rs",
      },
      {
        example: "build-anything/inbox-palette",
        label: "Command palette",
        title: "The command palette: actions on the open conversation, saved replies and places to go, rendered by navree-ui",
        caption: "Ctrl+K: assign, close, snooze, change the priority, or drop a saved reply into what you are writing.",
        file: "src/views/inbox.rs",
        interactive: true,
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
        { from: ".in_region(", to: ")" },
      ],
    },
  },
  {
    id: "dates",
    title: "Date and range pickers",
    hard: "A calendar, a date picker, a range picker with presets and a time picker, with locales, day rules and clock changes.",
    image: "tile-dates",
    docs: "/docs/ui/dates/",
    code: {
      file: "apps/ui-docs/src/examples/dates.rs",
      example: "date-range-picker/presets",
      parts: [{ from: 'DateRangePicker::new("from", "to")' }, { from: ".presets([", to: "])" }, { from: '.title("Closed")' }],
    },
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
    code: { file: "apps/ui-docs/src/examples/icons.rs", example: "flags/with-icons" },
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
