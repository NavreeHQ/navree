// navree-browser's page script: finds targets and checks what a user would
// see. Evaluated as `(<this>)()`; every function takes one JSON argument and
// returns JSON. `{ wait: code }` means "not yet" (the caller retries until its
// timeout); `{ fatal: message }` means retrying cannot help.
() => {
  const norm = (s) => String(s ?? "").replace(/\s+/g, " ").trim();

  const FIELDS = [
    "input:not([type=hidden])", "textarea", "select", "[contenteditable='']", "[contenteditable=true]",
    "[role=textbox]", "[role=searchbox]", "[role=combobox]", "[role=listbox]", "[role=checkbox]",
    "[role=switch]", "[role=radio]", "[role=spinbutton]", "[role=slider]",
  ].join(",");
  const BUTTONS = [
    "button", "a", "[role=button]", "[role=link]", "[role=menuitem]", "[role=menuitemcheckbox]",
    "[role=menuitemradio]", "[role=tab]", "[role=option]", "[role=treeitem]", "input[type=submit]",
    "input[type=button]", "input[type=reset]", "input[type=image]", "summary",
  ].join(",");
  const OPTIONS = "[role=option],[role=menuitem],[role=menuitemradio],[role=treeitem]";
  const NO_TEXT = new Set(["SCRIPT", "STYLE", "NOSCRIPT", "TEMPLATE", "HEAD", "TITLE"]);
  const SET_VALUE = new Set(["date", "time", "datetime-local", "month", "week", "color", "range"]);

  class Fatal extends Error {}

  function visible(el) {
    if (!el || !el.isConnected || el.nodeType !== 1) return false;
    if (typeof el.checkVisibility === "function" && !el.checkVisibility({ visibilityProperty: true })) return false;
    if (getComputedStyle(el).visibility !== "visible") return false;
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  }

  function enabled(el) {
    if (el.matches(":disabled")) return false;
    for (let e = el; e && e.getAttribute; e = e.parentElement) {
      if (e.getAttribute("aria-disabled") === "true") return false;
    }
    return true;
  }

  function editable(el) {
    if (el.readOnly) return false;
    return el.getAttribute("aria-readonly") !== "true";
  }

  function textOf(el) {
    return norm(visible(el) ? el.innerText : el.textContent);
  }

  function labelText(label) {
    const clone = label.cloneNode(true);
    clone.querySelectorAll("input,select,textarea,button,[role=combobox],[role=listbox]").forEach((c) => c.remove());
    return norm(clone.textContent);
  }

  function byIds(ids) {
    return norm(
      String(ids)
        .split(/\s+/)
        .filter(Boolean)
        .map((id) => {
          const e = document.getElementById(id);
          return e ? e.textContent : "";
        })
        .join(" "),
    );
  }

  function labelsOf(el) {
    const out = [];
    if (el.labels) {
      for (const l of el.labels) out.push(l);
    } else if (el.id) {
      for (const l of document.querySelectorAll(`label[for="${CSS.escape(el.id)}"]`)) out.push(l);
    }
    const wrapping = el.parentElement && el.parentElement.closest("label");
    if (wrapping && !out.includes(wrapping)) out.push(wrapping);
    return out;
  }

  function fieldNames(el) {
    const names = labelsOf(el).map(labelText);
    const by = el.getAttribute("aria-labelledby");
    if (by) names.push(byIds(by));
    for (const attr of ["aria-label", "placeholder", "name"]) {
      const v = el.getAttribute(attr);
      if (v != null) names.push(norm(v));
    }
    return names;
  }

  function buttonNames(el) {
    const names = [];
    const by = el.getAttribute("aria-labelledby");
    if (by) names.push(byIds(by));
    const label = el.getAttribute("aria-label");
    if (label != null) names.push(norm(label));
    names.push(norm(el.textContent));
    if (visible(el)) names.push(norm(el.innerText));
    if (el.tagName === "INPUT") {
      names.push(norm(el.value));
      const alt = el.getAttribute("alt");
      if (alt) names.push(norm(alt));
    }
    const title = el.getAttribute("title");
    if (title) names.push(norm(title));
    for (const img of el.querySelectorAll("img[alt]")) names.push(norm(img.getAttribute("alt")));
    return names;
  }

  function css(root, selector) {
    try {
      return [...root.querySelectorAll(selector)];
    } catch (e) {
      throw new Fatal(`${JSON.stringify(selector)} is not a valid CSS selector`);
    }
  }

  // The deepest elements whose whole text is `text`.
  function exactText(root, text) {
    const found = new Set();
    const walker = document.createTreeWalker(root === document ? document.body || document : root, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const data = norm(node.data);
      if (!data || !text.includes(data)) continue;
      for (let el = node.parentElement; el; el = el.parentElement) {
        if (NO_TEXT.has(el.tagName)) break;
        if (root !== document && !root.contains(el)) break;
        const t = norm(el.textContent);
        if (t === text) {
          found.add(el);
          break;
        }
        if (t.length > text.length) break;
      }
    }
    const all = [...found];
    return all.filter((el) => !all.some((other) => other !== el && el.contains(other)));
  }

  // Candidates for a text target, best kind first: fields by label, then
  // buttons and links by name, then any element with exactly that text.
  function byText(root, text, mode) {
    const out = new Set();
    if (mode === "option") {
      for (const el of css(root, OPTIONS)) if (buttonNames(el).includes(text)) out.add(el);
      return [...out];
    }
    if (mode !== "press") {
      for (const el of css(root, FIELDS)) if (fieldNames(el).includes(text)) out.add(el);
    }
    if (mode === "field") return [...out];
    for (const el of css(root, BUTTONS)) if (buttonNames(el).includes(text)) out.add(el);
    if (mode === "press") return [...out];
    for (const el of exactText(root, text)) out.add(el);
    return [...out];
  }

  function candidates(root, t, mode) {
    if (t.kind === "css") return css(root, t.value);
    if (t.kind === "testid") return css(root, `[data-testid="${CSS.escape(t.value)}"]`);
    return byText(root, t.value, mode);
  }

  // The matches of a target: visible ones (all, visible first, with
  // `hidden`), filtered by `containing`, then `nth`.
  function matches(root, t, mode, hidden) {
    const all = candidates(root, t, mode);
    const shown = all.filter(visible);
    let list = hidden ? shown.concat(all.filter((e) => !shown.includes(e))) : shown;
    if (t.containing != null) {
      const c = norm(t.containing);
      list = list.filter((e) => textOf(e).includes(c) || norm(e.textContent).includes(c));
    }
    if (t.nth != null) list = t.nth < list.length ? [list[t.nth]] : [];
    return { list, total: all.length };
  }

  function scope(chain) {
    let root = document;
    for (let i = 0; i < (chain || []).length; i++) {
      const { list } = matches(root, chain[i], "any", false);
      if (!list.length) return { missing: i };
      root = list[0];
    }
    return { root };
  }

  // The first match of `a.target` in the scope, or a reason to wait.
  function first(a, mode, hidden) {
    const s = scope(a.chain);
    if (s.missing != null) return { wait: "scope", index: s.missing };
    const { list, total } = matches(s.root, a.target, mode, hidden);
    if (!list.length) return { wait: total ? "hidden" : "missing" };
    return { el: list[0], root: s.root };
  }

  function describe(el) {
    let text = `<${el.tagName.toLowerCase()}`;
    if (el.id) text += ` id="${el.id}"`;
    const cls = typeof el.className === "string" ? norm(el.className) : "";
    if (cls) text += ` class="${cls.slice(0, 60)}"`;
    text += ">";
    const inner = norm(el.textContent).slice(0, 40);
    return inner ? `${text} ${JSON.stringify(inner)}` : text;
  }

  function settle() {
    return new Promise((resolve) => {
      let done = false;
      const finish = () => {
        if (!done) {
          done = true;
          resolve();
        }
      };
      requestAnimationFrame(() => requestAnimationFrame(finish));
      setTimeout(finish, 50);
    });
  }

  function labelledBy(hit, el) {
    const label = hit.closest && hit.closest("label");
    return !!label && (label.control === el || label.contains(el) || labelsOf(el).includes(label));
  }

  // Scrolls `el` into view and returns its center if it is steady and
  // nothing covers it.
  async function hitPoint(el, owner) {
    el.scrollIntoView({ block: "center", inline: "center", behavior: "instant" });
    const before = el.getBoundingClientRect();
    await settle();
    if (!el.isConnected) return { wait: "missing" };
    const r = el.getBoundingClientRect();
    if (Math.abs(r.x - before.x) > 1 || Math.abs(r.y - before.y) > 1 || Math.abs(r.width - before.width) > 1 || Math.abs(r.height - before.height) > 1) {
      return { wait: "moving" };
    }
    const x = r.left + r.width / 2;
    const y = r.top + r.height / 2;
    if (x < 0 || y < 0 || x > innerWidth || y > innerHeight) return { wait: "offscreen" };
    const hit = document.elementFromPoint(x, y);
    if (!hit) return { wait: "offscreen" };
    if (hit === el || el.contains(hit) || labelledBy(hit, owner || el)) return { ok: true, x, y };
    return { wait: "covered", by: describe(hit) };
  }

  // A point inside `el` to press for a drag: its center, else a point near
  // a corner, avoiding links and images inside it (a press there starts the
  // browser's own drag of the link) unless `el` itself is draggable.
  function grabPoint(el) {
    const r = el.getBoundingClientRect();
    const inset = Math.min(12, r.width / 4, r.height / 4);
    const points = [
      [r.left + r.width / 2, r.top + r.height / 2],
      [r.left + inset, r.bottom - inset],
      [r.left + inset, r.top + inset],
      [r.right - inset, r.bottom - inset],
      [r.right - inset, r.top + inset],
    ];
    const native = el.draggable || el.closest("a[href], img");
    for (const [x, y] of points) {
      if (x < 0 || y < 0 || x > innerWidth || y > innerHeight) continue;
      const hit = document.elementFromPoint(x, y);
      if (!hit || !(hit === el || el.contains(hit))) continue;
      if (!native && hit.closest("a[href], img, [draggable=true]") && hit.closest("a[href], img, [draggable=true]") !== el) continue;
      return [x, y];
    }
    return null;
  }

  function checkState(el) {
    if (el.tagName === "INPUT" && (el.type === "checkbox" || el.type === "radio")) {
      return { checked: el.checked, radio: el.type === "radio" };
    }
    const role = el.getAttribute("role");
    if (role === "checkbox" || role === "switch" || role === "radio" || el.hasAttribute("aria-checked")) {
      return { checked: el.getAttribute("aria-checked") === "true", radio: role === "radio" };
    }
    return null;
  }

  function nativeSet(el, value) {
    const proto = el.tagName === "TEXTAREA" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(proto, "value").set;
    setter.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  }

  function guard(fn) {
    return async (a) => {
      try {
        return await fn(a || {});
      } catch (e) {
        if (e instanceof Fatal) return { fatal: e.message };
        throw e;
      }
    };
  }

  return {
    // A point to click: the first match, visible, enabled (a.enabled), not
    // covered and not moving.
    point: guard(async (a) => {
      const f = first(a, a.mode || "any", false);
      if (!f.el) return f;
      if (a.enabled && !enabled(f.el)) return { wait: "disabled" };
      return hitPoint(f.el);
    }),

    // Focuses a field for typing; with `clear`, selects its content.
    focus: guard(async (a) => {
      const f = first(a, "field", false);
      if (!f.el) return f;
      const el = f.el;
      if (!enabled(el)) return { wait: "disabled" };
      if (!editable(el)) return { wait: "readonly" };
      const p = await hitPoint(el);
      if (!p.ok) return p;
      el.focus();
      const active = document.activeElement;
      if (active !== el && !el.contains(active)) return { wait: "unfocused" };
      if (el.tagName === "INPUT" && SET_VALUE.has(el.type)) return { ok: true, set: true };
      const value = el.isContentEditable ? el.textContent : String(el.value ?? "");
      if (el.isContentEditable) {
        const range = document.createRange();
        range.selectNodeContents(el);
        if (!a.clear) range.collapse(false);
        const sel = getSelection();
        sel.removeAllRanges();
        sel.addRange(range);
      } else if (a.clear) {
        if (typeof el.select === "function") el.select();
      } else {
        try {
          el.setSelectionRange(value.length, value.length);
        } catch (e) {
          // Inputs without a text selection (email, number) keep the caret.
        }
      }
      return { ok: true, set: false, hasText: value.length > 0 };
    }),

    // After clearing with Backspace: empties the field if a selection
    // could not cover it (email and number inputs).
    emptied: guard(async (a) => {
      const f = first(a, "field", false);
      if (!f.el) return f;
      const el = f.el;
      const value = el.isContentEditable ? el.textContent : String(el.value ?? "");
      if (value.length && !el.isContentEditable) nativeSet(el, "");
      return { ok: true };
    }),

    // Sets the value of date, time, color and range inputs.
    setValue: guard(async (a) => {
      const f = first(a, "field", false);
      if (!f.el) return f;
      nativeSet(f.el, a.value);
      return { ok: true };
    }),

    // Selects an option of a native <select>, or reports a custom one.
    select: guard(async (a) => {
      const f = first(a, "field", false);
      if (!f.el) return f;
      const el = f.el;
      if (!enabled(el)) return { wait: "disabled" };
      if (el.tagName !== "SELECT") {
        const p = await hitPoint(el);
        if (!p.ok) return p;
        return { custom: true, x: p.x, y: p.y };
      }
      const options = [...el.options];
      const want = norm(a.option);
      const option = options.find((o) => norm(o.textContent) === want || norm(o.label) === want || o.value === a.option);
      if (!option) return { wait: "option", option: a.option, options: options.map((o) => norm(o.textContent)) };
      if (option.disabled) return { wait: "option-disabled", option: a.option };
      el.focus();
      option.selected = true;
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
      return { ok: true };
    }),

    // A checkbox, radio or switch: its state and where to click it (the
    // control, else one of its labels).
    checkbox: guard(async (a) => {
      const f = first(a, "field", true);
      if (!f.el) return f;
      const el = f.el;
      const state = checkState(el);
      if (!state) return { fatal: "not-checkable" };
      if (state.checked === a.checked) return { ok: true, done: true, ...state };
      if (!a.checked && state.radio) return { fatal: "radio" };
      if (!enabled(el)) return { wait: "disabled" };
      const targets = [el, ...labelsOf(el)].filter(visible);
      let last = { wait: "hidden" };
      for (const target of targets) {
        const p = await hitPoint(target, el);
        if (p.ok) return { ok: true, done: false, x: p.x, y: p.y, ...state };
        last = p;
      }
      return last;
    }),

    // The checked state of the first matching checkbox (after a click).
    checked: guard(async (a) => {
      const f = first(a, "field", true);
      if (!f.el) return f;
      const state = checkState(f.el);
      return { ok: true, checked: state ? state.checked : false };
    }),

    // A file input (visible or not); returns the element itself.
    fileInput: guard(async (a) => {
      const f = first(a, "field", true);
      if (!f.el) return null;
      return f.el.tagName === "INPUT" && f.el.type === "file" ? f.el : null;
    }),

    fileState: guard(async (a) => {
      const f = first(a, "field", true);
      if (!f.el) return f;
      if (!(f.el.tagName === "INPUT" && f.el.type === "file")) return { fatal: "not-file" };
      if (!enabled(f.el)) return { wait: "disabled" };
      return { ok: true };
    }),

    // Where a drag starts and ends.
    drag: guard(async (a) => {
      const from = first({ chain: a.chain, target: a.from }, "any", false);
      if (!from.el) return { ...from, which: "from" };
      const to = first({ chain: a.chain, target: a.to }, "any", false);
      if (!to.el) return { ...to, which: "to" };
      if (!enabled(from.el)) return { wait: "disabled", which: "from" };
      const start = await hitPoint(from.el);
      if (!start.ok) return { ...start, which: "from" };
      let r = to.el.getBoundingClientRect();
      if (r.left + r.width / 2 < 0 || r.top + r.height / 2 < 0 || r.left + r.width / 2 > innerWidth || r.top + r.height / 2 > innerHeight) {
        to.el.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "instant" });
        await settle();
        r = to.el.getBoundingClientRect();
      }
      const s = from.el.getBoundingClientRect();
      const center = [s.left + s.width / 2, s.top + s.height / 2];
      if (center[0] < 0 || center[1] < 0 || center[0] > innerWidth || center[1] > innerHeight) {
        return { wait: "apart", which: "to" };
      }
      const grab = grabPoint(from.el) || center;
      // The middle of the target's visible part.
      const left = Math.max(r.left, 0);
      const top = Math.max(r.top, 0);
      const right = Math.min(r.right, innerWidth);
      const bottom = Math.min(r.bottom, innerHeight);
      if (right <= left || bottom <= top) return { wait: "offscreen", which: "to" };
      return { ok: true, from: grab, to: [(left + right) / 2, (top + bottom) / 2] };
    }),

    scrollTo: guard(async (a) => {
      const f = first(a, "any", false);
      if (!f.el) return f;
      f.el.scrollIntoView({ block: "center", inline: "center", behavior: "instant" });
      return { ok: true };
    }),

    // Whether `a.text` is in the visible text of the scope (or the page).
    see: guard(async (a) => {
      const s = scope(a.chain);
      if (s.missing != null) return { wait: "scope", index: s.missing };
      const root = s.root === document ? document.body || document.documentElement : s.root;
      if (!root) return { seen: false };
      const want = norm(a.text);
      if (norm(root.innerText).includes(want)) return { seen: true };
      // Visible text without CSS text-transform: the text of visible elements.
      let text = "";
      const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        const parent = node.parentElement;
        if (parent && !NO_TEXT.has(parent.tagName) && visible(parent)) text += node.data;
      }
      return { seen: norm(text).includes(want) };
    }),

    // Whether a visible match of the target contains the text.
    seeIn: guard(async (a) => {
      const s = scope(a.chain);
      if (s.missing != null) return { wait: "scope", index: s.missing };
      const { list, total } = matches(s.root, a.target, "any", false);
      if (!list.length) return { wait: total ? "hidden" : "missing" };
      const want = norm(a.text);
      return { seen: list.some((e) => textOf(e).includes(want) || norm(e.textContent).includes(want)) };
    }),

    count: guard(async (a) => {
      const s = scope(a.chain);
      if (s.missing != null) return { wait: "scope", index: s.missing };
      const { list, total } = matches(s.root, a.target, a.mode || "any", false);
      return { count: list.length, total };
    }),

    text: guard(async (a) => {
      const f = first(a, "any", false);
      if (!f.el) return f;
      return { ok: true, text: String(f.el.innerText ?? "").trim() };
    }),

    // The text of the scope element itself.
    ownText: guard(async (a) => {
      const s = scope(a.chain);
      if (s.missing != null) return { wait: "scope", index: s.missing };
      const root = s.root === document ? document.body : s.root;
      return { ok: true, text: String(root.innerText ?? "").trim() };
    }),

    value: guard(async (a) => {
      const f = first(a, "field", false);
      if (!f.el) return f;
      const el = f.el;
      let value;
      if (el.isContentEditable) value = el.innerText;
      else if ("value" in el && el.tagName !== "BUTTON") value = String(el.value);
      else value = el.getAttribute("aria-valuetext") ?? el.getAttribute("aria-valuenow") ?? norm(el.textContent);
      const shown = el.tagName === "SELECT" ? [...el.selectedOptions].map((o) => norm(o.textContent)).join(", ") : null;
      return { ok: true, value, shown };
    }),

    enabled: guard(async (a) => {
      const f = first(a, "any", false);
      if (!f.el) return f;
      return { ok: true, enabled: enabled(f.el) };
    }),

    location: guard(async () => ({
      href: location.href,
      path: location.pathname,
      search: location.search,
      query: [...new URLSearchParams(location.search)],
      title: document.title,
      ready: document.readyState,
    })),

    html: guard(async () => {
      const doctype = document.doctype ? `<!DOCTYPE ${document.doctype.name}>\n` : "";
      return doctype + (document.documentElement ? document.documentElement.outerHTML : "");
    }),

    size: guard(async () => {
      const e = document.documentElement;
      const b = document.body;
      return {
        width: Math.ceil(Math.max(e.scrollWidth, b ? b.scrollWidth : 0, innerWidth)),
        height: Math.ceil(Math.max(e.scrollHeight, b ? b.scrollHeight : 0, innerHeight)),
      };
    }),
  };
}
