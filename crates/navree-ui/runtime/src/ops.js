// The op applier: the closed list the server sends. Nothing here runs code from
// the wire but the toolkit's own modules; the HTML in an op is rendered by the server.
import { morph, parse, pieces } from "./morph.js";
import { all, byId, field, make, ownModule, root } from "./dom.js";
import { announce, toast, openTarget, closeTarget } from "./ui.js";
import { forget, go, leave, setUrl, refresh } from "./nav.js";

/** @param {HTMLFormElement} form @param {Record<string, string[]>} fields */
function showErrors(form, fields) {
  for (const old of all(form, "[data-slot=field-error]")) old.remove();
  for (const old of all(form, "[data-invalid]")) old.removeAttribute("data-invalid");
  for (const old of all(form, "[aria-invalid]")) old.removeAttribute("aria-invalid");
  let first = null;
  let count = 0;
  for (const name in fields) {
    const named = /** @type {HTMLElement | undefined} */ (field(form, name));
    // The value of a select, combobox or multi-select is in a hidden select: the error
    // belongs on the control a person uses.
    const input = named?.matches?.("[data-slot=select-native]") ? /** @type {HTMLElement | undefined} */ (named.closest("[data-ui-picker]")?.querySelector("[data-field-control]") || named) : named;
    const box = input?.closest?.("[data-slot=field]");
    const radio = input?.getAttribute?.("type") === "radio";
    const text = fields[name].join(" ");
    count++;
    if (!input || !box) {
      toast("danger", text);
      continue;
    }
    box.setAttribute("data-invalid", "");
    if (!radio) input.setAttribute("aria-invalid", "true");
    const error = make("p");
    error.setAttribute("data-slot", "field-error");
    error.id = (input.id || name) + "-error";
    error.textContent = text;
    box.append(error);
    const described = (input.getAttribute("aria-describedby") || "").split(" ").filter(Boolean);
    if (!described.includes(error.id)) described.push(error.id);
    input.setAttribute("aria-describedby", described.join(" "));
    first ||= input;
  }
  if (first) first.focus();
  if (count) announce(count === 1 ? "1 field needs attention" : count + " fields need attention");
}

/** @param {any} op */
function apply(op) {
  const target = op.target ? byId(op.target) : null;
  switch (op.op) {
    case "morph": {
      const next = parse(op.html);
      if (target && next) morph(target, next);
      break;
    }
    case "replace":
      target?.replaceWith(pieces(op.html));
      break;
    case "append": target?.append(pieces(op.html)); break;
    case "prepend": target?.prepend(pieces(op.html)); break;
    case "remove": target?.remove(); break;
    case "errors": {
      const form = byId(op.form);
      if (form) showErrors(/** @type {HTMLFormElement} */ (form), op.fields);
      break;
    }
    case "toast": toast(op.tone, op.text); break;
    case "open": if (target) openTarget(target); break;
    case "close": if (target) closeTarget(target); break;
    case "focus": target?.focus(); break;
    case "url": setUrl(op.href, op.mode); break;
    case "navigate": go(op.href); break;
    case "navigate_external": leave(op.href); break;
    case "reload": location.reload(); break;
    case "invalidate": refresh(op.target); break;
  }
}

/** Applies an envelope; a build this page doesn't know means reload. @param {any} envelope */
export function applyEnvelope(envelope) {
  const build = root().getAttribute("data-build");
  if (envelope.build && build && envelope.build !== build) {
    location.reload();
    return;
  }
  forget();
  for (const url of Array.isArray(envelope.modules) ? envelope.modules : []) {
    if (ownModule(url)) import(url).catch(() => {});
  }
  // One op that can't apply doesn't take the others with it.
  for (const op of Array.isArray(envelope.ops) ? envelope.ops : []) {
    try {
      apply(op);
    } catch {}
  }
}
