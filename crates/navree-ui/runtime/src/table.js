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

/** @param {number} n */
const grouped = (n) => n.toLocaleString("en-US");

/** @param {string} template @param {number} n @param {number} [total] */
const fill = (template, n, total) => (template || "").replace("{n}", grouped(n)).replace("{total}", grouped(total || 0));

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
    all.textContent = fill(words.all, everything);
  }
  let text;
  if (selection.mode === "all") text = selection.ex.length ? fill(words.some, n, everything) : fill(words.allOn, n);
  else if (offer) text = fill(words.page, onPage);
  else {
    text = fill(words.selected, n);
    if (n > onPage) text += " · " + fill(words.off, n - onPage);
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
  if (!root.querySelector('[data-sticky="1"],tr[data-sticky-row]')) return;
  let at = root.querySelector(`thead ${STICKY_SELECT}`)?.getBoundingClientRect().width || 0;
  el.style.setProperty("--s0", at + "px");
  const head = root.querySelector("thead");
  if (head) el.style.setProperty("--head-h", head.getBoundingClientRect().height + "px");
  for (const th of root.querySelectorAll("thead tr:not([data-slot=group-row]) th[data-sticky]")) {
    const name = th.getAttribute("data-sticky");
    if (name === "s" || name === "e" || name === "e1") continue;
    el.style.setProperty("--s" + name, at + "px");
    at += th.getBoundingClientRect().width;
  }
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
  drawSelection(root);
  if (root.hasAttribute("data-ui-grid")) setupGrid(root);
  listen(root);
  const place = location.search.replace(/.*\b(page|after|before)=([^&]*).*/, "$2");
  const scroll = root.getAttribute("data-scroll-to");
  if (scroll && /** @type {any} */ (root).uiPlace !== undefined && /** @type {any} */ (root).uiPlace !== place) document.getElementById(scroll)?.scrollIntoView();
  /** @type {any} */ (root).uiPlace = place;
}

function syncAll() {
  for (const root of document.querySelectorAll("[data-ui-table]")) sync(/** @type {HTMLElement} */ (root));
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

const collator = new Intl.Collator(undefined, { sensitivity: "base" });
const digits = /^\d{1,15}$/;

/**
 * A cell's value for sorting: what the server wrote in `data-v`, or the cell's text, as
 * a number in a column of numbers. Empty is NULL, the smallest value, as on every store.
 * @param {HTMLTableRowElement} row @param {number} at @param {string | null} kind
 */
function valueOf(row, at, kind) {
  const cell = row.cells[at];
  const raw = (cell?.getAttribute("data-v") ?? cell?.textContent ?? "").trim();
  const number = kind === "n" ? Number(raw.replace(/[^\d.eE+-]/g, "")) : 0;
  return raw === "" || Number.isNaN(number) ? null : kind === "n" ? number : raw;
}

/**
 * Orders two values: NULL first, numbers as numbers, dates (written ISO) and keys of
 * digits by their text or number, the rest as words.
 * @param {any} a @param {any} b @param {string | null} [kind]
 */
function compare(a, b, kind) {
  if (a === null || b === null) return a === b ? 0 : a === null ? -1 : 1;
  if (typeof a === "number") return a - b;
  if (kind === "k" && digits.test(a) && digits.test(b)) return a - b;
  if (kind === "d" || kind === "k") return a < b ? -1 : a > b ? 1 : 0;
  return collator.compare(a, b);
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
  /** Column, kind and direction of each term. @type {[number, string | null, boolean][]} */
  const terms = [];
  /** @type {Map<string, boolean>} */
  const sorted = new Map();
  for (const term of sort.split(",").filter(Boolean)) {
    const down = term[0] === "-", name = down ? term.slice(1) : term;
    const th = /** @type {HTMLTableCellElement | undefined} */ (heads.find((cell) => cell.getAttribute("data-col") === name));
    if (!th) return false;
    terms.push([th.cellIndex, th.getAttribute("data-kind"), down]);
    sorted.set(name, down);
  }
  /** Each row with what it sorts by, and its details row, which moves with it. @type {any[][]} */
  const groups = [];
  for (const row of body.rows) {
    const key = row.getAttribute("data-key");
    if (!key) return false;
    if (row.getAttribute("data-slot") === "row-detail" && groups.length) groups[groups.length - 1].push(row);
    else groups.push([key, terms.map(([at, kind]) => valueOf(row, at, kind)), row]);
  }
  // Ties go by key, in the last term's direction, as the server orders them.
  const last = terms.length ? terms[terms.length - 1][2] : false;
  groups.sort((a, b) => {
    for (let i = 0; i < terms.length; i++) {
      const order = compare(a[1][i], b[1][i], terms[i][1]);
      if (order) return terms[i][2] ? -order : order;
    }
    return (last ? -1 : 1) * compare(a[0], b[0], "k");
  });
  /** @type {any} */ (root).uiBefore = [[...body.rows], heads.map((th) => [th, th.getAttribute("aria-sort")])];
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
  for (const th of heads) {
    const down = sorted.get(th.getAttribute("data-col") || "");
    if (down === undefined) th.removeAttribute("aria-sort");
    else th.setAttribute("aria-sort", down ? "descending" : "ascending");
  }
  const field = /** @type {HTMLInputElement | null} */ (root.querySelector("form[data-ui-target] input[type=hidden][name=sort]"));
  if (field) field.value = sort;
  // The rows are right already: the server's answer only confirms them.
  root.setAttribute("data-ui-confirm", "");
  return true;
}

// The server couldn't confirm: what was done here is undone, and the table shows what
// its URL says.
document.addEventListener("ui:failed", (e) => {
  const root = /** @type {any} */ (/** @type {Element} */ (e.target).closest?.("[data-ui-table]"));
  const [rows, sorts] = root?.uiBefore || [];
  root?.removeAttribute("data-ui-confirm");
  if (!rows) return;
  root.uiBefore = null;
  rows[0]?.parentNode?.append(...rows);
  for (const [th, sort] of sorts) {
    if (sort === null) th.removeAttribute("aria-sort");
    else th.setAttribute("aria-sort", sort);
  }
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
  if (text) text.textContent = fill(question, n);
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

/** The event streams live tables listen to. @type {Map<Element, EventSource>} */
const streams = new Map();

/** Refreshes a table when its stream speaks, at most every 300 ms. @param {HTMLElement} root */
function listen(root) {
  const url = root.getAttribute("data-ui-live-src");
  for (const [other, stream] of streams) {
    if (!other.isConnected) {
      stream.close();
      streams.delete(other);
    }
  }
  if (!url || streams.has(root)) return;
  const stream = new EventSource(url);
  let timer = 0;
  stream.onmessage = () => {
    clearTimeout(timer);
    timer = /** @type {any} */ (setTimeout(() => /** @type {HTMLElement | null} */ (root.querySelector("[data-ui-refresh]"))?.click(), 300));
  };
  streams.set(root, stream);
}

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
  setWidth(root, target, Number(target.getAttribute("aria-valuenow") || 160) + (grow ? 16 : -16));
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
