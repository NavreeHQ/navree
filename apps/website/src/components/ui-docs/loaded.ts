/**
 * What loads each of the toolkit's files, in words, for the table of files on the
 * overview page and its Markdown copy. The files themselves, and their sizes, come
 * from `content/ui/toolkit.json` (`cargo run -p ui-docs`): every script,
 * stylesheet and data file the toolkit builds is listed, and a file without a line
 * here says "A page that uses it" until it gets one.
 *
 * Node runs this with the types stripped: no imports.
 */
export const loadedWhen: Record<string, string> = {
  "navree-ui": "Every page: tokens, controls, display parts, overlays, toasts and the shell",
  core: "A form, a region or a command",
  overlay: "A popover, tooltip, hover card, menu or picker",
  sidebar: "The app shell",
  table: "A data table",
  live: "A region, table or list that follows an event stream",
  select: "A select, combobox, multi-select or command palette",
  dates: "A calendar, or a date or time picker",
  menus: "A menu, tabs, a toggle group, or a table row's actions",
  slider: "A slider",
  tree: "A tree view",
  toolbar: "A toolbar",
  split: "A split pane",
  keys: "A single-key shortcut",
  lazy: "A lazy list",
  sortable: "A sortable list or board that moves items",
  upload: "An upload",
  otp: "A one-time code field",
  carousel: "A carousel",
  charts: "A line, area, bar, donut or funnel chart",
  menubar: "A menu bar or a context menu",
  phone: "A phone input",
  "phone-data": "The first focus of a phone input",
  speculation: "A link marked with prefetch",
  "theme-builder": "The theme builder",
  markdown: "Markdown text",
  "upload-styles": "An upload",
  "otp-styles": "A one-time code field",
  "carousel-styles": "A carousel",
  "charts-styles": "A line, area, bar, donut or funnel chart",
  "menubar-styles": "A menu bar or a context menu",
  "table-styles": "A data table",
  "select-styles": "A select, combobox, multi-select or command palette",
  "phone-styles": "A phone input",
  "slider-styles": "A slider",
  "menus-styles": "A menu, tabs, a toggle group, or a table row's actions",
  "split-styles": "A split pane",
  "tree-styles": "A tree view",
  "lazy-styles": "A lazy list",
  "toolbar-styles": "A toolbar",
  "dates-styles": "A calendar, or a date or time picker",
  islands: "An island",
  parts: "A colour picker, a clearable input, an indeterminate checkbox or an avatar with a fallback",
  "parts-styles": "An avatar group, a trace, a questionnaire, an alert's actions, a card variant or a sparkline in a cell",
  genui: "A conversation or its composer",
};

/** The files the overview lists: scripts, stylesheets and the data they fetch, the stylesheet first and the core runtime after it. The fonts are described in the text. */
export function listedAssets<T extends { name: string; ext: string }>(assets: T[]): T[] {
  const shown = assets.filter((a) => a.ext === "js" || a.ext === "css" || a.ext === "json");
  const first = ["navree-ui", "core"];
  return [...first.flatMap((name) => shown.filter((a) => a.name === name)), ...shown.filter((a) => !first.includes(a.name))];
}
