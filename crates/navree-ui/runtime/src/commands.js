// The command interpreter: closed verbs from `data-ui-click`, `data-ui-change`
// and `data-ui-input`. Commands need no network and no expression language.
import { byId, on } from "./dom.js";
import { openTarget, closeTarget, announce } from "./ui.js";

const REFUSED = /^(on|style$|srcdoc$|href$|src$|action$|formaction$)/i;

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
    case "set_attr": if (!REFUSED.test(b)) target?.setAttribute(b, c); break;
    case "remove_attr": if (!REFUSED.test(b)) target?.removeAttribute(b); break;
    case "toggle_attr": if (!REFUSED.test(b)) target?.toggleAttribute(b); break;
    case "dispatch": from.dispatchEvent(new CustomEvent(a, { bubbles: true })); break;
    case "focus": target?.focus(); break;
    case "submit": /** @type {HTMLFormElement | null} */ (target)?.requestSubmit?.(); break;
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
