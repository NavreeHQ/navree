/* Fleet dashboard. Reads the JSON API under data-api with a bearer token
   from the app's sign-in route (data-login), kept in this tab's session
   storage. Every value from the API is written with textContent. */
"use strict";

(function () {
  const body = document.body;
  const API = body.dataset.api;
  const LOGIN = body.dataset.login;
  const AUTH = LOGIN.replace(/\/login$/, "");
  const STORE = "navree.fleet";
  const REFRESH_MS = 5000;
  const VIEWS = {
    overview: "Overview",
    supervisors: "Supervisors",
    jobs: "Jobs",
    failed: "Failed jobs",
    metrics: "Metrics",
  };

  const state = {
    session: load(),
    view: "overview",
    detail: null,
    jobsTab: "pending",
    search: { jobs: "", failed: "" },
    period: "hour",
    timer: null,
    busy: false,
  };

  // ---------------------------------------------------------------- utils

  function load() {
    try {
      return JSON.parse(sessionStorage.getItem(STORE) || "null");
    } catch (_) {
      return null;
    }
  }

  function save(session) {
    state.session = session;
    try {
      if (session) sessionStorage.setItem(STORE, JSON.stringify(session));
      else sessionStorage.removeItem(STORE);
    } catch (_) {
      /* storage unavailable: the session lasts as long as the page */
    }
    renderUser();
  }

  function h(tag, attrs, children) {
    const el = document.createElement(tag);
    for (const [key, value] of Object.entries(attrs || {})) {
      if (value === null || value === undefined || value === false) continue;
      if (key === "class") el.className = value;
      else if (key === "text") el.textContent = value;
      else if (key.startsWith("on")) el.addEventListener(key.slice(2), value);
      else el.setAttribute(key, value === true ? "" : String(value));
    }
    for (const child of [].concat(children || [])) {
      if (child === null || child === undefined || child === false) continue;
      el.append(child instanceof Node ? child : document.createTextNode(String(child)));
    }
    return el;
  }

  function svg(tag, attrs) {
    const el = document.createElementNS("http://www.w3.org/2000/svg", tag);
    for (const [key, value] of Object.entries(attrs || {})) el.setAttribute(key, String(value));
    return el;
  }

  function number(n) {
    return new Intl.NumberFormat().format(n || 0);
  }

  function duration(secs) {
    secs = Math.max(0, Math.round(secs || 0));
    if (secs < 60) return secs + " s";
    if (secs < 3600) return Math.floor(secs / 60) + " min " + (secs % 60) + " s";
    return Math.floor(secs / 3600) + " h " + Math.floor((secs % 3600) / 60) + " min";
  }

  function millis(ms) {
    ms = ms || 0;
    return ms < 1000 ? Math.round(ms) + " ms" : (ms / 1000).toFixed(ms < 10000 ? 2 : 1) + " s";
  }

  function time(iso) {
    if (!iso) return "";
    const date = new Date(iso);
    return date.toLocaleString();
  }

  function ago(iso) {
    if (!iso) return "";
    return duration((Date.now() - new Date(iso).getTime()) / 1000) + " ago";
  }

  function firstLine(text) {
    return String(text || "").split("\n")[0];
  }

  function toast(message, error) {
    const el = document.getElementById("toast");
    el.textContent = message;
    el.className = "toast" + (error ? " error" : "");
    el.hidden = false;
    clearTimeout(toast.timer);
    toast.timer = setTimeout(() => (el.hidden = true), 4000);
  }

  // ------------------------------------------------------------------ API

  class ApiError extends Error {
    constructor(status, message) {
      super(message);
      this.status = status;
    }
  }

  async function request(url, options, retried) {
    const headers = { Accept: "application/json" };
    if (options && options.json !== undefined) headers["Content-Type"] = "application/json";
    if (state.session && state.session.token) headers.Authorization = "Bearer " + state.session.token;
    const response = await fetch(url, {
      method: (options && options.method) || "GET",
      headers,
      body: options && options.json !== undefined ? JSON.stringify(options.json) : undefined,
      credentials: "same-origin",
    });
    if (response.status === 401 && !retried && (await refresh())) {
      return request(url, options, true);
    }
    if (response.status === 204) return null;
    const data = await response.json().catch(() => null);
    if (!response.ok) {
      const message = data && data.message ? [].concat(data.message).join("; ") : response.statusText;
      throw new ApiError(response.status, message);
    }
    return data;
  }

  function api(path, options) {
    return request(API + path, options);
  }

  async function refresh() {
    const session = state.session;
    if (!session || !session.refresh) return false;
    try {
      const response = await fetch(AUTH + "/refresh", {
        method: "POST",
        headers: { "Content-Type": "application/json", Accept: "application/json" },
        body: JSON.stringify({ refresh_token: session.refresh }),
      });
      if (!response.ok) return false;
      const pair = await response.json();
      save(Object.assign({}, session, { token: pair.access_token, refresh: pair.refresh_token }));
      return true;
    } catch (_) {
      return false;
    }
  }

  // --------------------------------------------------------------- sign-in

  function renderUser() {
    const name = document.getElementById("user-name");
    const out = document.getElementById("sign-out");
    if (state.session) {
      name.textContent = state.session.name || "Signed in";
      out.hidden = false;
    } else {
      name.textContent = "Signed out";
      out.hidden = true;
    }
  }

  function signIn(problem) {
    stopTimer();
    setStatus(null);
    const content = document.getElementById("content");
    const error = h("div", { class: "message error", hidden: !problem, text: problem || "" });
    const email = h("input", { class: "input", type: "email", name: "email", autocomplete: "username", required: true });
    const password = h("input", { class: "input", type: "password", name: "password", autocomplete: "current-password", required: true });
    const code = h("input", { class: "input", name: "code", inputmode: "numeric", autocomplete: "one-time-code" });
    const codeRow = h("label", { hidden: true }, ["Two-factor code", code]);
    const token = h("input", { class: "input", name: "token", autocomplete: "off" });
    let challenge = null;

    const fail = (message) => {
      error.textContent = message;
      error.hidden = false;
    };
    const finish = (data) => {
      const user = data.user || {};
      save({ token: data.access_token, refresh: data.refresh_token, name: user.name || user.email || "Signed in" });
      route();
    };

    const form = h("form", {
      onsubmit: async (event) => {
        event.preventDefault();
        error.hidden = true;
        try {
          if (challenge) {
            const response = await fetch(AUTH + "/2fa/verify", {
              method: "POST",
              headers: { "Content-Type": "application/json", Accept: "application/json" },
              body: JSON.stringify({ challenge_token: challenge, code: code.value.trim() }),
            });
            const data = await response.json().catch(() => ({}));
            if (!response.ok) return fail([].concat(data.message || "The code was not accepted").join("; "));
            return finish(data);
          }
          const response = await fetch(LOGIN, {
            method: "POST",
            headers: { "Content-Type": "application/json", Accept: "application/json" },
            body: JSON.stringify({ email: email.value.trim(), password: password.value }),
          });
          const data = await response.json().catch(() => ({}));
          if (!response.ok) return fail([].concat(data.message || "Sign-in failed").join("; "));
          if (data.two_factor_required) {
            challenge = data.challenge_token;
            codeRow.hidden = false;
            code.focus();
            return;
          }
          finish(data);
        } catch (_) {
          fail("The sign-in route did not answer.");
        }
      },
    }, [
      h("label", {}, ["Email", email]),
      h("label", {}, ["Password", password]),
      codeRow,
      h("button", { class: "button primary", type: "submit", text: "Sign in" }),
    ]);

    const tokenForm = h("form", {
      onsubmit: (event) => {
        event.preventDefault();
        if (!token.value.trim()) return;
        save({ token: token.value.trim(), name: "Access token" });
        route();
      },
    }, [
      h("label", {}, ["Access token", token]),
      h("button", { class: "button", type: "submit", text: "Use token" }),
    ]);

    content.replaceChildren(
      h("div", { class: "card signin" }, [
        h("h1", { text: "Sign in to Fleet" }),
        h("p", { class: "muted", text: "Use your account on this app. Opening the dashboard needs the fleet.view permission." }),
        error,
        form,
        h("div", { class: "or", text: "or" }),
        tokenForm,
      ])
    );
    email.focus();
  }

  function signOut() {
    save(null);
    signIn();
  }

  // ---------------------------------------------------------------- shell

  function setStatus(status) {
    const el = document.getElementById("fleet-status");
    el.className = "status" + (status ? " " + status : "");
    el.querySelector(".status-text").textContent = status
      ? { running: "Running", paused: "Paused", inactive: "Inactive" }[status] || status
      : "Signed out";
  }

  function stamp() {
    document.getElementById("updated").textContent = "Updated " + new Date().toLocaleTimeString();
  }

  function stopTimer() {
    clearInterval(state.timer);
    state.timer = null;
  }

  function route() {
    const [view, id] = location.hash.replace(/^#/, "").split("/");
    state.view = VIEWS[view] ? view : "overview";
    state.detail = id ? decodeURIComponent(id) : null;
    for (const item of document.querySelectorAll(".nav-item")) {
      item.classList.toggle("active", item.dataset.view === state.view);
    }
    document.getElementById("crumb").textContent = VIEWS[state.view] + (state.detail ? " / " + state.detail : "");
    if (!state.session) return signIn();
    stopTimer();
    show();
    state.timer = setInterval(() => {
      if (!document.hidden && !state.busy) show(true);
    }, REFRESH_MS);
  }

  async function show(quiet) {
    state.busy = true;
    try {
      const page = await PAGES[state.view](quiet);
      if (page) {
        const content = document.getElementById("content");
        const focused = document.activeElement && document.activeElement.dataset.keep;
        content.replaceChildren(...[].concat(page));
        if (focused) {
          const again = content.querySelector('[data-keep="' + focused + '"]');
          if (again) {
            again.focus();
            if (again.setSelectionRange) again.setSelectionRange(again.value.length, again.value.length);
          }
        }
      }
      stamp();
    } catch (error) {
      if (error.status === 401) return signIn("Your session ended. Sign in again.");
      if (error.status === 403) {
        stopTimer();
        return document.getElementById("content").replaceChildren(
          h("div", { class: "message error", text: "Your account cannot open the Fleet dashboard: it needs the fleet.view permission." })
        );
      }
      if (!quiet) toast(error.message || "The dashboard could not load.", true);
    } finally {
      state.busy = false;
    }
  }

  /** Runs an operation, then shows `next` (a hash) or refreshes the page;
      true when it succeeded. */
  async function act(label, path, options, next) {
    try {
      await api(path, Object.assign({ method: "POST" }, options || {}));
      toast(label);
      if (next) location.hash = next;
      else await show(true);
      return true;
    } catch (error) {
      if (error.status === 403) toast("This needs the fleet.operate permission.", true);
      else toast(error.message || "That did not work.", true);
      return false;
    }
  }

  // ------------------------------------------------------------ components

  function table(columns, rows, onClick, rowClass) {
    if (!rows.length) return h("div", { class: "empty", text: "Nothing here." });
    return h("div", { class: "table-wrap" }, [
      h("table", {}, [
        h("thead", {}, [h("tr", {}, columns.map((c) => h("th", { class: c.num ? "num" : null, text: c.label })))]),
        h("tbody", {}, rows.map((row) =>
          h("tr", {
            class: [onClick ? "clickable" : "", rowClass ? rowClass(row) : ""].join(" ").trim() || null,
            onclick: onClick ? () => onClick(row) : null,
          }, columns.map((c) => {
            const value = c.value(row);
            return h("td", { class: [c.num ? "num" : "", c.cls || ""].join(" ").trim() || null }, [value]);
          }))
        )),
      ]),
    ]);
  }

  function figure(label, value, note) {
    return h("div", { class: "card figure" }, [
      h("div", { class: "label", text: label }),
      h("div", { class: "value", text: value }),
      note ? h("div", { class: "note", text: note }) : null,
    ]);
  }

  function searchBox(key, placeholder) {
    let pending = null;
    return h("input", {
      class: "input",
      type: "search",
      placeholder,
      value: state.search[key],
      "data-keep": "search-" + key,
      oninput: (event) => {
        state.search[key] = event.target.value;
        clearTimeout(pending);
        pending = setTimeout(() => show(true), 250);
      },
    });
  }

  function outcomeBadge(outcome) {
    const cls = { succeeded: "ok", released: "warn", failed: "bad", waiting: "", delayed: "warn", running: "ok" }[outcome];
    return h("span", { class: "badge " + (cls || ""), text: outcome });
  }

  function plot(points, first, second) {
    const width = 300;
    const height = 96;
    const el = svg("svg", { class: "plot", viewBox: "0 0 " + width + " " + height, preserveAspectRatio: "none" });
    el.append(svg("line", { class: "axis", x1: 0, y1: height - 1, x2: width, y2: height - 1 }));
    if (!points.length) return el;
    const values = points.map(first).concat(second ? points.map(second) : []);
    const max = Math.max(1e-9, ...values);
    const x = (i) => (points.length === 1 ? width / 2 : (i / (points.length - 1)) * width);
    const y = (v) => height - 2 - (v / max) * (height - 8);
    const path = (get) => points.map((p, i) => (i ? "L" : "M") + x(i).toFixed(1) + " " + y(get(p) || 0).toFixed(1)).join(" ");
    el.append(svg("path", { class: "area", d: path(first) + " L" + x(points.length - 1) + " " + height + " L" + x(0) + " " + height + " Z" }));
    el.append(svg("path", { class: "line", d: path(first) }));
    if (second) el.append(svg("path", { class: "line second", d: path(second) }));
    return el;
  }

  function chart(label, summary, points, first, second) {
    return h("div", { class: "card chart" }, [
      h("div", { class: "label" }, [h("span", { text: label }), h("span", { class: "mono", text: summary })]),
      plot(points, first, second),
    ]);
  }

  // ---------------------------------------------------------------- pages

  const PAGES = {
    async overview() {
      const o = await api("/overview");
      setStatus(o.status);
      document.getElementById("count-failed").textContent = o.failed_past_week ? number(o.failed_past_week) : "";
      const paused = o.paused.length;
      return [
        h("div", { class: "page-head" }, [
          h("h1", { text: "Overview" }),
          h("div", { class: "actions" }, [
            paused
              ? h("button", { class: "button", type: "button", text: "Continue all", onclick: () => act("Every supervisor continues.", "/continue") })
              : h("button", { class: "button", type: "button", text: "Pause all", onclick: () => act("Every supervisor is paused.", "/pause") }),
          ]),
        ]),
        h("div", { class: "figures" }, [
          figure("Status", { running: "Running", paused: "Paused", inactive: "Inactive" }[o.status], paused ? paused + " supervisor(s) paused" : null),
          figure("Jobs per minute", number(o.jobs_per_minute)),
          figure("Jobs past hour", number(o.jobs_past_hour)),
          figure("Failed past 7 days", number(o.failed_past_week)),
          figure("Longest wait", o.longest_wait ? duration(o.longest_wait.wait_secs) : "0 s", o.longest_wait ? o.longest_wait.queue : null),
        ]),
        h("div", { class: "card" }, [
          h("div", { class: "card-head" }, [h("h2", { text: "Workload" })]),
          table([
            { label: "Queue", value: (w) => w.queue },
            { label: "Pending", num: true, value: (w) => number(w.pending) },
            { label: "Processes", num: true, value: (w) => number(w.processes) },
            { label: "Oldest wait", num: true, cls: "wait", value: (w) => duration(w.oldest_wait_secs) },
            { label: "Time to clear", num: true, value: (w) => duration(w.time_to_clear_secs) },
            { label: "Jobs/min", num: true, value: (w) => number(w.jobs_per_minute) },
          ], o.workload, null, (w) => (w.over_threshold ? "behind" : "")),
        ]),
      ];
    },

    async supervisors() {
      const [{ data: masters }, overview] = await Promise.all([api("/masters"), api("/overview")]);
      setStatus(overview.status);
      const processes = masters.reduce((n, m) => n + m.supervisors.reduce((k, s) => k + s.processes.length, 0), 0);
      document.getElementById("count-processes").textContent = processes ? number(processes) : "";
      const page = [h("div", { class: "page-head" }, [h("h1", { text: "Supervisors" })])];
      if (!masters.length) {
        page.push(h("div", { class: "message", text: "No master has reported in the last 30 seconds. Start one with the fleet command." }));
      }
      for (const m of masters) {
        page.push(h("div", { class: "card" }, [
          h("div", { class: "card-head" }, [
            h("h2", { text: m.name }),
            h("span", { class: "muted mono", text: "pid " + m.pid }),
            outcomeBadge(m.status),
            h("span", { class: "muted", text: "started " + time(m.started_at) + ", last report " + ago(m.heartbeat_at) }),
          ]),
          ...m.supervisors.map((s) => h("div", { class: "card-body" }, [
            h("div", { class: "page-head" }, [
              h("h2", { text: s.name }),
              h("span", { class: "muted", text: s.queues.join(", ") + " · " + s.balance + " · up to " + s.max_processes + " processes" }),
              s.paused ? h("span", { class: "badge warn", text: "paused" }) : null,
              h("div", { class: "actions" }, [
                s.paused
                  ? h("button", { class: "button", type: "button", text: "Continue", onclick: () => act(s.name + " continues.", "/continue", { json: { supervisor: s.name } }) })
                  : h("button", { class: "button", type: "button", text: "Pause", onclick: () => act(s.name + " is paused.", "/pause", { json: { supervisor: s.name } }) }),
              ]),
            ]),
            table([
              { label: "Queue", value: (p) => p.queue },
              { label: "PID", num: true, value: (p) => (p.pid === null ? "in process" : p.pid) },
              { label: "Memory", num: true, value: (p) => (p.memory_mb === null ? "" : p.memory_mb + " MB") },
              { label: "Jobs", num: true, value: (p) => number(p.jobs) },
              { label: "Started", value: (p) => ago(p.started_at) },
            ], s.processes),
          ])),
        ]));
      }
      return page;
    },

    async jobs() {
      if (state.detail) {
        const job = await api("/jobs/" + encodeURIComponent(state.detail));
        return [
          h("div", { class: "page-head" }, [
            h("h1", { text: job.job_type }),
            outcomeBadge(job.state),
            h("div", { class: "actions" }, [h("a", { class: "button", href: "#jobs", text: "Back to jobs" })]),
          ]),
          h("div", { class: "card card-body" }, [
            h("dl", { class: "detail" }, [
              h("dt", { text: "Id" }), h("dd", { class: "mono", text: job.id }),
              h("dt", { text: "Queue" }), h("dd", { text: job.queue }),
              h("dt", { text: "Attempts" }), h("dd", { text: job.attempts + " of " + job.options.max_attempts }),
              h("dt", { text: "Available" }), h("dd", { text: time(job.available_at) }),
              h("dt", { text: "Queued" }), h("dd", { text: time(job.created_at) }),
            ]),
          ]),
          h("div", { class: "card" }, [h("div", { class: "card-head" }, [h("h2", { text: "Payload" })]), h("div", { class: "card-body" }, [h("pre", { text: JSON.stringify(job.payload, null, 2) })])]),
        ];
      }
      const query = state.search.jobs ? "?search=" + encodeURIComponent(state.search.jobs) : "";
      const data = await api("/jobs" + query);
      const tab = (name, label, count) => h("button", {
        class: "tab" + (state.jobsTab === name ? " active" : ""),
        type: "button",
        text: label + " (" + count + ")",
        onclick: () => { state.jobsTab = name; show(true); },
      });
      const list = state.jobsTab === "pending"
        ? table([
          { label: "Type", value: (j) => j.job_type },
          { label: "Queue", value: (j) => j.queue },
          { label: "State", value: (j) => outcomeBadge(j.state) },
          { label: "Attempts", num: true, value: (j) => j.attempts },
          { label: "Available", value: (j) => time(j.available_at) },
        ], data.pending, (j) => (location.hash = "#jobs/" + encodeURIComponent(j.id)))
        : table([
          { label: "Type", value: (j) => j.job_type },
          { label: "Queue", value: (j) => j.queue },
          { label: "Outcome", value: (j) => outcomeBadge(j.outcome) },
          { label: "Attempt", num: true, value: (j) => j.attempt },
          { label: "Runtime", num: true, value: (j) => millis(j.runtime_ms) },
          { label: "Finished", value: (j) => time(j.finished_at) },
        ], data.completed);
      return [
        h("div", { class: "page-head" }, [
          h("h1", { text: "Jobs" }),
          h("div", { class: "tabs" }, [tab("pending", "Pending", data.pending.length), tab("completed", "Completed", data.completed.length)]),
          h("div", { class: "actions" }, [searchBox("jobs", "Search by job type or queue")]),
        ]),
        h("div", { class: "card" }, [list]),
      ];
    },

    async failed() {
      if (state.detail) {
        const job = await api("/failed/" + encodeURIComponent(state.detail));
        return [
          h("div", { class: "page-head" }, [
            h("h1", { text: job.job_type }),
            h("span", { class: "badge bad", text: "failed" }),
            h("div", { class: "actions" }, [
              h("a", { class: "button", href: "#failed", text: "Back" }),
              h("button", { class: "button danger", type: "button", text: "Forget",
                onclick: () => act("The job is forgotten.", "/failed/" + encodeURIComponent(job.id), { method: "DELETE" }, "#failed") }),
              h("button", { class: "button primary", type: "button", text: "Retry",
                onclick: () => act("The job is back on its queue.", "/failed/" + encodeURIComponent(job.id) + "/retry", null, "#failed") }),
            ]),
          ]),
          h("div", { class: "card card-body" }, [
            h("dl", { class: "detail" }, [
              h("dt", { text: "Id" }), h("dd", { class: "mono", text: job.id }),
              h("dt", { text: "Queue" }), h("dd", { text: job.queue }),
              h("dt", { text: "Attempts" }), h("dd", { text: String(job.attempts) }),
              h("dt", { text: "Failed" }), h("dd", { text: time(job.failed_at) }),
            ]),
          ]),
          h("div", { class: "card" }, [h("div", { class: "card-head" }, [h("h2", { text: "Error" })]), h("div", { class: "card-body" }, [h("pre", { text: job.error })])]),
          h("div", { class: "card" }, [h("div", { class: "card-head" }, [h("h2", { text: "Payload" })]), h("div", { class: "card-body" }, [h("pre", { text: JSON.stringify(job.payload, null, 2) })])]),
        ];
      }
      const search = state.search.failed;
      const { data } = await api("/failed" + (search ? "?search=" + encodeURIComponent(search) : ""));
      if (!search) document.getElementById("count-failed").textContent = data.length ? number(data.length) : "";
      return [
        h("div", { class: "page-head" }, [
          h("h1", { text: "Failed jobs" }),
          h("div", { class: "actions" }, [
            searchBox("failed", "Search by type, queue, error or payload"),
            h("button", {
              class: "button", type: "button", disabled: !data.length,
              text: search ? "Retry " + data.length + " matching" : "Retry all",
              onclick: () => act("Failed jobs are back on their queues.", "/failed/retry" + (search ? "?search=" + encodeURIComponent(search) : "")),
            }),
          ]),
        ]),
        h("div", { class: "card" }, [
          table([
            { label: "Type", value: (j) => j.job_type },
            { label: "Queue", value: (j) => j.queue },
            { label: "Attempts", num: true, value: (j) => j.attempts },
            { label: "Failed", value: (j) => time(j.failed_at) },
            { label: "Error", cls: "error", value: (j) => firstLine(j.error) },
          ], data, (j) => (location.hash = "#failed/" + encodeURIComponent(j.id))),
        ]),
      ];
    },

    async metrics(quiet) {
      if (quiet && state.metricsAt && Date.now() - state.metricsAt < 30000) return null;
      const m = await api("/metrics?period=" + state.period);
      state.metricsAt = Date.now();
      const periods = [["hour", "Hour"], ["day", "Day"], ["week", "Week"]];
      const section = (title, series, waits) => [
        h("h2", { text: title }),
        series.length
          ? h("div", { class: "charts" }, series.flatMap((s) => {
            const points = s.points;
            const total = points.reduce((n, p) => n + p.processed, 0);
            const p95 = Math.max(0, ...points.map((p) => p.runtime_p95_ms));
            const charts = [
              chart(s.name + " · throughput", number(total) + " jobs", points, (p) => p.throughput_per_minute),
              chart(s.name + " · runtime avg / p95", millis(p95), points, (p) => p.runtime_avg_ms, (p) => p.runtime_p95_ms),
            ];
            if (waits) charts.push(chart(s.name + " · wait", duration(Math.max(0, ...points.map((p) => p.wait_secs || 0))), points, (p) => p.wait_secs || 0));
            return charts;
          }))
          : h("div", { class: "card empty", text: "No snapshots yet: a running master stores one every fleet.snapshot_secs." }),
      ];
      return [
        h("div", { class: "page-head" }, [
          h("h1", { text: "Metrics" }),
          h("div", { class: "tabs" }, periods.map(([value, label]) => h("button", {
            class: "tab" + (state.period === value ? " active" : ""),
            type: "button",
            text: label,
            onclick: () => { state.period = value; state.metricsAt = 0; show(); },
          }))),
        ]),
        ...section("Queues", m.queues, true),
        ...section("Job types", m.jobs, false),
      ];
    },
  };

  // ----------------------------------------------------------------- boot

  function boot() {
    const shell = document.getElementById("shell");
    try {
      if (localStorage.getItem(STORE + ".collapsed") === "1") shell.classList.add("collapsed");
    } catch (_) {
      /* no storage: start expanded */
    }
    const toggle = () => {
      if (window.matchMedia("(max-width: 760px)").matches) {
        shell.classList.toggle("open");
        return;
      }
      const collapsed = shell.classList.toggle("collapsed");
      try {
        localStorage.setItem(STORE + ".collapsed", collapsed ? "1" : "0");
      } catch (_) {
        /* ignore */
      }
    };
    document.getElementById("toggle-sidebar").addEventListener("click", toggle);
    document.addEventListener("keydown", (event) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "b") {
        event.preventDefault();
        toggle();
      }
    });
    document.getElementById("sign-out").addEventListener("click", signOut);
    for (const item of document.querySelectorAll(".nav-item")) {
      item.addEventListener("click", () => shell.classList.remove("open"));
    }
    window.addEventListener("hashchange", route);
    renderUser();
    route();
  }

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", boot);
  else boot();
})();
