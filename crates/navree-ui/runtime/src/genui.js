// The conversation module: two things a chat with an assistant needs that a form and a
// live list don't already do. In the composer, Enter sends and Shift+Enter starts a new
// line (never during an input method's composition, never an empty message). A
// conversation the reader was reading at its end stays at its end while an answer grows
// and new messages arrive; one they scrolled up in stays where they left it. Both are
// delegated from `document`, so a morph or a swap needs no wiring, and with no script the
// composer is a plain form and the list a plain list.
import { closest, every, on, scrolling } from "./dom.js";

const COMPOSER = "form[data-genui=composer]";
const CONVERSATION = "[data-genui=conversation]";
/** How near the end, in pixels, still counts as at the end. */
const NEAR = 64;

on(document, "keydown", (/** @type {KeyboardEvent} */ e) => {
  if (e.key !== "Enter" || e.shiftKey || e.ctrlKey || e.altKey || e.metaKey || e.isComposing || e.keyCode === 229 || e.defaultPrevented) return;
  const area = e.target;
  if (!(area instanceof HTMLTextAreaElement)) return;
  const form = closest(area, COMPOSER);
  if (!(form instanceof HTMLFormElement)) return;
  e.preventDefault();
  if (area.value.trim() !== "" && form.checkValidity()) HTMLFormElement.prototype.requestSubmit.call(form);
});

/** The element that scrolls `el`: the nearest that can, or the page. @param {Element} el @returns {Element} */
function scroller(el) {
  for (let at = el.parentElement; at; at = at.parentElement) {
    const overflow = getComputedStyle(at).overflowY;
    if ((overflow === "auto" || overflow === "scroll") && at.scrollHeight > at.clientHeight) return at;
  }
  return scrolling();
}

/** Whether the reader is at the end of what `box` scrolls. @param {Element} box */
const atEnd = (box) => box.scrollHeight - box.scrollTop - box.clientHeight <= NEAR;

/** @type {WeakMap<Element, boolean>} whether each conversation's reader is at its end */
const pinned = new WeakMap();
/** @type {WeakSet<Element>} */
const watched = new WeakSet();

/** Watches the conversations in the page. */
function scan() {
  for (const list of every(CONVERSATION)) {
    if (watched.has(list)) continue;
    watched.add(list);
    const box = scroller(list);
    pinned.set(list, atEnd(box));
    const target = box === scrolling() ? window : box;
    on(target, "scroll", () => pinned.set(list, atEnd(box)), { passive: true });
    new MutationObserver(() => {
      if (list.isConnected && pinned.get(list)) box.scrollTop = box.scrollHeight;
    }).observe(list, { childList: true, subtree: true, characterData: true });
  }
}

on(document, "ui:swap", scan);
scan();
