// The proof app's pad, for contract 1 of Navree UI's extensions: a few lines drawn into the element's owned
// body from the server's props, buttons that call the app's own routes through `ui.call`, and, for the vendor
// pad, the markup a library writes into the body it is given.
//
// It keeps what it has done in `window.__pad`, in all and by the id of each element, so the browser suite can
// see every mount, update and leave and every call's answer. `window.__seal` holds what the suite measures of
// the seal; the cells the pad writes add their own figures to it.
export const contract = 1;

/** What the markup of a library looks like when it writes a customer's address, a part with hooks and a field. */
const VENDOR =
  `<form data-ui-form method="post" action="/account/email"><input name="email" aria-label="Email" value="harpreet@ludhiana.example"><button id="vendor-send">Show on map</button></form>` +
  `<div data-ui-own><button id="planted" type="button" data-ui-click='[["toggle","menu"]]'>Open</button></div>` +
  `<input id="vendor-charge" name="charge" aria-label="Charge" value="0">`;

/** One cell that carries a hook. */
const CELL = `<i data-slot="app:cell" data-ui-click='[["toggle","menu"]]'></i>`;

/**
 * What the pads of this page have done: in all, and by the id of the element.
 * @param {string} id
 */
function note(id) {
  const all = (window.__pad ||= { mounts: 0, updates: 0, leaves: 0, calls: [], by: {} });
  all.by[id] ||= { mounts: 0, updates: 0, leaves: 0, props: null };
  return [all, all.by[id]];
}

/**
 * Writes 2,000 cells that each carry a hook, in one write, and records how long the seal took to take the
 * hooks off, and how many were left when the next frame began.
 * @param {HTMLElement} body
 */
async function addCells(body) {
  const start = performance.now();
  body.insertAdjacentHTML("beforeend", `<div data-slot="app:cells">${CELL.repeat(2000)}</div>`);
  const written = performance.now();
  const last = body.lastElementChild?.lastElementChild;
  let turns = 0;
  // The seal takes the hooks off when the microtasks that follow this write run: the last cell is the last it reaches.
  while (last?.hasAttribute("data-ui-click") && turns < 100) {
    await Promise.resolve();
    turns++;
  }
  const scrubbed = performance.now();
  await new Promise((resolve) => requestAnimationFrame(resolve));
  const left = body.querySelectorAll("[data-ui-click]").length;
  (window.__seal ||= {}).cells = {
    count: 2000,
    writeMs: written - start,
    scrubMs: scrubbed - written,
    turns,
    leftAtFrame: left,
    frameMs: performance.now() - scrubbed,
    done: true,
  };
}

/** @param {HTMLElement} el @param {any} props @param {any} ui */
export function mount(el, props, ui) {
  const [all, mine] = note(el.id);
  all.mounts++;
  mine.mounts++;
  mine.props = props;
  const body = ui.find('[data-slot="app:pad-body"]');
  if (!body) throw new Error("the pad has no body");
  // Text, never markup: the props are data.
  const line = (/** @type {string} */ text) => {
    const p = document.createElement("p");
    p.setAttribute("data-slot", "app:pad-line");
    p.textContent = text;
    body.append(p);
  };
  line(`${props.depot}, step ${props.step}`);
  // How many lines it drew when it mounted: a mark the extension keeps through a morph from the server,
  // which draws the element without it.
  el.setAttribute("data-count", String(body.querySelectorAll('[data-slot="app:pad-line"]').length));

  const call = async (/** @type {string} */ url, /** @type {Record<string, string>} */ data) => {
    const answer = await ui.call(url, data);
    all.calls.push({ url, ok: answer.ok, status: answer.status });
  };
  const wire = (/** @type {string} */ id, /** @type {() => unknown} */ run) => {
    const button = ui.find(`#${id}`);
    if (button) ui.on(button, "click", run);
  };
  wire("call-save", () => call("/extensions/pad/save", { n: props.step }));
  wire("call-fail", () => call("/extensions/pad/fail", { n: props.step }));
  // An address the props hold: the server did not name it for this element, so no request leaves.
  wire("call-planted", () => call(props.next, {}));
  wire("add-cells", () => addCells(body));

  if (props.vendor) {
    body.insertAdjacentHTML("beforeend", VENDOR);
    // A library keeps writing after it first draws: it points a field at the page's form, takes the
    // body's mark away and adds a field of its own.
    ui.later(0, () => {
      body.querySelector("#vendor-charge")?.setAttribute("form", "new-delivery");
      body.removeAttribute("data-ui-stored");
      const late = document.createElement("input");
      late.setAttribute("id", "vendor-late");
      late.setAttribute("name", "charge");
      late.setAttribute("aria-label", "Late charge");
      late.setAttribute("value", "1");
      body.append(late);
    });
  }

  return {
    /** New props from a morph of the element. @param {any} next */
    update(next) {
      all.updates++;
      mine.updates++;
      mine.props = next;
      props = next;
      line(`${next.depot}, step ${next.step}`);
    },
    leave() {
      all.leaves++;
      mine.leaves++;
    },
  };
}
