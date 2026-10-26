// Helm: small enhancements on top of pages that work without scripts.
// Confirmations, select-all, local times, time-zone offsets for forms,
// relationship pickers and Markdown previews.
(function () {
  "use strict";

  var meta = function (name) {
    var el = document.querySelector('meta[name="' + name + '"]');
    return el ? el.getAttribute("content") : "";
  };
  var base = meta("helm-base");
  var csrf = meta("csrf-token");

  function ready(fn) {
    if (document.readyState !== "loading") fn();
    else document.addEventListener("DOMContentLoaded", fn);
  }

  ready(function () {
    // On phones the menu starts closed (it is open without this script);
    // wider screens always show it.
    var menu = document.querySelector("details.menu");
    if (menu && window.matchMedia) {
      var narrow = window.matchMedia("(max-width: 760px)");
      var fit = function () { menu.open = !narrow.matches; };
      fit();
      if (narrow.addEventListener) narrow.addEventListener("change", fit);
    }

    // Ask before actions that say so; the server asks itself when this
    // script is not running.
    document.addEventListener("click", function (event) {
      var button = event.target.closest("button[data-confirm]");
      if (!button) return;
      if (!window.confirm(button.getAttribute("data-confirm"))) {
        event.preventDefault();
        return;
      }
      var form = button.form;
      if (form && !form.querySelector('input[name="_confirmed"]')) {
        var input = document.createElement("input");
        input.type = "hidden";
        input.name = "_confirmed";
        input.value = "1";
        form.appendChild(input);
      }
    });

    // Select every row of a table.
    document.querySelectorAll("input[data-select-all]").forEach(function (all) {
      var table = all.closest("table");
      var boxes = function () {
        return table.querySelectorAll('tbody input[type="checkbox"][name="ids"]');
      };
      var count = all.closest("form").querySelector("[data-selected-count]");
      var update = function () {
        var n = Array.prototype.filter.call(boxes(), function (b) { return b.checked; }).length;
        if (count) count.textContent = n ? n + " selected" : "Select rows to run an action";
        all.checked = n > 0 && n === boxes().length;
        all.indeterminate = n > 0 && n < boxes().length;
      };
      all.addEventListener("change", function () {
        boxes().forEach(function (b) { b.checked = all.checked; });
        update();
      });
      table.addEventListener("change", function (e) {
        if (e.target.name === "ids") update();
      });
    });

    // Date-times in the viewer's time zone.
    document.querySelectorAll("time[data-local]").forEach(function (el) {
      var d = new Date(el.getAttribute("datetime"));
      if (!isNaN(d)) {
        el.textContent = d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
        el.title = el.getAttribute("datetime");
      }
    });

    // Forms send the viewer's offset from UTC; date-time inputs show local
    // time (the server stores UTC).
    var offset = new Date().getTimezoneOffset();
    document.querySelectorAll("input[data-tz-offset]").forEach(function (el) {
      el.value = String(offset);
    });
    document.querySelectorAll("input[data-utc]").forEach(function (el) {
      if (!el.value) return;
      var d = new Date(el.value + "Z");
      if (isNaN(d)) return;
      var local = new Date(d.getTime() - d.getTimezoneOffset() * 60000);
      el.value = local.toISOString().slice(0, 16);
    });

    // Relationship pickers: search the records the user may read.
    document.querySelectorAll("input[data-picker]").forEach(function (search) {
      var select = document.getElementById(search.getAttribute("data-picker"));
      var source = search.getAttribute("data-source");
      var timer = null;
      search.addEventListener("input", function () {
        clearTimeout(timer);
        timer = setTimeout(function () {
          var url = base + "/api/resources/" + encodeURIComponent(source) + "/options?search=" + encodeURIComponent(search.value);
          fetch(url, { headers: { Accept: "application/json" }, credentials: "same-origin" })
            .then(function (r) { return r.ok ? r.json() : { data: [] }; })
            .then(function (body) {
              var current = select.value;
              var keep = select.querySelector('option[value="' + CSS.escape(current) + '"]');
              select.innerHTML = "";
              var blank = document.createElement("option");
              blank.value = "";
              blank.textContent = "—";
              select.appendChild(blank);
              if (keep && current) select.appendChild(keep);
              body.data.forEach(function (row) {
                if (row.id === current) return;
                var option = document.createElement("option");
                option.value = row.id;
                option.textContent = row.title;
                select.appendChild(option);
              });
              select.value = current;
            });
        }, 200);
      });
    });

    // Markdown previews, rendered by the server without raw HTML.
    document.querySelectorAll("button[data-preview]").forEach(function (button) {
      var area = document.getElementById(button.getAttribute("data-preview"));
      var preview = document.getElementById(area.id.replace(/^f-/, "p-"));
      button.addEventListener("click", function () {
        if (!preview.hidden) {
          preview.hidden = true;
          area.hidden = false;
          button.textContent = "Preview";
          return;
        }
        fetch(base + "/api/markdown", {
          method: "POST",
          credentials: "same-origin",
          headers: { "Content-Type": "application/json", Accept: "application/json", "X-CSRF-Token": csrf },
          body: JSON.stringify({ text: area.value })
        })
          .then(function (r) { return r.ok ? r.json() : { html: "" }; })
          .then(function (body) {
            preview.innerHTML = body.html;
            preview.hidden = false;
            area.hidden = true;
            button.textContent = "Edit";
          });
      });
    });
  });
})();
