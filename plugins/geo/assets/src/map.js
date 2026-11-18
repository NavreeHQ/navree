// The map of the geo plugin, for contract 1 of Navree UI's extensions: Leaflet drawn into the element's
// owned body. When the element is a form field's control (`props.pick` names the field), a click on the
// map, or the device's own place, becomes the field's value, asks `/geo/reverse` for the nearest place,
// and writes the place line inside this element as text.
//
// What it is given: the props the server wrote on the element, and `ui`, whose helpers reach only this
// element's own parts, its own timers and listeners (they end when the element leaves) and the routes
// the server named for it (`/geo/`). It never touches the page outside its element.
//
// Leaflet is taken from its module build, named by its file: the package's own entry is the script
// build, which also writes a global named `L`.
import * as L from "leaflet/dist/leaflet-src.esm.js";

export const contract = 1;

/** Leaflet writes an attribution as HTML: the server's text is turned into text first. @param {unknown} s */
const escapeText = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

/** `lat,lng` as a pair of numbers, or null when the text is not one. @param {string} text */
const point = (text) => {
  const [lat, lng] = String(text || "").split(",").map(Number);
  return Number.isFinite(lat) && Number.isFinite(lng) ? [lat, lng] : null;
};

/**
 * @param {Element} el
 * @param {any} props
 * @param {any} ui
 */
export function mount(el, props, ui) {
  const body = ui.find("[data-slot='geo:map-body']");
  const input = props.pick ? ui.find(`input[name="${CSS.escape(props.pick)}"]`) : null;
  const locate = ui.find("[data-slot='geo:locate']");
  const place = ui.find("[data-slot='geo:place']");
  let map = null, pin = null, last = input?.value || "";
  // Whether the field, or the page, gave the map a place of its own: an empty field's map opens on the
  // delivery area's centre with no pin on it.
  let placed = Boolean(props.center);
  const at = () => [props.center.lat, props.center.lng];
  /** The pin, made where it is first wanted, with no picture files. @param {any} p */
  const put = (p) => {
    if (pin) pin.setLatLng(p);
    else if (map) pin = L.circleMarker(p, { radius: 8 }).addTo(map);
  };

  /** @param {string | null} tiles @param {string} attribution */
  const start = (tiles, attribution) => {
    // Not allowed (no tiles), or the element left meanwhile: the server's fallback stays.
    if (!body || !tiles || !props.center || ui.signal.aborted) return;
    body.replaceChildren();
    map = L.map(body).setView(at(), props.zoom);
    L.tileLayer(tiles, { attribution: escapeText(attribution || ""), referrerPolicy: "strict-origin-when-cross-origin" }).addTo(map);
    if (placed) put(at());
    // A morph from the server keeps this mark through a move, so the page can tell a map that is moving.
    map.on("movestart", () => el.setAttribute("data-moving", ""));
    map.on("moveend", () => el.removeAttribute("data-moving"));
    if (input) map.on("click", (/** @type {any} */ e) => pick(e.latlng));
  };

  if (props.tiles) start(props.tiles, props.attribution);
  else if (props.pick) {
    // A control is drawn with no request: where its tiles are, and the centre of an empty map, come from the server.
    // On a guest page `/geo/tiles.json` answers 401 and the control keeps its coordinate input and fallback text.
    ui.get("/geo/tiles.json")
      .then((/** @type {Response} */ r) => (r.ok ? r.json() : null))
      .then((/** @type {any} */ t) => {
        if (!t) return;
        if (!props.center) props = { ...props, center: t.centre, zoom: t.zoom || props.zoom };
        start(t.tiles, t.attribution);
      })
      .catch(() => {});
  }

  // The field takes keys as any text field does: in a table cell, Enter and Escape in it are the cell's own.
  if (locate) ui.on(locate, "click", () => navigator.geolocation?.getCurrentPosition((p) => pick({ lat: p.coords.latitude, lng: p.coords.longitude })));

  // An inline edit the server refused: the cell goes back to the point it had, and so does the pin. One it
  // took is the point the cell goes back to from then on.
  ui.on(document, "ui:sent", (/** @type {CustomEvent} */ e) => {
    const { form, ok } = e.detail || {};
    if (!input || !form?.hasAttribute?.("data-ui-edit") || !form.contains(input)) return;
    if (ok) {
      last = input.value;
      return;
    }
    input.value = last;
    const back = point(last);
    if (back) put(back);
  });

  /** @param {{lat: number, lng: number}} p */
  async function pick(p) {
    if (!input || input.disabled) return;
    const text = `${p.lat.toFixed(4)},${p.lng.toFixed(4)}`;
    input.value = text;
    put(p);
    input.dispatchEvent(new Event("change", { bubbles: true })); // posts an inline edit; a form posts on submit
    try {
      const r = await ui.get(`/geo/reverse?at=${encodeURIComponent(text)}`);
      if (r.ok) {
        const d = await r.json();
        if (place && input.value === text) {
          place.textContent = d.place ? `${d.place} (${text})` : text;
        }
      }
    } catch {}
    ui.announce("Drop point moved");
  }

  return {
    /** New props from a morph of the element: a higher `seq` than the map had sends it to the new centre. @param {any} next */
    update(next) {
      const moved = map && next.center && next.seq !== props.seq;
      props = next;
      if (moved) {
        placed = true;
        map.flyTo(at(), next.zoom);
        put(at());
      }
      if (input) last = input.value;
    },
    leave() {
      map?.remove();
      el.removeAttribute("data-moving");
    },
  };
}
