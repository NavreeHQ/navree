// The table: what only a browser can do. Filter, sort and page are links and a
// form, and work without this. Here: the selection across pages, dragging and
// keying column widths and order, the grid keys, and keeping the address bar and
// the form's controls true to what the server drew.

const STICKY_SELECT = "[data-sticky=s]";

/** @typedef {{mode: "ids" | "all", ids: string[], ex: string[], key: string}} Selection */

/** @param {Element} root */
const storeKey = (root) => "navree-table:" + root.id + ":" + location.pathname;

/** @param {Element} root @returns {Selection} */
function load(root) {
  try {
    const saved = JSON.parse(sessionStorage.getItem(storeKey(root)) || "null");
    if (saved && Array.isArray(saved.ids) && Array.isArray(saved.ex)) return saved;
  } catch {}
  return { mode: "ids", ids: [], ex: [], key: "" };
}

/** @param {Element} root @param {Selection} selection */
function save(root, selection) {
  try {
    if (selection.mode === "ids" && !selection.ids.length) sessionStorage.removeItem(storeKey(root));
    else sessionStorage.setItem(storeKey(root), JSON.stringify(selection));
  } catch {}
}

/** @param {Element} root */
const labels = (root) => {
  try {
    return JSON.parse(root.getAttribute("data-l") || "{}");
  } catch {
    return {};
  }
};

/** Counts in lakhs and crores (12,34,567) when the table's labels say `grouping: "indian"`. @param {Element} root @param {number} n */
const grouped = (root, n) => n.toLocaleString(labels(root).grouping === "indian" ? "en-IN" : "en-US");

/** @param {Element} root @param {string} template @param {number} n @param {number} [total] */
const fill = (root, template, n, total) => (template || "").replace("{n}", grouped(root, n)).replace("{total}", grouped(root, total || 0));

/** @param {Element} root */
const rowBoxes = (root) => /** @type {HTMLInputElement[]} */ ([...root.querySelectorAll("[data-ui-select-row]")]);

/** @param {Element} root */
const total = (root) => Number(root.getAttribute("data-total") || 0);

/** Says something to assistive technology, through the bar's live count. @param {Element} root @param {string} text */
function announce(root, text) {
  const live = root.querySelector("[data-slot=bulk-count]");
  if (live) live.textContent = text;
}

/** How many rows are selected. @param {Element} root @param {Selection} selection */
const selectedCount = (root, selection) => (selection.mode === "all" ? Math.max(0, total(root) - selection.ex.length) : selection.ids.length);

/** Draws the selection. @param {Element} root */
function drawSelection(root) {
  const bar = root.querySelector("[data-slot=table-bulk]");
  if (!bar) return;
  const selection = load(root);
  const words = labels(root);
  const key = root.getAttribute("data-filter-key") || "";
  // "All results" meant the results of that filter. It ends with the filter.
  let ended = false;
  if (selection.mode === "all" && selection.key !== key) {
    selection.mode = "ids";
    selection.ids = [];
    selection.ex = [];
    save(root, selection);
    ended = true;
  }
  const excluded = new Set(selection.ex);
  const chosen = new Set(selection.ids);
  const boxes = rowBoxes(root);
  let onPage = 0;
  let live = 0;
  for (const box of boxes) {
    const on = selection.mode === "all" ? !excluded.has(box.value) : chosen.has(box.value);
    box.checked = on;
    box.closest("tr")?.toggleAttribute("data-selected", on);
    if (!box.disabled) live++;
    if (on) onPage++;
  }
  const head = /** @type {HTMLInputElement | null} */ (root.querySelector("[data-ui-select-page]"));
  if (head) {
    head.checked = live > 0 && onPage >= live;
    head.indeterminate = onPage > 0 && onPage < live;
    head.disabled = !live;
  }
  const n = selectedCount(root, selection);
  const count = bar.querySelector("[data-slot=bulk-count]");
  const all = /** @type {HTMLElement | null} */ (bar.querySelector("[data-ui-select-all]"));
  const everything = total(root);
  const offer = root.hasAttribute("data-can-select-all") && selection.mode === "ids" && head?.checked && everything > boxes.length && selection.ids.length === onPage;
  if (all) {
    all.hidden = !offer;
    all.textContent = fill(root, words.all, everything);
  }
  let text;
  if (selection.mode === "all") text = selection.ex.length ? fill(root, words.some, n, everything) : fill(root, words.allOn, n);
  else if (offer) text = fill(root, words.page, onPage);
  else {
    text = fill(root, words.selected, n);
    if (n > onPage) text += " · " + fill(root, words.off, n - onPage);
  }
  if (count && count.textContent !== (ended ? words.ended : text)) count.textContent = ended ? words.ended : text;
  root.toggleAttribute("data-has-selection", n > 0 || ended);
}

/** Goes to `url` through the runtime, in the region. @param {Element} root @param {string} url */
function visit(root, url) {
  const a = document.createElement("a");
  a.href = url;
  a.setAttribute("data-ui-in-region", "");
  a.hidden = true;
  root.append(a);
  a.click();
  a.remove();
}

/** The table's URL with `name` set. @param {Element} root @param {string} name @param {string} value */
function withParam(root, name, value) {
  const url = new URL(root.getAttribute("data-ui-src") || location.href, location.href);
  url.searchParams.set(name, value);
  return url.href;
}

/** @param {Element} root */
const columnCols = (root) => /** @type {HTMLTableColElement[]} */ ([...root.querySelectorAll("col[data-col]")]);

/** Fixes every column at its drawn width, so changing one leaves the others alone. @param {Element} root */
function freezeWidths(root) {
  const table = root.querySelector("[data-slot=table]");
  if (!table || table.hasAttribute("data-fixed")) return;
  for (const col of columnCols(root)) {
    const th = root.querySelector(`th[data-col="${col.getAttribute("data-col")}"]`);
    if (th) col.setAttribute("width", String(Math.round(th.getBoundingClientRect().width)));
  }
  table.setAttribute("data-fixed", "");
}

/** Saves the widths in the URL. @param {Element} root */
function commitWidths(root) {
  const widths = columnCols(root).map((col) => col.getAttribute("data-col") + ":" + col.getAttribute("width")).join(",");
  visit(root, withParam(root, "w", widths));
}

/** @param {Element} root @param {Element} handle @param {number} width */
function setWidth(root, handle, width) {
  const name = handle.closest("th")?.getAttribute("data-col");
  const col = root.querySelector(`col[data-col="${name}"]`);
  const next = Math.max(40, Math.min(1600, Math.round(width)));
  col?.setAttribute("width", String(next));
  handle.setAttribute("aria-valuenow", String(next));
  measureSticky(root);
}

/** Sticky columns show an edge only once something has gone under them. @param {Element} root */
function markScroll(root) {
  const box = root.querySelector("[data-slot=table-container]");
  if (!box) return;
  const gone = Math.abs(box.scrollLeft);
  root.toggleAttribute("data-scrolled-start", gone > 0);
  root.toggleAttribute("data-scrolled-end", gone + box.clientWidth < box.scrollWidth - 1);
}

/** Places sticky columns: each starts where those before it end. @param {Element} root */
function measureSticky(root) {
  const el = /** @type {HTMLElement} */ (root);
  markScroll(root);
  // A table with nothing sticky but its boxes needs no offsets.
  if (!root.querySelector('[data-sticky="1"],[data-sticky=e2],tr[data-sticky-row]')) return;
  let at = root.querySelector(`thead ${STICKY_SELECT}`)?.getBoundingClientRect().width || 0;
  // What covers the box's start: a cell or control scrolled into view (by focus, or a script) stops beside it.
  let covered = at;
  el.style.setProperty("--s0", at + "px");
  const head = root.querySelector("thead");
  if (head) el.style.setProperty("--head-h", head.getBoundingClientRect().height + "px");
  for (const th of root.querySelectorAll("thead tr:not([data-slot=group-row]) th[data-sticky]")) {
    const name = th.getAttribute("data-sticky");
    if (name === "s" || name[0] === "e") continue;
    el.style.setProperty("--s" + name, at + "px");
    at += th.getBoundingClientRect().width;
    if (getComputedStyle(th).position === "sticky") covered = at;
  }
  // End columns the other way: each ends where those after it begin.
  let end = root.querySelector("thead th[data-sticky=e]")?.getBoundingClientRect().width || 0;
  for (const th of [...root.querySelectorAll("thead tr:not([data-slot=group-row]) th[data-sticky^=e]")].reverse()) {
    const name = th.getAttribute("data-sticky");
    if (name === "e") continue;
    el.style.setProperty("--" + name, end + "px");
    end += th.getBoundingClientRect().width;
  }
  el.style.setProperty("--sticky-start", covered + "px");
  el.style.setProperty("--sticky-end", end + "px");
}

/** Puts controls back to what the server drew: a tick the user made stays ticked by
 * the browser's rule even when the server's answer took it away. @param {Element} root */
function resetControls(root) {
  const form = /** @type {HTMLFormElement | null} */ (root.querySelector("form[data-ui-target]"));
  if (!form) return;
  const state = new URL(root.getAttribute("data-ui-src") || location.href, location.href).searchParams;
  for (const control of form.elements) {
    const input = /** @type {HTMLInputElement} */ (control);
    if (control === document.activeElement) {
      // Back or forward: the state is the URL's, even in the box being typed in.
      if (cameBack && input.nodeName === "INPUT" && input.type !== "hidden") input.value = state.get(input.name) || "";
      continue;
    }
    if (input.type === "checkbox") input.checked = input.defaultChecked;
    else if (input.nodeName === "INPUT" && input.type !== "hidden") input.value = input.defaultValue;
    else if (input.nodeName === "SELECT") {
      for (const option of /** @type {HTMLSelectElement} */ (control).options) option.selected = option.defaultSelected;
    }
  }
}

/** Sets up one table, after load and after each morph. @param {HTMLElement} root */
function sync(root) {
  const src = root.getAttribute("data-ui-src");
  if (src) {
    const url = new URL(src, location.href);
    // The server's spelling: a page past the end, an order of parameters.
    if (url.pathname === location.pathname && url.search !== location.search) history.replaceState(history.state, "", url.href);
  }
  root.setAttribute("data-ui-keep-attrs", "style data-ui-ready data-has-selection data-scrolled-start data-scrolled-end");
  root.setAttribute("data-ui-ready", "");
  for (const box of root.querySelectorAll("[data-ui-col-drag]")) box.setAttribute("draggable", "true");
  for (const handle of root.querySelectorAll("[data-slot=col-resize]")) {
    const th = handle.closest("th");
    if (th) handle.setAttribute("aria-valuenow", String(Math.round(th.getBoundingClientRect().width)));
  }
  resetControls(root);
  cameBack = false;
  measureSticky(root);
  // Measured again whenever a sticky column changes size: a width that settles after the first measure (a face that
  // loads late, a cell that fills in) moves what they cover.
  const any = /** @type {any} */ (root);
  any.uiSticky?.disconnect();
  any.uiSticky = window.ResizeObserver && new ResizeObserver(() => measureSticky(root));
  for (const th of root.querySelectorAll("thead th[data-sticky]")) any.uiSticky?.observe(th);
  drawSelection(root);
  if (root.hasAttribute("data-ui-grid")) setupGrid(root);
  const place = location.search.replace(/.*\b(page|cursor|before)=([^&]*).*/, "$2");
  const scroll = root.getAttribute("data-scroll-to");
  if (scroll && any.uiPlace !== undefined && any.uiPlace !== place) document.getElementById(scroll)?.scrollIntoView();
  any.uiPlace = place;
}

function syncAll() {
  for (const root of document.querySelectorAll("[data-ui-table]")) {
    // The server drew the table: what came before a sort here is no longer the last it drew.
    if (!root.hasAttribute("data-ui-confirm")) /** @type {any} */ (root).uiBefore = null;
    sync(/** @type {HTMLElement} */ (root));
  }
}

/** Whether back or forward is on its way. */
let cameBack = false;

/* The grid keys */

/** @param {HTMLElement} root */
function setupGrid(root) {
  const table = root.querySelector("[data-slot=table]");
  table?.setAttribute("role", "grid");
  const cells = /** @type {HTMLElement[]} */ ([...root.querySelectorAll("tbody td")]);
  let current = cells.find((cell) => cell.getAttribute("tabindex") === "0");
  for (const cell of cells) if (cell !== current) cell.setAttribute("tabindex", "-1");
  // The one tab stop starts on a cell with no control of its own.
  if (!current) (cells.find((cell) => !cell.querySelector("input,button,a[href],select")) || cells[0])?.setAttribute("tabindex", "0");
}

/** @param {HTMLElement} from @param {HTMLElement | null | undefined} to */
function moveTo(from, to) {
  if (!to) return;
  from.setAttribute("tabindex", "-1");
  to.setAttribute("tabindex", "0");
  to.focus();
}

/** @param {KeyboardEvent} e @param {HTMLElement} cell */
function gridKey(e, cell) {
  const row = /** @type {HTMLTableRowElement} */ (cell.parentElement);
  const rows = /** @type {HTMLTableRowElement[]} */ ([...(row.parentElement?.children || [])]);
  const at = cell.cellIndex;
  const rtl = getComputedStyle(cell).direction === "rtl";
  const byRow = (/** @type {number} */ delta) => {
    const target = rows[Math.max(0, Math.min(rows.length - 1, rows.indexOf(row) + delta))];
    return /** @type {HTMLElement | undefined} */ (target?.cells[Math.min(at, target.cells.length - 1)]);
  };
  let to;
  switch (e.key) {
    case "ArrowUp": to = byRow(-1); break;
    case "ArrowDown": to = byRow(1); break;
    case "ArrowLeft": to = row.cells[at + (rtl ? 1 : -1)]; break;
    case "ArrowRight": to = row.cells[at + (rtl ? -1 : 1)]; break;
    case "PageUp": to = byRow(-10); break;
    case "PageDown": to = byRow(10); break;
    case "Home": to = e.ctrlKey ? rows[0]?.cells[0] : row.cells[0]; break;
    case "End": to = e.ctrlKey ? rows[rows.length - 1]?.cells[at] : row.cells[row.cells.length - 1]; break;
    case "Enter":
    case "F2": {
      const inner = /** @type {HTMLElement | null} */ (cell.querySelector("a,button,input,select"));
      if (inner) {
        e.preventDefault();
        inner.focus();
      }
      return;
    }
    default: return;
  }
  e.preventDefault();
  moveTo(cell, /** @type {HTMLElement | undefined} */ (to));
}

/* Sorting the rows the page holds */

// A table that holds every row of its result (`data-ui-local`, which the server sets
// on one complete page) sorts them here at once. The link is still followed, so the
// server's answer comes after and has the last word: it confirms the order, or puts
// it right, and it redraws everything else the sort changes.

/** Whether `to` asks for the same rows as `from`, in another order. @param {URL} from @param {URL} to */
function onlySort(from, to) {
  const rest = (/** @type {URL} */ url) => {
    const params = new URLSearchParams(url.search);
    params.delete("sort");
    params.sort();
    return url.origin + url.pathname + "?" + params;
  };
  return rest(from) === rest(to);
}

const digits = /^\d{1,15}$/;

/**
 * A cell's value for sorting: what the server wrote in `data-v`, as a number in a column
 * of numbers. Empty is NULL, the smallest value, as on every store.
 * @param {HTMLTableRowElement} row @param {number} at @param {string} kind
 */
function valueOf(row, at, kind) {
  const raw = row.cells[at]?.getAttribute("data-v") ?? "";
  const number = kind === "n" ? Number(raw.replace(/[^\d.eE+-]/g, "")) : 0;
  return raw === "" || Number.isNaN(number) ? null : kind === "n" ? number : raw;
}

/**
 * Orders two values: NULL first, numbers as numbers, dates (written ISO) as their text,
 * keys of digits as numbers. Text is never ordered here: a database orders it by its
 * own collation.
 * @param {any} a @param {any} b @param {string} [kind]
 */
function compare(a, b, kind) {
  if (a === null || b === null) return a === b ? 0 : a === null ? -1 : 1;
  if (typeof a === "number" || (kind === "k" && digits.test(a) && digits.test(b))) return a - b;
  return a < b ? -1 : a > b ? 1 : 0;
}

/**
 * Puts the rows in the order `url` asks for, here, when the page holds all of them and
 * only the order changes. False when it can't: the server's answer does it all.
 * @param {HTMLElement} root @param {URL} url
 */
function sortHere(root, url) {
  const body = /** @type {HTMLTableSectionElement | null} */ (root.querySelector("[data-slot=table] > tbody"));
  const heads = [...root.querySelectorAll("[data-slot=table] > thead th[data-col]")];
  if (!body || !root.hasAttribute("data-ui-local") || !onlySort(new URL(root.getAttribute("data-ui-src") || "", location.href), url)) return false;
  const sort = url.searchParams.get("sort") ?? root.getAttribute("data-ui-sort-default") ?? "";
  // One column, or none (the key's order). An order by several shows ranks, and is the
  // server's to draw.
  if (sort.includes(",")) return false;
  const down = sort[0] === "-", name = down ? sort.slice(1) : sort;
  const th = /** @type {HTMLTableCellElement | undefined} */ (heads.find((cell) => cell.getAttribute("data-col") === name));
  // Only a column the server says the browser can read the way the store sorts it:
  // numbers and dates that show their own value.
  const kind = th?.getAttribute("data-kind") || "";
  if (name && !kind) return false;
  /** Each row's key, what it sorts by, the row and its details row, which moves with it. @type {any[][]} */
  const groups = [];
  for (const row of body.rows) {
    const key = row.getAttribute("data-key");
    if (!key) return false;
    if (row.getAttribute("data-slot") === "row-detail" && groups.length) groups[groups.length - 1].push(row);
    else groups.push([key, th ? valueOf(row, th.cellIndex, kind) : null, row]);
  }
  // Ties go by key, in the column's direction, as the server orders them.
  groups.sort((a, b) => (down ? -1 : 1) * (compare(a[1], b[1], kind) || compare(a[0], b[0], "k")));
  // What the server last drew, to put back if it can't confirm: rows, the headings' state
  // and their links' content, and the form's sort. A sort done here before the server
  // confirmed the last one keeps the state from before both.
  const links = heads.map((cell) => cell.querySelector("[data-slot=sort-link]"));
  const field = /** @type {HTMLInputElement | null} */ (root.querySelector("form[data-ui-target] input[type=hidden][name=sort]"));
  if (!root.hasAttribute("data-ui-confirm")) {
    /** @type {any} */ (root).uiBefore = [[...body.rows], heads.map((cell, i) => [cell, cell.getAttribute("aria-sort"), links[i], [...(links[i]?.childNodes || [])]]), field, field?.value];
  }
  /** An arrow for each state shown now: the icons are copied, never drawn here. */
  const arrows = new Map(heads.map((cell, i) => [cell.getAttribute("aria-sort") || "", links[i]?.querySelector("svg")]));
  let at = body.firstElementChild;
  for (const row of groups.flatMap((group) => group.slice(2))) {
    if (row === at) at = row.nextElementSibling;
    else {
      try {
        /** @type {any} */ (body).moveBefore(row, at);
      } catch {
        body.insertBefore(row, at);
      }
    }
  }
  heads.forEach((cell, i) => {
    const state = cell === th ? (down ? "descending" : "ascending") : "";
    const was = cell.getAttribute("aria-sort") || "";
    if (state) cell.setAttribute("aria-sort", state);
    else cell.removeAttribute("aria-sort");
    const link = links[i], icon = link?.querySelector("svg");
    // One column has no rank to show.
    link?.querySelector("[data-slot=sort-rank]")?.remove();
    // The arrow says the new state: one shown elsewhere, or the opposite one mirrored.
    const from = arrows.get(state) || (state && arrows.get(down ? "ascending" : "descending"));
    if (!icon || state === was || !from) return;
    const arrow = /** @type {Element} */ (from.cloneNode(true));
    arrow.toggleAttribute("data-flip", from.hasAttribute("data-flip") !== !arrows.has(state));
    icon.replaceWith(arrow);
  });
  if (field) field.value = sort;
  // The rows are right already: the server's answer only confirms them.
  root.setAttribute("data-ui-confirm", "");
  return true;
}

// The server couldn't confirm: what was done here is undone, and the table shows what
// the server last drew. Only a sort still waiting for its answer is undone: once the
// server has drawn the table, that is what a later failure leaves on screen.
document.addEventListener("ui:failed", (e) => {
  const root = /** @type {any} */ (/** @type {Element} */ (e.target).closest?.("[data-ui-table]"));
  if (!root) return;
  const waited = root.hasAttribute("data-ui-confirm");
  const [rows, sorts, field, sort] = root.uiBefore || [];
  root.uiBefore = null;
  root.removeAttribute("data-ui-confirm");
  if (!waited || !rows) return;
  rows[0]?.parentNode?.append(...rows);
  for (const [th, state, link, nodes] of sorts) {
    if (state === null) th.removeAttribute("aria-sort");
    else th.setAttribute("aria-sort", state);
    link?.replaceChildren(...nodes);
  }
  if (field) field.value = sort;
});

/* Wiring */

/** @type {HTMLInputElement | null} */
let anchor = null;
/** @type {HTMLElement | null} */
let confirmed = null;
/** @type {{root: Element, handle: Element, start: number, width: number, side: number} | null} */
let resizing = null;
/** @type {string | null} */
let dragged = null;
/** @type {WeakMap<Element, number>} */
const keyTimers = new WeakMap();

/** Sets rows on or off in the selection. @param {Element} root @param {string[]} keys @param {boolean} on */
function tick(root, keys, on) {
  const selection = load(root);
  const set = new Set(selection.mode === "all" ? selection.ex : selection.ids);
  // In "all" mode, the set holds the rows left out.
  const add = selection.mode === "all" ? !on : on;
  for (const key of keys) {
    if (add) set.add(key);
    else set.delete(key);
  }
  if (selection.mode === "all") selection.ex = [...set];
  else selection.ids = [...set];
  save(root, selection);
  drawSelection(root);
}

document.addEventListener("click", (e) => {
  const target = /** @type {Element} */ (e.target);
  const root = target.closest?.("[data-ui-table]");
  if (!root) return;
  const box = /** @type {HTMLInputElement} */ (target);
  if (target.matches("[data-ui-select-row]")) {
    const boxes = rowBoxes(root);
    if (e.shiftKey && anchor?.isConnected && anchor !== box) {
      const a = boxes.indexOf(anchor), b = boxes.indexOf(box);
      const range = boxes.slice(Math.min(a, b), Math.max(a, b) + 1).filter((item) => !item.disabled);
      tick(root, range.map((item) => item.value), box.checked);
    } else tick(root, [box.value], box.checked);
    anchor = box;
    return;
  }
  if (target.matches("[data-ui-select-page]")) {
    tick(root, rowBoxes(root).filter((item) => !item.disabled).map((item) => item.value), box.checked);
    return;
  }
  if (target.closest("[data-ui-select-all]")) {
    save(root, { mode: "all", ids: [], ex: [], key: root.getAttribute("data-filter-key") || "" });
    drawSelection(root);
    /** @type {HTMLElement | null} */ (root.querySelector("[data-ui-select-clear]"))?.focus();
    return;
  }
  if (target.closest("[data-ui-select-clear]")) {
    save(root, { mode: "ids", ids: [], ex: [], key: "" });
    drawSelection(root);
    return;
  }
  const sort = /** @type {HTMLAnchorElement | null} */ (target.closest("a[data-ui-add-sort]"));
  if (sort && e.shiftKey) {
    e.preventDefault();
    visit(root, sort.getAttribute("data-ui-add-sort") || sort.href);
    return;
  }
  // A plain click on a link of the table that only changes the order: the rows move
  // here at once, and the runtime still follows the link to confirm.
  const link = /** @type {HTMLAnchorElement | null} */ (target.closest("a[data-ui-in-region][href]"));
  if (link && !e.defaultPrevented && !e.button && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
    sortHere(/** @type {HTMLElement} */ (root), new URL(link.href));
    return;
  }
  const row = target.closest("tr[data-ui-row-href]");
  if (row && !target.closest("a,button,input,select,label,[popover]") && !getSelection()?.toString()) visit(document.body, row.getAttribute("data-ui-row-href") || "");
}, true);

// What a bulk form posts: the browser's own fields hold only this page's ticks.
document.addEventListener("formdata", (e) => {
  const form = /** @type {HTMLFormElement} */ (e.target);
  if (!form.hasAttribute?.("data-ui-bulk")) return;
  const root = form.closest("[data-ui-table]");
  if (!root) return;
  const selection = load(root);
  const data = /** @type {FormDataEvent} */ (e).formData;
  data.delete("ids");
  if (selection.mode === "all") {
    data.set("_mode", "all");
    data.set("_expected", String(selectedCount(root, selection)));
    const version = root.getAttribute("data-version");
    if (version) data.set("_version", version);
    for (const key of selection.ex) data.append("ex", key);
  } else {
    for (const key of selection.ids) data.append("ids", key);
  }
});

document.addEventListener("submit", (e) => {
  const form = /** @type {HTMLFormElement} */ (e.target);
  if (!form.hasAttribute?.("data-ui-bulk")) return;
  const root = form.closest("[data-ui-table]");
  if (!root) return;
  const selection = load(root);
  const n = selectedCount(root, selection);
  if (!n) {
    e.preventDefault();
    return;
  }
  const button = /** @type {HTMLElement | null} */ (/** @type {SubmitEvent} */ (e).submitter);
  const question = button?.getAttribute("data-ui-confirm");
  if (!question || confirmed === button) {
    confirmed = null;
    return;
  }
  const dialog = /** @type {HTMLDialogElement | null} */ (document.getElementById(root.id + "-confirm"));
  if (!dialog) return;
  e.preventDefault();
  const text = dialog.querySelector("[data-slot=dialog-description]");
  if (text) text.textContent = fill(root, question, n);
  /** @type {any} */ (dialog).uiSubmit = () => {
    confirmed = button;
    dialog.close();
    form.requestSubmit(button);
  };
  dialog.showModal();
}, true);

document.addEventListener("click", (e) => {
  const yes = /** @type {Element} */ (e.target).closest?.("[data-ui-confirm-yes]");
  if (yes) /** @type {any} */ (yes.closest("dialog"))?.uiSubmit?.();
});

/** Puts an edited cell back to what the server last said. @param {HTMLFormElement} form */
function rollBack(form) {
  const control = /** @type {any} */ (form.querySelector("input:not([type=hidden]),select"));
  if (!control) return;
  if (control.nodeName === "SELECT") for (const option of control.options) option.selected = option.defaultSelected;
  else control.value = control.defaultValue;
}

/** Makes what the cell shows the value it goes back to. @param {HTMLFormElement} form */
function settle(form) {
  const control = /** @type {any} */ (form.querySelector("input:not([type=hidden]),select"));
  if (!control) return;
  if (control.nodeName === "SELECT") for (const option of control.options) option.defaultSelected = option.selected;
  else control.defaultValue = control.value;
}

document.addEventListener("change", (e) => {
  const control = /** @type {HTMLInputElement} */ (e.target);
  const form = control.closest?.("form[data-ui-edit]");
  if (form) /** @type {HTMLFormElement} */ (form).requestSubmit();
});

document.addEventListener("keydown", (e) => {
  const control = /** @type {HTMLInputElement} */ (e.target);
  const form = /** @type {HTMLFormElement | null} */ (control.closest?.("form[data-ui-edit]"));
  if (!form) return;
  if (e.key === "Escape") {
    rollBack(form);
    control.blur();
  } else if (e.key === "Enter") {
    // Leaving the box posts it, once.
    e.preventDefault();
    control.blur();
  }
});

document.addEventListener("ui:sent", (e) => {
  const { form, ok } = /** @type {CustomEvent} */ (e).detail;
  if (form.hasAttribute?.("data-ui-edit")) {
    if (ok) settle(form);
    else rollBack(form);
    return;
  }
  const root = form.closest?.("[data-ui-table]");
  if (root && ok && form.hasAttribute("data-ui-bulk")) {
    save(root, { mode: "ids", ids: [], ex: [], key: "" });
    drawSelection(root);
  }
});

document.addEventListener("ui:swap", syncAll);

/* Resizing: with a pointer, or the arrow keys on the handle */

document.addEventListener("pointerdown", (e) => {
  const handle = /** @type {Element} */ (e.target).closest?.("[data-slot=col-resize]");
  const root = handle?.closest("[data-ui-table]");
  if (!handle || !root) return;
  e.preventDefault();
  freezeWidths(root);
  const th = /** @type {HTMLElement} */ (handle.closest("th"));
  const rtl = getComputedStyle(th).direction === "rtl";
  resizing = { root, handle, start: e.clientX, width: th.getBoundingClientRect().width, side: rtl ? -1 : 1 };
  /** @type {HTMLElement} */ (handle).setPointerCapture?.(e.pointerId);
});

document.addEventListener("pointermove", (e) => {
  if (resizing) setWidth(resizing.root, resizing.handle, resizing.width + (e.clientX - resizing.start) * resizing.side);
});

document.addEventListener("pointerup", () => {
  if (!resizing) return;
  const root = resizing.root;
  resizing = null;
  commitWidths(root);
});

document.addEventListener("keydown", (e) => {
  const target = /** @type {HTMLElement} */ (e.target);
  const cell = target.matches?.("td[tabindex]") && target.closest("[data-ui-grid]") ? target : null;
  if (cell) {
    gridKey(e, cell);
    return;
  }
  if (e.key === "Escape") {
    const inGrid = target.closest?.("[data-ui-grid] td[tabindex]");
    if (inGrid) /** @type {HTMLElement} */ (inGrid).focus();
  }
  if (!target.matches?.("[data-slot=col-resize]") || (e.key !== "ArrowLeft" && e.key !== "ArrowRight")) return;
  const root = target.closest("[data-ui-table]");
  if (!root) return;
  e.preventDefault();
  freezeWidths(root);
  const rtl = getComputedStyle(target).direction === "rtl";
  const grow = (e.key === "ArrowRight") !== rtl;
  // From the width the column has now, which a late face or a cell that filled in may have changed since it was drawn.
  setWidth(root, target, /** @type {Element} */ (target.closest("th")).getBoundingClientRect().width + (grow ? 16 : -16));
  clearTimeout(keyTimers.get(target));
  keyTimers.set(target, setTimeout(() => commitWidths(root), 400));
});

/* Reordering by dragging a heading */

document.addEventListener("dragstart", (e) => {
  const th = /** @type {Element} */ (e.target).closest?.("th[data-col]");
  if (!th || !/** @type {Element} */ (e.target).matches?.("[data-ui-col-drag]")) return;
  dragged = th.getAttribute("data-col");
  if (/** @type {DragEvent} */ (e).dataTransfer) /** @type {DataTransfer} */ (/** @type {DragEvent} */ (e).dataTransfer).effectAllowed = "move";
});

document.addEventListener("dragover", (e) => {
  if (dragged && /** @type {Element} */ (e.target).closest?.("th[data-col]")) e.preventDefault();
});

document.addEventListener("drop", (e) => {
  const th = /** @type {Element} */ (e.target).closest?.("th[data-col]");
  const root = th?.closest("[data-ui-table]");
  if (!dragged || !th || !root) return;
  e.preventDefault();
  const names = [...root.querySelectorAll("thead th[data-col]")].map((cell) => cell.getAttribute("data-col") || "");
  const from = names.indexOf(dragged), to = names.indexOf(th.getAttribute("data-col") || "");
  dragged = null;
  if (from < 0 || to < 0 || from === to) return;
  names.splice(to, 0, ...names.splice(from, 1));
  visit(root, withParam(root, "cols", names.join(",")));
});

document.addEventListener("dragend", () => {
  dragged = null;
});

/* A long list of filter choices: narrowing it needs no request */

document.addEventListener("input", (e) => {
  const find = /** @type {HTMLInputElement} */ (e.target);
  if (!find.matches?.("[data-ui-find-option]")) return;
  const needle = find.value.toLowerCase();
  for (const option of find.parentElement?.querySelectorAll("[data-slot=filter-option]") || []) {
    /** @type {HTMLElement} */ (option).hidden = !(option.textContent || "").toLowerCase().includes(needle);
  }
});

// In capture, ahead of the runtime's own listener: a prefetched page can morph in
// before the next listener runs.
addEventListener("popstate", () => {
  cameBack = true;
}, true);

document.addEventListener("scroll", (e) => {
  const box = /** @type {Element} */ (e.target);
  if (box.matches?.("[data-slot=table-container]")) {
    const root = box.closest("[data-ui-table]");
    if (root) markScroll(root);
  }
}, { capture: true, passive: true });

addEventListener("resize", () => {
  for (const root of document.querySelectorAll("[data-ui-table]")) measureSticky(root);
});

syncAll();
