// navree.com's preview frame. A preview shows a component, not the app behind
// it, so its links and forms go nowhere. This runs before the toolkit's modules
// and in the capture phase, so they see the event as already handled.
addEventListener(
  "click",
  (event) => {
    const target = event.target instanceof Element ? event.target : null;
    // A tab is a link the toolkit switches in place, so its click is left alone.
    if (target?.closest("a[href]:not([role=tab])")) event.preventDefault();
  },
  true,
);
addEventListener("submit", (event) => event.preventDefault(), true);

// ---- Pictures drawn in place ----

// A preview that shows a menu, list, card or dialog as it looks open draws the real
// element in place. Opened for real, it moves to the top layer and leaves the page:
// a stand-in of its size keeps its room meanwhile, so the control that opened it
// stays under the pointer, and a hover card doesn't close as its trigger moves away.
const PICTURES =
  "[data-slot=dialog],[data-slot=alert-dialog],[data-slot=sheet],[data-slot=drawer],[data-slot=select-popup],[data-slot=command-dialog],[data-slot=popover-content],[data-slot=hover-card-content],[data-slot=tooltip-content],[data-slot=dropdown-menu]";
if (document.body.hasAttribute("data-preview-show")) {
  /** @type {Map<Element, HTMLElement>} */
  const kept = new Map();
  /** The room `el` takes where it is drawn, margins too; nothing when it isn't drawn in place. */
  const roomOf = (el) => {
    const style = getComputedStyle(el);
    if (style.position !== "relative" || style.display === "none") return null;
    const box = el.getBoundingClientRect();
    const px = (value) => Number.parseFloat(value) || 0;
    return {
      width: box.width + px(style.marginLeft) + px(style.marginRight),
      height: box.height + px(style.marginTop) + px(style.marginBottom),
    };
  };
  const keep = (el, room) => {
    if (kept.has(el) || !room) return;
    const stand = document.createElement("div");
    stand.setAttribute("data-preview-kept", "");
    stand.setAttribute("aria-hidden", "true");
    stand.style.width = `${room.width}px`;
    stand.style.height = `${room.height}px`;
    el.before(stand);
    kept.set(el, stand);
  };
  const free = (el) => {
    kept.get(el)?.remove();
    kept.delete(el);
  };
  // A popover is measured as it is about to open, while it is still in place, and
  // gives its room back as it is about to close, so the page never moves between.
  addEventListener(
    "beforetoggle",
    (event) => {
      const el = event.target;
      if (!(el instanceof Element) || !el.matches(PICTURES)) return;
      if (event.newState === "open") keep(el, roomOf(el));
      else free(el);
    },
    true,
  );
  // A dialog opened with showModal() may say nothing before it opens, so its room is
  // read while it is in place, and its stand-in goes in before the next paint.
  const dialogs = [...document.querySelectorAll(PICTURES)].filter((el) => el instanceof HTMLDialogElement);
  /** @type {WeakMap<Element, {width: number, height: number}>} */
  const rooms = new WeakMap();
  const sizes = new ResizeObserver(() => {
    for (const dialog of dialogs) {
      const room = dialog.open ? null : roomOf(dialog);
      if (room) rooms.set(dialog, room);
    }
  });
  for (const dialog of dialogs) sizes.observe(dialog);
  new MutationObserver((changes) => {
    for (const { target } of changes) {
      if (!dialogs.includes(target)) continue;
      if (target.open) keep(target, rooms.get(target));
      else free(target);
    }
  }).observe(document.body, { subtree: true, attributes: true, attributeFilter: ["open"] });
}

// A searchable gallery: someone types, the cells that do not match go away. The
// cells are plain markup, so without this script the gallery still shows them all.
for (const form of document.querySelectorAll("form[data-preview-search]")) {
  const input = form.querySelector("input");
  const count = form.querySelector("[data-preview-count]");
  const cells = [...document.querySelectorAll("[data-preview-cell][data-terms]")];
  const empty = document.querySelector("[data-preview-empty]");
  const noun = form.dataset.noun ?? "results";
  const total = cells.length;
  let pending = 0;
  const filter = () => {
    const words = input.value.toLowerCase().split(/\s+/).filter(Boolean);
    let shown = 0;
    for (const cell of cells) {
      const terms = cell.dataset.terms;
      const match = words.every((word) => terms.includes(word));
      if (cell.hidden === match) cell.hidden = !match;
      if (match) shown += 1;
    }
    count.textContent = words.length ? `${shown} of ${total} ${noun}` : `${total} ${noun}`;
    if (empty) empty.hidden = shown > 0;
    pending = 0;
  };
  // One pass per frame, however fast the typing.
  input.addEventListener("input", () => {
    if (!pending) pending = requestAnimationFrame(filter);
  });
  form.addEventListener("submit", (event) => event.preventDefault());
}

// Live parts, replayed: what a stream would bring was drawn by the app's own Rust
// into a template, and this plays it one piece at a time. Each piece names the
// list it joins (`data-preview-into`, or the template's `data-preview-replay`),
// whether it goes at the end (`data-preview-at="end"`) rather than the top, and
// an element it takes the place of (`data-preview-replaces`), such as a card that
// moves or a count that changes. A static page has no server to stream from; in an
// app the server sends them.
for (const template of document.querySelectorAll("template[data-preview-replay]")) {
  const pieces = [...template.content.children];
  let at = 0;
  const next = () => {
    const piece = pieces[at++];
    if (!piece) return;
    const into = document.getElementById(piece.dataset.previewInto || template.dataset.previewReplay || "");
    const old = piece.dataset.previewReplaces ? document.getElementById(piece.dataset.previewReplaces) : null;
    if (into) {
      piece.setAttribute("data-preview-new", "");
      if (old && old.parentElement === into && piece.dataset.previewAt !== "end") old.replaceWith(piece);
      else {
        old?.remove();
        if (piece.dataset.previewAt === "end") into.append(piece);
        else into.prepend(piece);
      }
    }
    setTimeout(next, 2800);
  };
  setTimeout(next, 1600);
}
