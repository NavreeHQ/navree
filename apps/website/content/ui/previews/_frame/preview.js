// navree.com's preview frame. A preview shows a component, not the app behind
// it, so its links and forms go nowhere. This runs before the toolkit's modules
// and in the capture phase, so they see the event as already handled.
addEventListener(
  "click",
  (event) => {
    const target = event.target instanceof Element ? event.target : null;
    if (target?.closest("a[href]")) event.preventDefault();
  },
  true,
);
addEventListener("submit", (event) => event.preventDefault(), true);
