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
