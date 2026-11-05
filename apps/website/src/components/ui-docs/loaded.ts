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
  "navree-ui": "Every page",
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
  menubar: "A menu bar or a context menu",
  phone: "A phone input",
  "phone-data": "The first focus of a phone input",
  speculation: "A link marked with prefetch",
  "theme-builder": "The theme builder",
  markdown: "Markdown text",
  "upload-styles": "An upload",
  "otp-styles": "A one-time code field",
  "carousel-styles": "A carousel",
  "menubar-styles": "A menu bar or a context menu",
};

/** The files the overview lists: scripts, stylesheets and the data they fetch, the stylesheet first and the core runtime after it. The fonts are described in the text. */
export function listedAssets<T extends { name: string; ext: string }>(assets: T[]): T[] {
  const shown = assets.filter((a) => a.ext === "js" || a.ext === "css" || a.ext === "json");
  const first = ["navree-ui", "core"];
  return [...first.flatMap((name) => shown.filter((a) => a.name === name)), ...shown.filter((a) => !first.includes(a.name))];
}
