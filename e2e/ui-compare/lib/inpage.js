// Runs inside the page before any of the page's own script, in every load the harness
// makes. It records what the browser reports about painting and long tasks, remembers the
// time of each press and key that starts a gesture, and waits for a described state of
// the page. Everything it times is on the page's own clock, so the harness's own work
// is not in any number.
//
// A "probe" says what must be on the screen. All its parts must hold:
//   { sel }                  the selector, as for querySelectorAll
//   { hasText: "Pune" }      keeps the elements whose whole text is exactly this
//   { nth: 0 }               which of them the text, attr and prop tests look at
//   { exists: true }         there is at least one (false: there is none)
//   { visible: true }        at least one is drawn (false: none is)
//   { count: 50 }            exactly this many
//   { text: "Pune" }         the text of the nth one
//   { attr: ["name", "v"] }  an attribute of the nth one
//   { prop: ["value", "v"] } a property of the nth one, as text
//   { all: [probe, ...] }    every one of these
(() => {
  if (window.__cmp) return;

  const out = { fcp: null, lcp: null, tasks: [], gestures: [] };

  const watch = (type, onEntry) => {
    try {
      new PerformanceObserver((list) => list.getEntries().forEach(onEntry)).observe({ type, buffered: true });
    } catch {
      // A browser without this entry type simply reports nothing for it.
    }
  };
  watch("paint", (entry) => {
    if (entry.name === "first-contentful-paint") out.fcp = entry.startTime;
  });
  watch("largest-contentful-paint", (entry) => {
    out.lcp = entry.startTime;
  });
  watch("longtask", (entry) => {
    out.tasks.push([entry.startTime, entry.duration]);
  });
  for (const type of ["pointerdown", "keydown"]) {
    window.addEventListener(type, (event) => out.gestures.push(event.timeStamp), true);
  }

  const text = (el) => (el.textContent || "").replace(/\s+/g, " ").trim();

  const visible = (el) => {
    if (!el.isConnected) return false;
    if (typeof el.checkVisibility === "function" && !el.checkVisibility()) return false;
    const box = el.getBoundingClientRect();
    return box.width > 0 && box.height > 0;
  };

  const pick = (probe) => {
    let list = Array.from(document.querySelectorAll(probe.sel));
    if (probe.hasText !== undefined) list = list.filter((el) => text(el) === probe.hasText);
    return list;
  };

  const test = (probe) => {
    if (probe.all) return probe.all.every(test);
    const list = pick(probe);
    const el = list[probe.nth || 0];
    if (probe.exists !== undefined && list.length > 0 !== probe.exists) return false;
    if (probe.visible !== undefined && list.some(visible) !== probe.visible) return false;
    if (probe.count !== undefined && list.length !== probe.count) return false;
    if (probe.text !== undefined && (!el || text(el) !== probe.text)) return false;
    if (probe.attr !== undefined && (!el || el.getAttribute(probe.attr[0]) !== probe.attr[1])) return false;
    if (probe.prop !== undefined && (!el || String(el[probe.prop[0]]) !== probe.prop[1])) return false;
    return true;
  };

  const holds = (probe) => {
    try {
      return test(probe);
    } catch {
      return false;
    }
  };

  /** What a probe finds now, for the message when it never holds. */
  const describe = (probe) => {
    if (probe.all) return probe.all.map(describe);
    try {
      const list = pick(probe);
      const el = list[probe.nth || 0];
      return { sel: probe.sel, holds: holds(probe), found: list.length, first: el ? text(el).slice(0, 80) : null };
    } catch (error) {
      return { sel: probe.sel, error: String(error) };
    }
  };

  const WATCHED_EVENTS = ["toggle", "input", "change", "focusin", "transitionend", "animationend"];

  /** Resolves when `probe` holds and two frames have been drawn after that. */
  const wait = (probe, timeout) =>
    new Promise((resolve) => {
      let finished = false;
      let frame = 0;
      let timer = 0;
      const observer = new MutationObserver(() => check());
      const stop = () => {
        observer.disconnect();
        cancelAnimationFrame(frame);
        clearTimeout(timer);
        for (const type of WATCHED_EVENTS) document.removeEventListener(type, check, true);
      };
      const settle = (result) => {
        api.result = result;
        resolve(result);
      };
      function check() {
        if (finished || !holds(probe)) return;
        finished = true;
        stop();
        const seen = performance.now();
        requestAnimationFrame(() =>
          requestAnimationFrame(() => {
            const drawn = performance.now();
            let started = null;
            for (const stamp of out.gestures) if (stamp <= seen) started = stamp;
            settle({ ok: true, seen, drawn, started });
          }),
        );
      }
      const poll = () => {
        check();
        if (!finished) frame = requestAnimationFrame(poll);
      };
      observer.observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
      for (const type of WATCHED_EVENTS) document.addEventListener(type, check, true);
      frame = requestAnimationFrame(poll);
      timer = setTimeout(() => {
        if (finished) return;
        finished = true;
        stop();
        settle({ ok: false, reason: "timeout", detail: describe(probe) });
      }, timeout);
    });

  const api = {
    out,
    result: null,
    pending: null,

    /**
     * Starts waiting for `probe`. Returns true when it already holds, and then waits for
     * nothing: the step's state was on the screen before the step began.
     */
    arm(probe, timeout = 20000) {
      api.result = null;
      if (holds(probe)) return true;
      api.pending = wait(probe, timeout);
      return false;
    },

    /** The centre of the first drawn element of `spec`, scrolled into view; null if none is drawn. */
    pointOf(spec) {
      const el = pick(spec).find(visible);
      if (!el) return null;
      let box = el.getBoundingClientRect();
      if (box.top < 0 || box.left < 0 || box.bottom > innerHeight || box.right > innerWidth) {
        el.scrollIntoView({ block: "center", inline: "center", behavior: "instant" });
        box = el.getBoundingClientRect();
      }
      return { x: box.left + box.width / 2, y: box.top + box.height / 2 };
    },

    /** Focuses the first drawn element of `spec` and selects its text; false if none is drawn. */
    focusSelect(spec) {
      const el = pick(spec).find(visible);
      if (!el) return false;
      el.focus({ preventScroll: true });
      if (typeof el.select === "function") el.select();
      return document.activeElement === el;
    },

    /** What the page recorded about its own load. */
    paint() {
      const navigation = performance.getEntriesByType("navigation")[0];
      return {
        fcp: out.fcp,
        lcp: out.lcp,
        tasks: out.tasks,
        responseStart: navigation ? navigation.responseStart : null,
      };
    },

    /** Evaluates `probe` once, for the harness's own checks. */
    holds,
    describe,
    text: (selector) => Array.from(document.querySelectorAll(selector)).map(text),
  };

  window.__cmp = api;
})();
