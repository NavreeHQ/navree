// The command interpreter: closed verbs from `data-ui-click`, `data-ui-change`
// and `data-ui-input`. Commands need no network and no expression language.
import { all, attr, byId, has, on } from "./dom.js";
import { openTarget, closeTarget, announce } from "./ui.js";

// What a command may touch: state, never where a thing points or what it runs. The
// `data-ui-*` hooks are the runtime's. `Cmd` in Rust holds the same list.
const STATE = /^(data-(?!ui-)|aria-|(hidden|open|disabled)$)/i;

// Values and types are read and written through the prototypes, as everything else
// is: a field named `value` or `type` in a form can't stand in for them.
/** @param {object} proto @param {string} name */
const prop = (proto, name) => /** @type {PropertyDescriptor} */ (Object.getOwnPropertyDescriptor(proto, name));
const I = HTMLInputElement.prototype, A = HTMLTextAreaElement.prototype;
const inputValue = prop(I, "value"), inputType = prop(I, "type"), areaValue = prop(A, "value");

/** The text of a control, or nothing for anything else. @param {Element} el */
const valueOf = (el) => (el instanceof HTMLInputElement ? inputValue.get?.call(el) : el instanceof HTMLTextAreaElement ? areaValue.get?.call(el) : "") || "";

/** Whether `el` is a hidden field, the one kind of field a command may set. @param {Element | null} el */
const hiddenField = (el) => el instanceof HTMLInputElement && inputType.get?.call(el) === "hidden";

/**
 * What a command never writes text into: code (a script's text runs, a style's
 * text styles) and the fields a user types in or picks from.
 * @param {Element} el
 */
const closed = (el) => /^(script|style)$/i.test(el.localName) || el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement;

/**
 * Writes `text` into `el` as text, or as the value of a hidden field.
 * @param {Element} el @param {string} text
 */
function write(el, text) {
  if (hiddenField(el)) inputValue.set?.call(el, text);
  else if (!closed(el)) el.textContent = text;
}

/**
 * The text of the row's cell in `column`. In a table, the column is found by its
 * heading's `data-col`; elsewhere, by an element of the row with that `data-col`.
 * @param {Element} row @param {string} column
 */
function cellText(row, column) {
  const name = CSS.escape(column);
  if (row instanceof HTMLTableRowElement) {
    const table = row.closest("table");
    const head = table && all(table, `thead th[data-col="${name}"]`)[0];
    return head instanceof HTMLTableCellElement ? row.cells[head.cellIndex]?.textContent : null;
  }
  return all(row, `[data-col="${name}"]`)[0]?.textContent;
}

/**
 * Fills `target` from the row `from` sits in, as text, and opens it: details of what
 * the page already shows, with no request.
 * @param {Element | null} target @param {Element} from
 */
function fill(target, from) {
  const row = Element.prototype.closest.call(from, "[data-key]");
  if (!target || !row) return;
  for (const slot of all(target, "[data-ui-cell],[data-ui-row-key]")) {
    const text = has(slot, "data-ui-row-key") ? attr(row, "data-key") : cellText(row, attr(slot, "data-ui-cell") || "");
    write(slot, (text || "").trim());
  }
  openTarget(target);
}

/** @param {any[]} cmd @param {Element} from */
function run(cmd, from) {
  const [verb, a, b, c] = cmd;
  const target = typeof a === "string" ? byId(a) : null;
  switch (verb) {
    case "toggle": target?.toggleAttribute("hidden"); break;
    case "show": target?.removeAttribute("hidden"); break;
    case "hide": target?.setAttribute("hidden", ""); break;
    case "open": if (target) openTarget(target); break;
    case "close": if (target) closeTarget(target); break;
    case "copy":
      navigator.clipboard?.writeText(a).then(() => announce("Copied"));
      from.setAttribute("data-copied", "");
      setTimeout(() => from.removeAttribute("data-copied"), 1500);
      break;
    case "set_attr": if (STATE.test(b)) target?.setAttribute(b, c); break;
    case "remove_attr": if (STATE.test(b)) target?.removeAttribute(b); break;
    case "toggle_attr": if (STATE.test(b)) target?.toggleAttribute(b); break;
    case "dispatch": from.dispatchEvent(new CustomEvent(a, { bubbles: true })); break;
    case "focus": target?.focus(); break;
    case "submit": /** @type {HTMLFormElement | null} */ (target)?.requestSubmit?.(); break;
    // Text only, never markup: see `write`.
    case "text": if (target && typeof b === "string") write(target, b); break;
    case "count": {
      const n = [...valueOf(from)].length;
      if (target && !hiddenField(target)) write(target, typeof b === "number" ? n + "/" + b : String(n));
      break;
    }
    case "value": if (hiddenField(target) && typeof b === "string") inputValue.set?.call(target, b); break;
    case "fill": fill(target, from); break;
  }
}

/** @param {Event} e @param {string} attribute */
function handle(e, attribute) {
  const el = /** @type {Element} */ (e.target).closest?.(`[${attribute}]`);
  if (!el || /** @type {any} */ (el).disabled) return;
  try {
    for (const cmd of JSON.parse(el.getAttribute(attribute) || "[]")) run(cmd, el);
  } catch {}
}

/** Wires up commands. */
export function startCommands() {
  on(document, "click", (e) => handle(e, "data-ui-click"));
  on(document, "change", (e) => handle(e, "data-ui-change"));
  on(document, "input", (e) => handle(e, "data-ui-input"));
}
