// The proof page's island: a tally of parcels, drawn in the browser from the server's
// props, with a button that counts on the page alone. It keeps what it has done in
// `window.__islands`, so the browser suite can see each mount, update and leave.

/** @param {HTMLElement} body @param {{city?: string, start?: number} | null} props */
export function mount(body, props) {
  const seen = (window.__islands ||= { mounts: 0, updates: 0, leaves: 0 });
  seen.mounts++;
  let count = Number(props?.start) || 0;
  let city = String(props?.city || "the depot");
  const label = document.createElement("span");
  label.setAttribute("data-tally-city", "");
  const value = document.createElement("output");
  value.setAttribute("data-tally-count", "");
  const more = document.createElement("button");
  more.type = "button";
  more.setAttribute("data-tally-more", "");
  more.textContent = "One more parcel";
  const draw = () => {
    // Text, never markup: the props are data.
    label.textContent = `Parcels packed in ${city}: `;
    value.textContent = count.toLocaleString("en-IN");
  };
  more.addEventListener("click", () => {
    count++;
    draw();
  });
  draw();
  body.replaceChildren(label, value, more);
  return {
    update(next) {
      seen.updates++;
      city = String(next?.city || "the depot");
      draw();
    },
    leave() {
      seen.leaves++;
    },
  };
}
