// Logbook UI: tabs, the waterfall, live arrivals, clearing, replay and curl.
(function () {
  "use strict";
  document.documentElement.classList.add("js");
  var base = document.body.getAttribute("data-base") || "";

  function xsrfHeaders(headers) {
    var match = document.cookie.match(/(?:^|;\s*)XSRF-TOKEN=([^;]+)/);
    if (match) headers["X-XSRF-TOKEN"] = decodeURIComponent(match[1]);
    return headers;
  }

  // Tabs: one panel at a time, chosen by the hash or the first tab.
  var tabs = Array.prototype.slice.call(document.querySelectorAll("[data-tab]"));
  function show(id) {
    var found = false;
    tabs.forEach(function (tab) {
      var on = tab.getAttribute("data-tab") === id;
      tab.classList.toggle("on", on);
      var panel = document.getElementById(tab.getAttribute("data-tab"));
      if (panel) panel.classList.toggle("on", on);
      found = found || on;
    });
    return found;
  }
  if (tabs.length) {
    var wanted = location.hash.replace(/^#/, "");
    if (!wanted || !show(wanted)) show(tabs[0].getAttribute("data-tab"));
    tabs.forEach(function (tab) {
      tab.addEventListener("click", function (event) {
        event.preventDefault();
        var id = tab.getAttribute("data-tab");
        show(id);
        history.replaceState(null, "", "#" + id);
      });
    });
  }

  // The waterfall: bar offsets and widths come as data attributes.
  Array.prototype.forEach.call(document.querySelectorAll(".bar[data-left]"), function (bar) {
    bar.style.setProperty("--left", bar.getAttribute("data-left") + "%");
    bar.style.setProperty("--width", bar.getAttribute("data-width") + "%");
  });

  // Live arrivals on the list.
  var rows = document.getElementById("rows");
  if (rows && rows.getAttribute("data-live")) {
    var poll = function () {
      var url = rows.getAttribute("data-live") + "&after=" + encodeURIComponent(rows.getAttribute("data-after") || "0");
      fetch(url, { headers: { Accept: "text/html" }, credentials: "same-origin" })
        .then(function (response) {
          var latest = response.headers.get("X-Logbook-Latest");
          return response.text().then(function (html) {
            if (html.trim()) {
              // Rows rendered and escaped by this logbook's own endpoint (same origin), not user markup.
              // nosemgrep: navree-raw-html
              rows.insertAdjacentHTML("afterbegin", html);
              var empty = document.querySelector(".empty");
              if (empty) empty.remove();
            }
            if (latest) rows.setAttribute("data-after", latest);
          });
        })
        .catch(function () {})
        .then(function () { setTimeout(poll, 2000); });
    };
    setTimeout(poll, 2000);
  }

  // Clear every entry.
  var clear = document.querySelector("[data-clear]");
  if (clear) {
    clear.addEventListener("click", function () {
      if (!confirm("Delete every Logbook entry?")) return;
      fetch(clear.getAttribute("data-clear"), {
        method: "DELETE",
        credentials: "same-origin",
        headers: xsrfHeaders({ Accept: "application/json" })
      }).then(function () { location.href = base; });
    });
  }

  // Replay and Copy as curl.
  var form = document.querySelector("form.replay");
  if (form) {
    var output = form.querySelector(".result");
    form.addEventListener("submit", function (event) {
      event.preventDefault();
      var method = form.elements.method.value.trim().toUpperCase();
      var unsafe = ["GET", "HEAD", "OPTIONS", "TRACE"].indexOf(method) < 0;
      if (unsafe && !confirm("Replaying a " + method + " changes data again. Send it?")) return;
      var headers = form.elements.headers.value.split("\n").map(function (line) {
        var at = line.indexOf(":");
        return at > 0 ? [line.slice(0, at).trim(), line.slice(at + 1).trim()] : null;
      }).filter(Boolean);
      var asUser = form.elements.as_user;
      var body = {
        method: method,
        path: form.elements.path.value.trim(),
        headers: headers,
        body: form.elements.body.value,
        as_guest: asUser ? !asUser.checked : false,
        confirm: unsafe
      };
      output.textContent = "Sending…";
      fetch(form.getAttribute("data-replay"), {
        method: "POST",
        credentials: "same-origin",
        headers: xsrfHeaders({ "Content-Type": "application/json", Accept: "application/json" }),
        body: JSON.stringify(body)
      }).then(function (response) {
        return response.json().then(function (json) { return { ok: response.ok, json: json }; });
      }).then(function (result) {
        output.textContent = "";
        if (!result.ok) {
          output.textContent = result.json.message || "The replay failed.";
          return;
        }
        var link = document.createElement("a");
        link.href = base + "/entries/" + encodeURIComponent(result.json.entry_id || "");
        link.textContent = "Replayed: " + result.json.status + " (open the new batch)";
        var pre = document.createElement("pre");
        pre.textContent = result.json.body;
        output.appendChild(link);
        output.appendChild(pre);
      }).catch(function (error) { output.textContent = String(error); });
    });
    var copy = form.querySelector("[data-copy-curl]");
    if (copy) {
      copy.addEventListener("click", function () {
        fetch(form.getAttribute("data-curl"), { credentials: "same-origin" })
          .then(function (response) { return response.text(); })
          .then(function (text) {
            var pre = document.createElement("pre");
            pre.textContent = text;
            output.textContent = "";
            output.appendChild(pre);
            if (navigator.clipboard) navigator.clipboard.writeText(text).catch(function () {});
          });
      });
    }
  }
})();
