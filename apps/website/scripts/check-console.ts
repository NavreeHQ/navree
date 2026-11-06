/**
 * Loads key pages of the built site in a browser and fails on any console
 * error or uncaught exception, including React hydration errors. Run after
 * `pnpm build`:
 *
 *   node scripts/check-console.ts                serves out/ itself
 *   BASE_URL=http://127.0.0.1:3211 node scripts/check-console.ts
 *   PAGES=/,/about/ node scripts/check-console.ts
 *
 * Each page loads twice: as built, and as the CDN in front of the live site
 * can deliver it, with email addresses rewritten by Cloudflare's Email
 * Address Obfuscation (each address in the text becomes a
 * "[email protected]" link, each `mailto:` link points at
 * `/cdn-cgi/l/email-protection`, except between `<!--email_off-->` and
 * `<!--/email_off-->`). React fails to hydrate a page whose text was
 * rewritten (error #418), which `scripts/postbuild.ts` prevents by marking
 * the pages that show an address.
 */
import type { ChildProcess } from "node:child_process";

import { chromium, type Browser, type Page } from "playwright";

import { serveOut } from "./lib/serve-out.ts";
import { requireOut } from "./lib/site-html.ts";

const DEFAULT_PAGES = ["/", "/about/", "/docs/ai/mcp-servers/", "/ui/"];

/**
 * How long a page may take to load and go quiet. The server is already
 * answering by then (`serveOut` waits for it, as long as it needs), but its
 * first answer for each file is compressed on the way out, and a cold machine
 * takes a while over the larger pages.
 */
const NAVIGATION_TIMEOUT_MS = 90_000;
const pagesToCheck = (process.env.PAGES ?? DEFAULT_PAGES.join(",")).split(",").map((p) => p.trim()).filter(Boolean);

const EMAIL = /[\w.%+-]+@[\w-]+(?:\.[\w-]+)*\.[a-z]{2,}/gi;
const hex = (s: string) => Buffer.from(s).toString("hex");

/**
 * What the CDN's address rewriting does to a page: addresses in the text
 * and in `mailto:` links are replaced, except inside scripts and styles and
 * between the `email_off` markers.
 */
function rewriteAddresses(html: string): string {
  const skip = /<!--email_off-->[\s\S]*?<!--\/email_off-->|<script[\s\S]*?<\/script>|<style[\s\S]*?<\/style>/gi;
  let out = "";
  let last = 0;
  const rewrite = (part: string) =>
    part
      .replace(/href="mailto:([^"]+)"/gi, (_, address: string) => `href="/cdn-cgi/l/email-protection#${hex(address)}"`)
      .replace(/>([^<]*)</g, (_, text: string) =>
        `>${text.replace(EMAIL, (address) => `<a href="/cdn-cgi/l/email-protection" class="__cf_email__" data-cfemail="${hex(address)}">[email&#160;protected]</a>`)}<`,
      );
  for (const m of html.matchAll(skip)) {
    out += rewrite(html.slice(last, m.index)) + m[0];
    last = (m.index ?? 0) + m[0].length;
  }
  return out + rewrite(html.slice(last));
}

async function main() {
  let server: ChildProcess | null = null;
  let base = process.env.BASE_URL ?? "";
  if (!base) {
    requireOut("check-console");
    ({ base, server } = await serveOut("check-console"));
  }

  const problems: string[] = [];
  let browser: Browser | null = null;
  try {
    browser = await chromium.launch();
    for (const rewritten of [false, true]) {
      const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
      if (rewritten) {
        await context.route("**/*", async (route) => {
          if (route.request().resourceType() !== "document") return route.continue();
          const response = await route.fetch();
          await route.fulfill({ response, body: rewriteAddresses(await response.text()) });
        });
      }
      const page: Page = await context.newPage();
      page.setDefaultNavigationTimeout(NAVIGATION_TIMEOUT_MS);
      let current = "";
      const label = () => `${current}${rewritten ? " (addresses rewritten by the CDN)" : ""}`;
      page.on("pageerror", (error) => problems.push(`${label()}: ${error.message.split("\n")[0]}`));
      page.on("console", (message) => {
        if (message.type() === "error") problems.push(`${label()}: ${message.text().split("\n")[0]}`);
      });
      for (const url of pagesToCheck) {
        current = url;
        await page.goto(`${base}${url}`, { waitUntil: "networkidle" });
        // The app's scripts start after the first paint; give hydration time to finish.
        await page.waitForTimeout(2500);
      }
      await context.close();
    }
  } finally {
    await browser?.close();
    server?.kill();
  }

  if (problems.length) {
    console.error(`check-console: ${problems.length} problem(s):\n- ${problems.join("\n- ")}`);
    process.exit(1);
  }
  console.log(`check-console: ${pagesToCheck.length} pages load with no console errors, as built and with addresses rewritten by the CDN`);
}

await main();
