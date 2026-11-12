// Count-up: a number the server wrote counts up from zero to it when it comes into view.
// The page holds the number as the server formatted it, so nothing changes for a reader
// with no script or who asks for less motion. While counting, what comes before and
// after the digits (a currency sign, a percent sign) stays, the digits are grouped as
// the element's locale does it, and the count ends on the server's own text. A number a
// region morphs in again keeps its value and does not count twice.
import { attr, every, on } from "./dom.js";

const SELECTOR = "[data-slot=count-up][data-to]";
const calm = matchMedia("(prefers-reduced-motion: reduce)");
/** Numbers counted already, or waiting to come into view. @type {WeakSet<Element>} */
const seen = new WeakSet();

/** @param {HTMLElement} el */
function count(el) {
  const to = Number(attr(el, "data-to"));
  const text = el.textContent ?? "";
  const parts = /^(\D*)(.*\d)(\D*)$/s.exec(text);
  if (!parts || !Number.isFinite(to)) return;
  const places = (attr(el, "data-to")?.split(".")[1] ?? "").length;
  let format;
  try {
    format = new Intl.NumberFormat(attr(el, "data-locale") || el.closest("[lang]")?.getAttribute("lang") || "en", {
      minimumFractionDigits: places,
      maximumFractionDigits: places,
      numberingSystem: "latn",
    });
  } catch {
    return;
  }
  const start = performance.now();
  const step = (/** @type {number} */ now) => {
    const t = Math.min(1, (now - start) / 1200);
    if (t >= 1 || !el.isConnected || el.textContent !== shown) {
      if (el.isConnected && el.textContent === shown) el.textContent = text;
      return;
    }
    // Fast at first, settling into the value.
    el.textContent = shown = parts[1] + format.format(to * (1 - (1 - t) ** 3)) + parts[3];
    requestAnimationFrame(step);
  };
  let shown = (el.textContent = parts[1] + format.format(0) + parts[3]);
  requestAnimationFrame(step);
}

const view = new IntersectionObserver((entries) => {
  for (const entry of entries) {
    if (!entry.isIntersecting) continue;
    view.unobserve(entry.target);
    count(/** @type {HTMLElement} */ (entry.target));
  }
});

function scan() {
  for (const el of every(SELECTOR)) {
    if (seen.has(el)) continue;
    seen.add(el);
    if (!calm.matches) view.observe(el);
  }
}
scan();
on(document, "ui:swap", scan);
