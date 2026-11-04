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

// A live feed, replayed: the rows a stream would bring were drawn by the app's own
// Rust into a template, and this adds them to the top of the list one by one. A
// static page has no server to stream from; in an app the server sends them.
for (const template of document.querySelectorAll("template[data-preview-replay]")) {
  const list = document.getElementById(template.dataset.previewReplay ?? "");
  const rows = [...template.content.children];
  if (!list) continue;
  let at = 0;
  const next = () => {
    const row = rows[at++];
    if (!row) return;
    row.setAttribute("data-preview-new", "");
    list.prepend(row);
    setTimeout(next, 2800);
  };
  setTimeout(next, 1600);
}
