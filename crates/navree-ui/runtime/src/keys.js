// Hotkeys: single keys that click, or focus, the element that names them with
// `data-ui-hotkey`, the way a support or mail app reads its keyboard. A key never fires
// while someone types: in a field, in editable text, in a widget whose letters are its
// own (a menu's typeahead, a list box), during an input method's composition, with Ctrl,
// Alt or Meta held, or from outside a modal dialog that is open. The first element in
// the page that can take the key does.
import { active, attr, closest, every, has, on } from "./dom.js";

/** Widgets that read letters themselves. */
const OWN_KEYS = "input,textarea,select,[contenteditable]:not([contenteditable=false]),[role=textbox],[role=searchbox],[role=combobox],[role=spinbutton],[role=menu],[role=menubar],[role=menuitem],[role=listbox],[role=option],[role=tree],[role=treegrid],[role=grid]";
const FIELD = /^(input|textarea|select)$/i;

/** Whether keys typed now are someone's typing. @param {Element | null} el */
const typing = (el) => !!el && (/** @type {HTMLElement} */ (el).isContentEditable || !!closest(el, OWN_KEYS));

/** The modal dialog that is open, if one is. */
function modal() {
  for (const dialog of every("dialog[open]")) {
    try {
      if (dialog.matches(":modal")) return dialog;
    } catch {}
  }
  return null;
}

/** Whether `el` can take its key now. @param {Element} el @param {Element | null} holder */
function usable(el, holder) {
  if (/** @type {HTMLButtonElement} */ (el).disabled || attr(el, "aria-disabled") === "true") return false;
  if (closest(el, "[hidden],[inert],template")) return false;
  const dialog = closest(el, "dialog");
  if (dialog && !has(dialog, "open")) return false;
  const popover = closest(el, "[popover]");
  if (popover && !popover.matches(":popover-open")) return false;
  return !holder || holder.contains(el);
}

on(document, "keydown", (/** @type {KeyboardEvent} */ e) => {
  if (e.defaultPrevented || e.repeat || e.isComposing || e.keyCode === 229 || e.ctrlKey || e.metaKey || e.altKey) return;
  const key = e.key;
  if (!key || [...key].length !== 1 || key.trim() === "") return;
  if (typing(active()) || typing(/** @type {Element} */ (e.target))) return;
  const holder = modal();
  for (const el of every("[data-ui-hotkey]")) {
    if (attr(el, "data-ui-hotkey") !== key || !usable(el, holder)) continue;
    e.preventDefault();
    if (FIELD.test(el.localName) || /** @type {HTMLElement} */ (el).isContentEditable) /** @type {HTMLElement} */ (el).focus();
    else /** @type {HTMLElement} */ (el).click();
    return;
  }
});
