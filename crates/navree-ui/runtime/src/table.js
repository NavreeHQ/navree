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

/** Places sticky columns: each starts where those before it end. @param {Element} root */
function measureSticky(root) {
  const el = /** @type {HTMLElement} */ (root);
  let at = root.querySelector(`thead ${STICKY_SELECT}`)?.getBoundingClientRect().width || 0;
  el.style.setProperty("--s0", at + "px");
  for (const th of root.querySelectorAll("thead tr:not([data-slot=group-row]) th[data-sticky]")) {
    const name = th.getAttribute("data-sticky");
    if (name === "s" || name === "e") continue;
    el.style.setProperty("--s" + name, at + "px");
    at += th.getBoundingClientRect().width;
  }
}

/** Puts controls back to what the server drew: a tick the user made stays ticked by
 * the browser's rule even when the server's answer took it away. @param {Element} root */
function resetControls(root) {
  const form = /** @type {HTMLFormElement | null} */ (root.querySelector("[data-slot=table-toolbar]"));
  if (!form) return;
  for (const control of form.elements) {
    if (control === document.activeElement) continue;
    const input = /** @type {HTMLInputElement} */ (control);
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
  root.setAttribute("data-ui-keep-attrs", "style data-ui-ready data-has-selection");
  root.setAttribute("data-ui-ready", "");
  for (const box of root.querySelectorAll("[data-ui-col-drag]")) box.setAttribute("draggable", "true");
  resetControls(root);
  measureSticky(root);
  drawSelection(root);
  if (root.hasAttribute("data-ui-grid")) setupGrid(root);
  const place = location.search.replace(/.*\b(page|after|before)=([^&]*).*/, "$2");
  const scroll = root.getAttribute("data-scroll-to");
  if (scroll && /** @type {any} */ (root).uiPlace !== undefined && /** @type {any} */ (root).uiPlace !== place) document.getElementById(scroll)?.scrollIntoView();
  /** @type {any} */ (root).uiPlace = place;
}

function syncAll() {
  for (const root of document.querySelectorAll("[data-ui-table]")) sync(/** @type {HTMLElement} */ (root));
}

/* The grid keys */

/** @param {HTMLElement} root */
function setupGrid(root) {
  const table = root.querySelector("[data-slot=table]");
  table?.setAttribute("role", "grid");
  const cells = /** @type {HTMLElement[]} */ ([...root.querySelectorAll("tbody td")]);
  let current = cells.find((cell) => cell.getAttribute("tabindex") === "0");
  for (const cell of cells) if (cell !== current) cell.setAttribute("tabindex", "-1");
  if (!current) cells[0]?.setAttribute("tabindex", "0");
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

document.addEventListener("ui:sent", (e) => {
  const { form, ok } = /** @type {CustomEvent} */ (e).detail;
  const root = form.closest?.("[data-ui-table]");
  if (root && ok && form.hasAttribute("data-ui-bulk")) save(root, { mode: "ids", ids: [], ex: [], key: "" });
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

addEventListener("resize", () => {
  for (const root of document.querySelectorAll("[data-ui-table]")) measureSticky(root);
});

syncAll();
