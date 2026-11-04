/**
 * The pictures of the "Build anything" section on `/ui/`: screenshots of the
 * example apps while they run, and of the toolkit's own preview pages, saved
 * as AVIF and WebP in the widths the page asks for, with a manifest of their
 * sizes and alt texts (`public/images/showcase/manifest.json`).
 *
 *   cargo run --release -p store-demo        # the store, on http://127.0.0.1:8095
 *   pnpm build && pnpm serve                 # the preview pages, on http://127.0.0.1:3211
 *   node scripts/showcase-images.ts          # every picture
 *   node scripts/showcase-images.ts store-drawer tile-phone   # only these
 *
 * `STORE_URL` and `SITE_URL` point it at other addresses. The full-size PNGs
 * stay in `target/showcase-raw/` (git-ignored). Look at every picture before
 * you commit it: the store's simulator places orders while it runs, so a
 * second run draws other orders.
 */
import fs from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";

import { chromium, type Page } from "playwright";

const WEBSITE = path.resolve(import.meta.dirname, "..");
const REPO = path.resolve(WEBSITE, "..", "..");
const OUT = path.join(WEBSITE, "public", "images", "showcase");
const RAW = path.join(REPO, "target", "showcase-raw");
const MANIFEST = path.join(OUT, "manifest.json");
const STORE = process.env.STORE_URL ?? "http://127.0.0.1:8095";
const SITE = process.env.SITE_URL ?? "http://127.0.0.1:3211";

// sharp ships with Next; resolve it from there rather than adding a dependency.
interface SharpImage {
  resize(options: { width: number; withoutEnlargement: boolean }): SharpImage;
  extract(region: { left: number; top: number; width: number; height: number }): SharpImage;
  avif(options: { quality: number; effort: number; chromaSubsampling: string }): SharpImage;
  webp(options: { quality: number; effort: number; smartSubsample: boolean }): SharpImage;
  toFile(file: string): Promise<{ width: number; height: number; size: number }>;
  metadata(): Promise<{ width?: number; height?: number }>;
}
const sharp = createRequire(createRequire(import.meta.url).resolve("next/package.json"))("sharp") as (
  input: Buffer | string,
) => SharpImage;

type Box = { x: number; y: number; width: number; height: number };

type Capture = {
  key: string;
  alt: string;
  /** Output widths in pixels; the largest is the picture's own size. */
  widths: number[];
} & (
  | {
      /** The page to draw. */
      url: () => string;
      /** The window, in CSS pixels. */
      viewport: { width: number; height: number };
      /** Device pixels per CSS pixel. */
      scale: number;
      /** What to do before the picture: open a drawer, a popover. */
      prepare?: (page: Page) => Promise<void>;
      /** The part of the window to keep, in CSS pixels; the whole window when left out. */
      clip?: (page: Page) => Promise<Box>;
    }
  | {
      /** A picture already in the repository, from `apps/website`. */
      file: string;
    }
);

export type ManifestImage = {
  key: string;
  width: number;
  height: number;
  alt: string;
  avif: { src: string; width: number }[];
  webp: { src: string; width: number }[];
};

/** The date in India, `days` from today, as the store's filters write it. */
function istDate(days: number): string {
  const ms = Date.now() + 5.5 * 3_600_000 + days * 86_400_000;
  return new Date(ms).toISOString().slice(0, 10);
}

/** The orders of the two days before today: shipped, packed and unfulfilled side by side. */
const twoDays = () => `${STORE}/orders?f.placed_on.min=${istDate(-2)}&f.placed_on.max=${istDate(-1)}`;

/** Opens the first order of the table in its drawer, and waits for the parts that load after it opens. */
async function openFirstOrder(page: Page) {
  await page.locator("table tbody tr").first().locator("button").first().click();
  await page.locator('[data-slot="timeline"]').first().waitFor({ state: "visible" });
  await page.waitForTimeout(400);
}

/** `box` grown by `pad` and widened or heightened to `ratio` (width over height), kept inside the window. */
function framed(box: Box, pad: number, ratio: number, view: { width: number; height: number }): Box {
  let width = box.width + pad * 2;
  let height = box.height + pad * 2;
  if (width / height < ratio) width = height * ratio;
  else height = width / ratio;
  const x = Math.max(0, Math.min(view.width - width, box.x + box.width / 2 - width / 2));
  const y = Math.max(0, Math.min(view.height - height, box.y + box.height / 2 - height / 2));
  return { x: Math.round(x), y: Math.round(y), width: Math.round(width), height: Math.round(height) };
}

/** The smallest box holding every element `selectors` match that is on the screen. */
async function around(page: Page, selectors: string[]): Promise<Box> {
  const boxes: Box[] = [];
  for (const selector of selectors) {
    for (const element of await page.locator(selector).all()) {
      const b = await element.boundingBox();
      if (b && b.width > 0 && b.height > 0) boxes.push(b);
    }
  }
  if (!boxes.length) throw new Error(`showcase-images: nothing on screen for ${selectors.join(", ")}`);
  const x = Math.min(...boxes.map((b) => b.x));
  const y = Math.min(...boxes.map((b) => b.y));
  return {
    x,
    y,
    width: Math.max(...boxes.map((b) => b.x + b.width)) - x,
    height: Math.max(...boxes.map((b) => b.y + b.height)) - y,
  };
}

const captures: Capture[] = [
  {
    key: "hero-desktop",
    alt: "Cloudworks CRM's deals page: a sidebar with counts, four figures in crore and lakh, a table of eight open deals with their owners and stages, and the Sutlej Supply renewal open in a drawer with its amount, its contact and a timeline of seven events in IST",
    url: () => `${SITE}/ui-preview/app-shell/full.html`,
    viewport: { width: 1440, height: 840 },
    scale: 2,
    widths: [800, 1200, 1600, 2000],
  },
  {
    key: "hero-phone",
    alt: "The same deal on a phone: the drawer over the deals table, with its stage, amount, contact and the first events of its timeline",
    url: () => `${SITE}/ui-preview/app-shell/full.html`,
    viewport: { width: 358, height: 600 },
    scale: 3,
    widths: [400, 716, 1074],
  },
  {
    key: "store-drawer",
    alt: "Charkha Supply Co.'s orders, filtered to two days, with one order open in a drawer over the table: its badges and actions, the customer, the payment, the warehouse and the address with their flags, and its items in rupees",
    url: twoDays,
    viewport: { width: 1440, height: 900 },
    scale: 2,
    prepare: openFirstOrder,
    widths: [640, 960, 1440, 2880],
  },
  {
    key: "store-drawer-phone",
    alt: "The same order's drawer on a phone, full width, with its actions and the customer, payment and route cards stacked",
    url: twoDays,
    viewport: { width: 390, height: 844 },
    scale: 3,
    prepare: openFirstOrder,
    widths: [390, 780],
  },
  {
    key: "store-dashboard",
    alt: "Charkha Supply Co.'s dashboard: today's revenue, orders, orders to fulfil and low stock, the live order activity, products running low and today's top sellers",
    url: () => `${STORE}/`,
    viewport: { width: 1440, height: 900 },
    scale: 2,
    widths: [640, 960, 1440],
  },
  {
    key: "crm-pipeline",
    alt: "The Navree CRM's deal pipeline in its Next.js front end: lead, qualified, proposal and negotiation columns of deal cards",
    file: "public/images/crm/next/pipeline.webp",
    widths: [640, 960, 1440],
  },
  {
    key: "tile-table",
    alt: "The store's orders table: search, five filters with a date filter applied, and rows with status, fulfilment and payment badges and totals in rupees",
    url: twoDays,
    viewport: { width: 1440, height: 900 },
    scale: 2,
    clip: async () => ({ x: 272, y: 100, width: 720, height: 450 }),
    widths: [480, 960],
  },
  {
    key: "tile-live",
    alt: "The store's live order activity: who placed, paid for or delivered which order, with amounts in rupees, cities with flags and times in IST",
    url: () => `${STORE}/`,
    viewport: { width: 1440, height: 900 },
    scale: 2,
    clip: async () => ({ x: 272, y: 272, width: 768, height: 480 }),
    widths: [480, 960],
  },
  {
    key: "tile-dates",
    alt: "A date range picker open: presets on the left, from today to this year, and two months side by side",
    url: () => `${SITE}/ui-preview/date-range-picker/presets.html`,
    viewport: { width: 760, height: 640 },
    scale: 2,
    // A start picked and the pointer on an end: the range the picker would take, before it closes.
    prepare: async (page) => {
      await page.locator("button").first().click();
      const open = page.locator("[popover]:popover-open").first();
      await open.locator("button", { hasText: /^10$/ }).first().click();
      await open.locator("button", { hasText: /^18$/ }).first().hover();
      await page.waitForTimeout(300);
    },
    clip: async (page) =>
      framed(await around(page, ["[popover]:popover-open", "button >> nth=0"]), 12, 1.6, { width: 760, height: 640 }),
    widths: [480, 960],
  },
  {
    key: "tile-phone",
    alt: "A phone number field showing a saved Indian number in the national format, 098140 12345, with +91 chosen and the country list open at India",
    url: () => `${SITE}/ui-preview/phone-input/saved.html`,
    // Tall enough that the list opens under its field rather than over it.
    viewport: { width: 760, height: 900 },
    scale: 2,
    prepare: async (page) => {
      await page.locator("button").first().click();
      await page.waitForTimeout(400);
    },
    clip: async (page) =>
      framed(await around(page, ['[data-slot="field"]', "[popover]:popover-open"]), 12, 1.6, { width: 760, height: 900 }),
    widths: [480, 960],
  },
  {
    key: "tile-flags",
    alt: "India's flag first and largest; under it the flags of Nepal, Bhutan, Bangladesh, Sri Lanka, the UAE, Singapore, Japan, the UK and the US; and badges with icons for a store, shipped, paid in rupees and Ludhiana",
    url: () => `${SITE}/ui-preview/flags/with-icons.html`,
    viewport: { width: 640, height: 400 },
    // Close around the example and drawn at 3x, so the flags show large and sharp on the card.
    scale: 3,
    clip: async (page) => framed(await around(page, ['[data-slot="flag"]', '[data-slot="badge"]']), 28, 1.6, { width: 640, height: 400 }),
    widths: [480, 960],
  },
  {
    key: "tile-command",
    alt: "A command palette: a search field, a Go to group with icons and a keyboard shortcut, and an Account group with an item held back and the reason",
    url: () => `${SITE}/ui-preview/command/default.html`,
    viewport: { width: 720, height: 560 },
    scale: 2,
    clip: async (page) => framed(await around(page, ['[data-slot="command"]']), 16, 1.6, { width: 720, height: 560 }),
    widths: [480, 960],
  },
];

async function encode(key: string, png: string, widths: number[]): Promise<Omit<ManifestImage, "key" | "alt">> {
  const meta = await sharp(png).metadata();
  const full = { width: meta.width ?? 0, height: meta.height ?? 0 };
  const avif: ManifestImage["avif"] = [];
  const webp: ManifestImage["webp"] = [];
  const top = Math.min(Math.max(...widths), full.width);
  for (const width of widths.filter((w) => w <= full.width)) {
    const a = await sharp(png)
      .resize({ width, withoutEnlargement: true })
      .avif({ quality: 60, effort: 6, chromaSubsampling: "4:4:4" })
      .toFile(path.join(OUT, `${key}-${width}.avif`));
    const w = await sharp(png)
      .resize({ width, withoutEnlargement: true })
      .webp({ quality: 82, effort: 6, smartSubsample: true })
      .toFile(path.join(OUT, `${key}-${width}.webp`));
    avif.push({ src: `/images/showcase/${key}-${width}.avif`, width });
    webp.push({ src: `/images/showcase/${key}-${width}.webp`, width });
    console.log(`  ${key}-${width}: avif ${(a.size / 1024).toFixed(0)} KB, webp ${(w.size / 1024).toFixed(0)} KB`);
  }
  return { width: top, height: Math.round((full.height * top) / full.width), avif, webp };
}

async function main() {
  const only = new Set(process.argv.slice(2));
  const todo = captures.filter((c) => only.size === 0 || only.has(c.key));
  if (only.size && todo.length !== only.size) {
    throw new Error(`showcase-images: unknown key in ${[...only].join(", ")}; known: ${captures.map((c) => c.key).join(", ")}`);
  }
  fs.mkdirSync(OUT, { recursive: true });
  fs.mkdirSync(RAW, { recursive: true });
  const previous: { images: ManifestImage[] } = fs.existsSync(MANIFEST)
    ? JSON.parse(fs.readFileSync(MANIFEST, "utf8"))
    : { images: [] };
  const images = new Map(previous.images.map((image) => [image.key, image]));

  const browser = await chromium.launch();
  try {
    for (const capture of todo) {
      console.log(capture.key);
      let png: string;
      if ("file" in capture) {
        png = path.join(WEBSITE, capture.file);
      } else {
        const context = await browser.newContext({
          viewport: capture.viewport,
          deviceScaleFactor: capture.scale,
          colorScheme: "dark",
          reducedMotion: "reduce",
        });
        const page = await context.newPage();
        await page.goto(capture.url(), { waitUntil: "networkidle" });
        await page.evaluate(() => document.fonts.ready);
        await capture.prepare?.(page);
        png = path.join(RAW, `${capture.key}.png`);
        await page.screenshot({ path: png, clip: capture.clip ? await capture.clip(page) : undefined, animations: "disabled" });
        await context.close();
      }
      images.set(capture.key, { key: capture.key, alt: capture.alt, ...(await encode(capture.key, png, capture.widths)) });
    }
  } finally {
    await browser.close();
  }

  const ordered = captures.map((c) => images.get(c.key)).filter((image) => image !== undefined);
  const manifest = {
    captured: istDate(0),
    note: "Written by scripts/showcase-images.ts from the apps running (cargo run --release) and the site's preview pages. Each width is an AVIF (4:4:4) and a WebP.",
    images: ordered,
  };
  fs.writeFileSync(MANIFEST, `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`showcase-images: ${todo.length} captured, ${ordered.length} in ${path.relative(WEBSITE, MANIFEST)}`);
}

await main();
