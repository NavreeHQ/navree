// Lighthouse dashboard: issue actions, alert channel and settings changes,
// sign-out, the sidebar toggle and the overview's live updates. No
// dependencies; served from the app itself.
(function () {
  "use strict";
  var body = document.body;
  var mount = body.getAttribute("data-mount") || "";
  var csrf = body.getAttribute("data-csrf") || "";

  function send(method, path, payload) {
    var options = {
      method: method,
      credentials: "same-origin",
      headers: { "x-csrf-token": csrf, accept: "application/json" }
    };
    if (payload !== undefined) {
      options.headers["content-type"] = "application/json";
      options.body = JSON.stringify(payload);
    }
    return fetch(mount + path, options).then(function (response) {
      if (!response.ok) {
        return response.json().catch(function () { return {}; }).then(function (error) {
          throw new Error(error.detail || error.message || ("The request failed (" + response.status + ")."));
        });
      }
      return response;
    });
  }
  function post(path, payload) { return send("POST", path, payload || {}); }

  // Sidebar: Ctrl/Cmd+B toggles, the state persists.
  try {
    if (localStorage.getItem("lighthouse.sidebar") === "collapsed") body.classList.add("collapsed");
  } catch (e) { /* storage may be unavailable */ }
  function toggleSidebar() {
    if (window.matchMedia("(max-width: 900px)").matches) {
      body.classList.toggle("sidebar-open");
      return;
    }
    body.classList.toggle("collapsed");
    try {
      localStorage.setItem("lighthouse.sidebar", body.classList.contains("collapsed") ? "collapsed" : "expanded");
    } catch (e) { /* ignore */ }
  }
  document.addEventListener("keydown", function (event) {
    if ((event.ctrlKey || event.metaKey) && (event.key === "b" || event.key === "B")) {
      event.preventDefault();
      toggleSidebar();
    }
  });

  // Where a button reports what happened: the nearest [data-result], else
  // the page's own.
  function reporter(target) {
    var scope = target.closest("[data-channel], .actions, .card") || document;
    var box = scope.querySelector("[data-result]") || document.getElementById("settings-result") || document.getElementById("action-error");
    return function (text, bad) {
      if (!box) return;
      box.textContent = text;
      box.className = "small" + (bad ? " error-text" : "");
    };
  }

  function channelAction(target, action) {
    var box = target.closest("[data-channel]");
    var name = target.getAttribute("data-channel");
    var say = reporter(target);
    var path = "/api/alerts/channels/" + encodeURIComponent(name);
    var request;
    if (action === "mute") {
      var minutes = box.querySelector(".mute-for").value;
      request = send("PUT", path, minutes ? { muted: true, minutes: Number(minutes) } : { muted: true });
    } else if (action === "unmute") {
      request = send("PUT", path, { muted: false });
    } else if (action === "reset-kinds") {
      request = send("PUT", path, { on: null });
    } else if (action === "save-kinds") {
      var kinds = Array.prototype.map.call(box.querySelectorAll(".kinds input:checked"), function (input) { return input.value; });
      if (!kinds.length) {
        say("Pick at least one kind, or mute the channel.", true);
        return;
      }
      request = send("PUT", path, { on: kinds });
    } else {
      say("Sending…", false);
      target.disabled = true;
      post(path + "/test").then(function (response) { return response.json(); }).then(function (result) {
        target.disabled = false;
        say(result.ok ? "Delivered." : "Failed: " + (result.error || "no answer"), !result.ok);
      }).catch(function (error) {
        target.disabled = false;
        say(error.message, true);
      });
      return;
    }
    target.disabled = true;
    request.then(function () { window.location.reload(); }).catch(function (error) {
      target.disabled = false;
      say(error.message, true);
    });
  }

  function settingsAction(target, action) {
    var say = reporter(target);
    var values = {};
    if (action === "reset-setting") {
      values[target.getAttribute("data-key")] = null;
    } else {
      Array.prototype.forEach.call(document.querySelectorAll("input[data-key]"), function (input) {
        if (input.value !== input.defaultValue) {
          values[input.getAttribute("data-key")] = input.value === "" ? null : Number(input.value);
        }
      });
      if (!Object.keys(values).length) {
        say("Nothing changed.", false);
        return;
      }
    }
    target.disabled = true;
    send("PUT", "/api/settings", { values: values }).then(function () {
      window.location.reload();
    }).catch(function (error) {
      target.disabled = false;
      say(error.message, true);
    });
  }

  document.addEventListener("click", function (event) {
    var target = event.target.closest("[data-action]");
    if (!target) return;
    var action = target.getAttribute("data-action");
    if (action === "toggle-sidebar") {
      toggleSidebar();
      return;
    }
    if (action === "logout") {
      event.preventDefault();
      post("/logout").then(function () { window.location.assign(mount + "/login"); })
        .catch(function () { window.location.assign(mount + "/login"); });
      return;
    }
    if (["mute", "unmute", "save-kinds", "reset-kinds", "test-channel"].indexOf(action) >= 0) {
      event.preventDefault();
      channelAction(target, action);
      return;
    }
    if (action === "save-settings" || action === "reset-setting") {
      event.preventDefault();
      settingsAction(target, action);
      return;
    }
    var issue = target.getAttribute("data-issue");
    if (!issue) return;
    event.preventDefault();
    var comment = document.getElementById("comment");
    var text = comment ? comment.value : "";
    var request;
    if (action === "resolve") request = post("/api/issues/" + issue + "/resolve", { comment: text });
    else if (action === "resolve-next") request = post("/api/issues/" + issue + "/resolve", { in_next_release: true, comment: text });
    else if (action === "reopen") request = post("/api/issues/" + issue + "/reopen");
    else if (action === "ignore") {
      var choice = document.getElementById("ignore-for");
      var value = choice ? choice.value : "forever";
      var payload = { comment: text };
      if (value.indexOf("hours:") === 0) {
        payload.until = new Date(Date.now() + Number(value.slice(6)) * 3600 * 1000).toISOString();
      } else if (value.indexOf("occurrences:") === 0) {
        payload.occurrences = Number(value.slice(12));
      }
      request = post("/api/issues/" + issue + "/ignore", payload);
    }
    if (!request) return;
    target.disabled = true;
    request.then(function () { window.location.reload(); }).catch(function (error) {
      target.disabled = false;
      var box = document.getElementById("action-error");
      if (box) box.textContent = error.message;
    });
  });

  // Overview: refresh the figures every ten seconds and slide new
  // occurrences into the live panel.
  if (body.getAttribute("data-page") === "overview") {
    var period = body.getAttribute("data-period") || "24h";
    var feed = document.getElementById("feed");
    var seen = {};
    function ageText(iso) {
      var s = Math.max(0, Math.round((Date.now() - new Date(iso).getTime()) / 1000));
      if (s < 5) return "now";
      if (s < 60) return s + "s";
      if (s < 3600) return Math.floor(s / 60) + " min";
      if (s < 86400) return Math.floor(s / 3600) + " h";
      return Math.floor(s / 86400) + " d";
    }
    function kindLabel(kind) {
      var text = String(kind).replace(/_/g, " ");
      return text.charAt(0).toUpperCase() + text.slice(1);
    }
    function set(id, text) {
      var element = document.getElementById(id);
      if (element) element.textContent = text;
    }
    // A sparkline path, as the server draws it: 80 x 24, the largest value
    // at the top.
    function sparkPath(values) {
      if (!values || values.length < 2) return "";
      var max = Math.max.apply(null, values.concat([0]));
      var step = 80 / (values.length - 1);
      return values.map(function (value, i) {
        var y = max > 0 ? 24 - Math.min(24, Math.max(0, value / max * 24)) : 24;
        y = Math.min(23, Math.max(1, y));
        return (i === 0 ? "M" : "L") + (Math.round(i * step * 100) / 100) + " " + (Math.round(y * 100) / 100);
      }).join(" ");
    }
    function spark(id, values) {
      var path = document.getElementById(id);
      var d = sparkPath(values);
      if (path && d) path.setAttribute("d", d);
    }
    function status(text) {
      set("live-text", text === "Streaming" ? "Live" : text);
      set("stream-text", text);
    }
    function tickAges() {
      if (!feed) return;
      Array.prototype.forEach.call(feed.querySelectorAll("li[data-at]"), function (li) {
        var age = li.querySelector(".age");
        if (age) age.textContent = ageText(li.getAttribute("data-at"));
      });
    }
    function itemNode(item) {
      var li = document.createElement("li");
      li.className = "new " + item.level;
      li.setAttribute("data-id", item.occurrence_id);
      li.setAttribute("data-at", item.at);
      var top = document.createElement("div");
      top.className = "live-top";
      var kind = document.createElement("span");
      kind.className = "kind " + item.level;
      kind.appendChild(document.createElement("i"));
      kind.appendChild(document.createTextNode(kindLabel(item.kind)));
      var age = document.createElement("span");
      age.className = "age";
      age.textContent = ageText(item.at);
      top.appendChild(kind);
      top.appendChild(age);
      var link = document.createElement("a");
      link.className = "mono live-title";
      link.href = mount + "/issues/" + item.issue_id;
      link.textContent = item.title;
      var where = document.createElement("div");
      where.className = "sub mono live-where";
      where.textContent = item.culprit;
      li.appendChild(top);
      li.appendChild(link);
      li.appendChild(where);
      return li;
    }
    if (feed) {
      Array.prototype.forEach.call(feed.querySelectorAll("li[data-id]"), function (li) {
        seen[li.getAttribute("data-id")] = true;
      });
    }
    function refresh() {
      if (document.hidden) { status("Paused"); return; }
      fetch(mount + "/api/overview?period=" + encodeURIComponent(period), {
        credentials: "same-origin",
        headers: { accept: "application/json" }
      }).then(function (response) { return response.ok ? response.json() : null; }).then(function (data) {
        if (!data) { status("Reconnecting"); return; }
        status("Streaming");
        set("fig-rpm", data.requests_per_minute.toFixed(1));
        set("fig-error-rate", (data.error_rate * 100).toFixed(1) + "%");
        set("fig-p95", Math.round(data.p95_ms) + " ms");
        set("fig-p50", String(Math.round(data.p50_ms)));
        set("fig-failed-jobs", String(data.failed_jobs));
        set("fig-open-issues", String(data.open_issues));
        set("fig-new-issues", String(data.new_issues));
        set("fig-sampled-out", String(data.sampled_out));
        if (data.series) {
          spark("spark-requests", data.series.requests_per_minute);
          spark("spark-errors", data.series.error_rate);
          spark("spark-p95", data.series.p95_ms);
          spark("spark-failed", data.series.failed_jobs);
        }
        if (!feed) return;
        var empty = feed.querySelector(".empty");
        data.feed.slice().reverse().forEach(function (item) {
          if (seen[item.occurrence_id]) return;
          seen[item.occurrence_id] = true;
          if (empty) { empty.remove(); empty = null; }
          feed.insertBefore(itemNode(item), feed.firstChild);
          while (feed.children.length > 20) feed.removeChild(feed.lastChild);
        });
        tickAges();
      }).catch(function () { status("Reconnecting"); });
    }
    setInterval(refresh, 10000);
    setInterval(tickAges, 5000);
    document.addEventListener("visibilitychange", function () {
      if (document.hidden) status("Paused"); else refresh();
    });
  }
})();
