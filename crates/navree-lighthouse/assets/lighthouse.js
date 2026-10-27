// Lighthouse dashboard: issue actions, sign-out, the sidebar toggle and the
// overview's live updates. No dependencies; served from the app itself.
(function () {
  "use strict";
  var body = document.body;
  var mount = body.getAttribute("data-mount") || "";
  var csrf = body.getAttribute("data-csrf") || "";

  function post(path, payload) {
    return fetch(mount + path, {
      method: "POST",
      credentials: "same-origin",
      headers: { "content-type": "application/json", "x-csrf-token": csrf, accept: "application/json" },
      body: JSON.stringify(payload || {})
    }).then(function (response) {
      if (!response.ok) {
        return response.json().catch(function () { return {}; }).then(function (error) {
          throw new Error(error.message || ("The request failed (" + response.status + ")."));
        });
      }
      return response;
    });
  }

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

  // Overview: refresh the figures and slide new occurrences into the feed.
  if (body.getAttribute("data-page") === "overview") {
    var period = body.getAttribute("data-period") || "24h";
    var feed = document.getElementById("feed");
    var seen = {};
    if (feed) {
      Array.prototype.forEach.call(feed.querySelectorAll("li[data-id]"), function (li) {
        seen[li.getAttribute("data-id")] = true;
      });
    }
    function set(id, text) {
      var element = document.getElementById(id);
      if (element) element.textContent = text;
    }
    function refresh() {
      fetch(mount + "/api/overview?period=" + encodeURIComponent(period), {
        credentials: "same-origin",
        headers: { accept: "application/json" }
      }).then(function (response) { return response.ok ? response.json() : null; }).then(function (data) {
        if (!data) return;
        set("fig-rpm", data.requests_per_minute.toFixed(1));
        set("fig-error-rate", (data.error_rate * 100).toFixed(1) + "%");
        set("fig-p95", Math.round(data.p95_ms) + " ms");
        set("fig-failed-jobs", String(data.failed_jobs));
        set("fig-open-issues", String(data.open_issues));
        if (!feed) return;
        var empty = feed.querySelector(".empty");
        data.feed.slice().reverse().forEach(function (item) {
          if (seen[item.occurrence_id]) return;
          seen[item.occurrence_id] = true;
          if (empty) { empty.remove(); empty = null; }
          var li = document.createElement("li");
          li.className = "new";
          li.setAttribute("data-id", item.occurrence_id);
          var dot = document.createElement("span");
          dot.className = "dot " + item.level;
          var link = document.createElement("a");
          link.href = mount + "/issues/" + item.issue_id;
          link.textContent = item.title;
          var where = document.createElement("span");
          where.className = "sub";
          where.textContent = item.culprit;
          var when = document.createElement("span");
          when.className = "when";
          when.textContent = new Date(item.at).toLocaleTimeString();
          li.appendChild(dot); li.appendChild(link); li.appendChild(where); li.appendChild(when);
          feed.insertBefore(li, feed.firstChild);
          while (feed.children.length > 20) feed.removeChild(feed.lastChild);
        });
      }).catch(function () { /* try again next time */ });
    }
    setInterval(refresh, 10000);
  }
})();
