/**
 * Choosing a theme and a mode at run time. `themes.css` reads two attributes, `data-theme` and
 * `data-mode`; shadcn's `dark:` classes read a `dark` class. `applyAppearance` sets all three, and
 * `appearanceScript` does the same before the first paint, so a stored choice does not flash.
 */
import type { ThemeId } from "./themes.js";

/** `light`, `dark`, or `system` (the visitor's operating system decides). */
export type Mode = "light" | "dark" | "system";

export const modes: readonly Mode[] = ["light", "dark", "system"];

export interface Appearance {
  /** A built-in theme, or the id of a theme of your own (`[data-theme="mine"]` in your CSS). Default `navree`. */
  theme?: ThemeId | (string & {});
  /** Default `dark`, which is what `theme.css` alone gives. */
  mode?: Mode;
}

/** What a root element needs: attributes and a class list. */
export type AppearanceRoot = Pick<Element, "setAttribute" | "classList">;

/** The part of `window.matchMedia` that is used. */
type MatchMedia = (query: string) => Pick<MediaQueryList, "matches" | "addEventListener" | "removeEventListener">;

const THEME_ID = /^[a-z0-9-]+$/;

/** The mode that is drawn: `system` is whatever the visitor's system prefers. */
export function resolveMode(mode: Mode, prefersDark: boolean): "light" | "dark" {
  if (mode === "system") return prefersDark ? "dark" : "light";
  return mode;
}

/** An appearance read from stored text (JSON); anything unknown is left out. */
export function parseAppearance(text: string | null | undefined): Appearance {
  if (!text) return {};
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return {};
  }
  if (typeof value !== "object" || value === null) return {};
  const { theme, mode } = value as { theme?: unknown; mode?: unknown };
  const out: Appearance = {};
  if (typeof theme === "string" && THEME_ID.test(theme)) out.theme = theme;
  if (typeof mode === "string" && (modes as readonly string[]).includes(mode)) out.mode = mode as Mode;
  return out;
}

/**
 * Sets `data-theme` and `data-mode` on `root` (usually `document.documentElement`) and the `dark`
 * class to match. In `system` mode the class follows the visitor's system until the function
 * this returns is called.
 *
 * ```ts
 * const stop = applyAppearance(document.documentElement, { theme: "ocean", mode: "system" });
 * ```
 */
export function applyAppearance(
  root: AppearanceRoot,
  appearance: Appearance = {},
  matchMedia: MatchMedia | undefined = globalThis.matchMedia?.bind(globalThis),
): () => void {
  const theme = appearance.theme ?? "navree";
  const mode = appearance.mode ?? "dark";
  root.setAttribute("data-theme", theme);
  root.setAttribute("data-mode", mode);

  const query = matchMedia?.("(prefers-color-scheme: dark)");
  const sync = () => root.classList.toggle("dark", resolveMode(mode, query?.matches ?? true) === "dark");
  sync();
  if (mode !== "system" || !query) return () => undefined;
  query.addEventListener("change", sync);
  return () => query.removeEventListener("change", sync);
}

export interface AppearanceScriptOptions {
  /** Where the choice is stored (`localStorage`, as JSON `{theme, mode}`). Default `navree-appearance`. */
  storageKey?: string;
  /** What to use when nothing is stored. */
  defaults?: Appearance;
}

/**
 * A script for `<head>` that applies the stored choice before the first paint. Put it inline, before
 * the stylesheet's body renders (`<script dangerouslySetInnerHTML>` in React, `useHead` in Nuxt).
 * Store a choice with `localStorage.setItem(key, JSON.stringify({ theme, mode }))`.
 */
export function appearanceScript(options: AppearanceScriptOptions = {}): string {
  // Inline in HTML: a `<` in a value must not be able to close the script element.
  const embed = (value: string) => JSON.stringify(value).replace(/</g, "\\u003c");
  const key = embed(options.storageKey ?? "navree-appearance");
  const theme = embed(options.defaults?.theme ?? "navree");
  const mode = embed(options.defaults?.mode ?? "dark");
  return (
    "(function(){var d=document.documentElement,a={};" +
    `try{a=JSON.parse(localStorage.getItem(${key})||"{}")||{}}catch(e){}` +
    `var t=typeof a.theme==="string"&&/^[a-z0-9-]+$/.test(a.theme)?a.theme:${theme},` +
    `m=["light","dark","system"].indexOf(a.mode)>=0?a.mode:${mode};` +
    'd.setAttribute("data-theme",t);d.setAttribute("data-mode",m);' +
    'd.classList.toggle("dark",m==="dark"||(m==="system"&&matchMedia("(prefers-color-scheme: dark)").matches))' +
    "})()"
  );
}
