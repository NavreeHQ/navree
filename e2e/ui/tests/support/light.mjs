// The light values of the tokens, read from the toolkit's one source of them (`src/tokens.rs`), for a suite
// that checks a page in light. The toolkit ships dark only, so the light page is a fixture: read from the
// tokens, it can't drift from what a light theme draws, as hand-copied values did (an amber lighter than the
// toolkit's own made a warning badge on a hovered row fall short in the suites alone).

import { readFileSync } from "node:fs";

const tokens = readFileSync(new URL("../../../../crates/navree-ui/src/tokens.rs", import.meta.url), "utf8");

/** `:root` in light: `color-scheme` and every colour token at its light value. */
export const LIGHT =
  ":root{color-scheme:light" +
  [...tokens.matchAll(/t\("([a-z0-9-]+)",\s*"(#[0-9a-f]{6})",\s*"(#[0-9a-f]{6})"\)/g)].map((m) => `;--${m[1]}:${m[3]}`).join("") +
  "}";

/**
 * Gives every page of `context` the light tokens before its first style is worked out, ahead of the toolkit's
 * own (`:root:root` outranks its `:root`). Tokens changed after a page has loaded can leave WebKit's table cells
 * with the colours they had (their computed colour stays the dark one while the body's turns light), so a check
 * that reads the cells' colours gives the page its light tokens from the start.
 * @param {import("playwright").BrowserContext} context
 */
export function lightFromTheStart(context) {
  return context.addInitScript((css) => {
    const add = () => {
      const style = document.createElement("style");
      style.textContent = css;
      document.documentElement.append(style);
    };
    if (document.documentElement) add();
    else new MutationObserver((_, seen) => {
      if (!document.documentElement) return;
      seen.disconnect();
      add();
    }).observe(document, { childList: true });
  }, LIGHT.replace(/^:root/, ":root:root"));
}
