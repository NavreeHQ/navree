// The op applier: the closed list the server sends. Nothing here runs code
// from the wire; the HTML in an op is rendered by the server.
import { morph, parse } from "./morph.js";
import { all, byId, field, make, root } from "./dom.js";
import { announce, toast, openTarget, closeTarget } from "./ui.js";
import { forget, go, setUrl, refresh } from "./nav.js";

/** @param {HTMLFormElement} form @param {Record<string, string[]>} fields */
function showErrors(form, fields) {
  for (const old of all(form, "[data-slot=field-error]")) old.remove();
  for (const old of all(form, "[data-invalid]")) old.removeAttribute("data-invalid");
  for (const old of all(form, "[aria-invalid]")) old.removeAttribute("aria-invalid");
  let first = null;
  let count = 0;
  for (const name in fields) {
    const input = /** @type {HTMLElement | undefined} */ (field(form, name));
    const box = input?.closest?.("[data-slot=field]");
    const text = fields[name].join(" ");
    count++;
    if (!input || !box) {
      toast("danger", text);
      continue;
    }
    box.setAttribute("data-invalid", "");
    input.setAttribute("aria-invalid", "true");
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
      if (target) target.outerHTML = op.html;
      break;
    case "append": target?.insertAdjacentHTML("beforeend", op.html); break;
    case "prepend": target?.insertAdjacentHTML("afterbegin", op.html); break;
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
  for (const url of envelope.modules || []) import(url);
  for (const op of envelope.ops || []) apply(op);
}
