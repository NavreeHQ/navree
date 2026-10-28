/**
 * Renders the favicon set from the Navree mark (`MARK_PATHS` in
 * src/components/site/logo.tsx) with Playwright's Chromium, and packs the
 * small sizes into an ICO. The output is committed; run this again only when
 * the mark changes:
 *
 *   node scripts/icons.ts
 *
 * Writes:
 * - src/app/favicon.ico        16, 32 and 48 px PNGs in one ICO (the tab icon
 *                              where SVG icons aren't supported)
 * - src/app/icon.svg           the scalable tab icon
 * - src/app/apple-icon.png     180 px home-screen icon (iOS rounds the corners)
 * - public/icon-192.png        manifest icons, the rounded tile
 * - public/icon-512.png
 * - public/icon-maskable-512.png  full-bleed, the mark inside the maskable safe zone
 *
 * The mark sits on the ink-0 tile. At 16, 32 and 48 px it is drawn from a
 * copy whose horizontal edges fall on whole pixels, so the hull, the
 * waterline gap and the fin stay sharp at tab size.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { chromium } from "playwright";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const ACCENT = "#3dff8f";
const INK = "#040605";

const logo = fs.readFileSync(path.join(ROOT, "src/components/site/logo.tsx"), "utf8");
const block = /MARK_PATHS = \[([\s\S]*?)\] as const/.exec(logo)?.[1];
const MARK = [...(block ?? "").matchAll(/"([^"]+)"/g)].map((m) => m[1]);
if (MARK.length !== 3) throw new Error("icons: could not read MARK_PATHS from logo.tsx");

/**
 * The fin as drawn in the PNGs: the same visible shape, its top tucked one
 * unit up under the keel band, so the two shapes overlap instead of meeting
 * on a shared edge (which leaves a faint anti-aliased seam at large sizes).
 */
const MARK_PNG = [MARK[0], MARK[1], "M12.82 17.3H19.79L16.4 26H14.4Z"];

/**
 * The mark with its horizontal edges moved by at most 0.3 units onto even
 * grid lines (whole pixels at 16 px): hull 6.5 to 12, waterline gap 12 to
 * 14, keel band 14 to 18, fin 18 to 26 (tucked under the band as above).
 */
const MARK_SNAPPED = [
  "M1.5 9 L30.5 6.5 C29.6 8.9 28.6 10.7 27.4 12 L3.4 12 C2.6 11.2 2 10.3 1.5 9Z",
  "M4.4 14 L26 14 C24.4 16 22.4 17.3 19.5 18 L10 18 C7.6 17.5 5.8 16 4.4 14Z",
  "M12.83 17H19.78L16.4 26H14.4Z",
];

const paths = (d: string[]) => d.map((p) => `<path d="${p}"/>`).join("\n    ");

/** The tab icon: the mark on a rounded ink tile, as `icon.svg`. */
function tile(mark: string[], shift: number): string {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">
  <rect width="32" height="32" rx="7" fill="${INK}"/>
  <g fill="${ACCENT}"${shift ? ` transform="translate(0 ${shift})"` : ""}>
    ${paths(mark)}
  </g>
</svg>
`;
}

/** Full-bleed ink with the mark scaled to `scale` of the side, centred (home-screen and maskable icons). */
function fullBleed(scale: number): string {
  const offset = (32 - 32 * scale) / 2;
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">
  <rect width="32" height="32" fill="${INK}"/>
  <g fill="${ACCENT}" transform="translate(${offset} ${offset - 0.5 * scale}) scale(${scale})">
    ${paths(MARK_PNG)}
  </g>
</svg>
`;
}

/** A PNG of `svg` at `size` px, transparent where the SVG draws nothing. */
async function render(page: import("playwright").Page, svg: string, size: number): Promise<Buffer> {
  await page.setViewportSize({ width: size, height: size });
  const html = `<!doctype html><html><body style="margin:0;background:transparent">
    <img src="data:image/svg+xml;base64,${Buffer.from(svg).toString("base64")}" width="${size}" height="${size}" style="display:block">
  </body></html>`;
  await page.setContent(html);
  await page.locator("img").evaluate((img: HTMLImageElement) => img.decode());
  return page.screenshot({ omitBackground: true, clip: { x: 0, y: 0, width: size, height: size } });
}

/** An ICO holding PNG images (supported by every browser that still reads favicon.ico). */
function ico(images: { size: number; png: Buffer }[]): Buffer {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0); // reserved
  header.writeUInt16LE(1, 2); // type: icon
  header.writeUInt16LE(images.length, 4);
  let offset = 6 + 16 * images.length;
  const entries = images.map(({ size, png }) => {
    const entry = Buffer.alloc(16);
    entry.writeUInt8(size >= 256 ? 0 : size, 0); // width
    entry.writeUInt8(size >= 256 ? 0 : size, 1); // height
    entry.writeUInt8(0, 2); // palette size
    entry.writeUInt8(0, 3); // reserved
    entry.writeUInt16LE(1, 4); // colour planes
    entry.writeUInt16LE(32, 6); // bits per pixel
    entry.writeUInt32LE(png.length, 8);
    entry.writeUInt32LE(offset, 12);
    offset += png.length;
    return entry;
  });
  return Buffer.concat([header, ...entries, ...images.map((i) => i.png)]);
}

async function main() {
  const browser = await chromium.launch();
  const page = await browser.newPage({ deviceScaleFactor: 1 });
  const write = (file: string, data: Buffer | string) => {
    fs.writeFileSync(path.join(ROOT, file), data);
    console.log(`icons: ${file} (${Buffer.byteLength(data)} bytes)`);
  };

  const iconSvg = tile(MARK, -0.5);
  write("src/app/icon.svg", iconSvg);

  const small = tile(MARK_SNAPPED, 0);
  const sizes = [16, 32, 48];
  const pngs = [];
  for (const size of sizes) pngs.push({ size, png: await render(page, small, size) });
  write("src/app/favicon.ico", ico(pngs));

  write("src/app/apple-icon.png", await render(page, fullBleed(132 / 180), 180));
  const tilePng = tile(MARK_PNG, -0.5);
  write("public/icon-192.png", await render(page, tilePng, 192));
  write("public/icon-512.png", await render(page, tilePng, 512));
  // The maskable safe zone is the centred circle of 80% diameter; the mark (29 by 19 units) fits it at 0.6.
  write("public/icon-maskable-512.png", await render(page, fullBleed(0.6), 512));

  await browser.close();
}

await main();
