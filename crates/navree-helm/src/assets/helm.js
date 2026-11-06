// Helm: small enhancements on top of pages that work without scripts.
// The sidebar (Ctrl/Cmd+B, the sheet on phones) and its counts, the account
// menu, the command menu (Ctrl/Cmd+K), filter chips, confirmations,
// select-all and the bulk-action menu, local times, time-zone offsets for
// forms, relationship pickers and Markdown previews.
(function () {
  "use strict";

  var meta = function (name) {
    var el = document.querySelector('meta[name="' + name + '"]');
    return el ? el.getAttribute("content") : "";
  };
  var base = meta("helm-base");
  var csrf = meta("csrf-token");
  var mac = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

  function ready(fn) {
    if (document.readyState !== "loading") fn();
    else document.addEventListener("DOMContentLoaded", fn);
  }

  // Ctrl+key, or Cmd+key on a Mac.
  function shortcut(event, key) {
    return (mac ? event.metaKey : event.ctrlKey) && !event.altKey && !event.shiftKey &&
      event.key && event.key.toLowerCase() === key;
  }

  function sidebar() {
    // Wide screens: the trigger and Ctrl/Cmd+B collapse the sidebar to its
    // icons, remembered in a cookie so the next page renders the same way.
    // Phones: the trigger opens it as a sheet.
    var shell = document.querySelector("[data-shell]");
    var trigger = document.querySelector("[data-sidebar-trigger]");
    if (!shell || !trigger) return;
    var panel = document.getElementById("sidebar");
    var narrow = window.matchMedia("(max-width: 767px)");
    trigger.setAttribute("role", "button");
    trigger.title = "Toggle the sidebar (" + (mac ? "⌘B" : "Ctrl+B") + ")";

    var sync = function () {
      var expanded = narrow.matches
        ? shell.classList.contains("is-open")
        : !shell.classList.contains("is-collapsed");
      trigger.setAttribute("aria-expanded", String(expanded));
    };
    var setCollapsed = function (on) {
      shell.classList.toggle("is-collapsed", on);
      var secure = location.protocol === "https:" ? "; secure" : "";
      document.cookie = "navree_helm_sidebar=" + (on ? "collapsed" : "expanded") +
        "; path=" + (base || "/") + "; max-age=31536000; samesite=strict" + secure;
      sync();
    };
    var setOpen = function (on) {
      shell.classList.toggle("is-open", on);
      if (location.hash === "#sidebar") history.replaceState(null, "", location.pathname + location.search);
      sync();
      if (on) {
        var first = panel.querySelector(".menu-item[aria-current]") || panel.querySelector(".menu-item");
        if (first) first.focus();
      }
    };
    var toggle = function () {
      if (narrow.matches) setOpen(!shell.classList.contains("is-open"));
      else setCollapsed(!shell.classList.contains("is-collapsed"));
    };

    trigger.addEventListener("click", function (event) {
      event.preventDefault();
      toggle();
    });
    document.querySelectorAll("[data-sidebar-close]").forEach(function (el) {
      el.addEventListener("click", function (event) {
        event.preventDefault();
        setOpen(false);
        trigger.focus();
      });
    });
    document.addEventListener("keydown", function (event) {
      if (shortcut(event, "b")) {
        event.preventDefault();
        toggle();
      } else if (event.key === "Escape" && shell.classList.contains("is-open")) {
        setOpen(false);
        trigger.focus();
      }
    });
    var fit = function () {
      if (!narrow.matches) shell.classList.remove("is-open");
      sync();
    };
    if (narrow.addEventListener) narrow.addEventListener("change", fit);
    sync();
  }

  function accountMenu() {
    // The account menu closes on a click elsewhere and on Escape.
    var menu = document.querySelector("details.user-menu");
    if (!menu) return;
    document.addEventListener("click", function (event) {
      if (menu.open && !menu.contains(event.target)) menu.open = false;
    });
    menu.addEventListener("keydown", function (event) {
      if (event.key === "Escape" && menu.open) {
        menu.open = false;
        menu.querySelector("summary").focus();
      }
    });
  }

  function commandMenu() {
    // Ctrl/Cmd+K: go to a resource or page of the menu, or search the
    // current list.
    var dialog = document.querySelector("dialog[data-command]");
    var open = document.querySelector("[data-command-open]");
    if (!dialog || !open || typeof dialog.showModal !== "function") return;
    var input = dialog.querySelector("[data-command-input]");
    var list = dialog.querySelector("[data-command-list]");
    var empty = dialog.querySelector("[data-command-empty]");
    open.hidden = false;
    var kbd = open.querySelector("[data-shortcut]");
    if (kbd) kbd.textContent = mac ? "⌘K" : "Ctrl K";
    input.setAttribute("role", "combobox");
    input.setAttribute("aria-controls", "command-list");
    input.setAttribute("aria-expanded", "true");
    list.id = "command-list";

    var entries = [];
    document.querySelectorAll("#sidebar .menu-item").forEach(function (link) {
      var group = link.closest(".sidebar-group");
      var heading = group && group.querySelector(".group-label");
      var label = link.querySelector(".menu-label");
      entries.push({
        label: label ? label.textContent.trim() : link.textContent.trim(),
        group: heading ? heading.textContent.trim() : "",
        url: link.getAttribute("href"),
        icon: link.querySelector("svg")
      });
    });
    var searchForm = document.querySelector("form.filter-bar");
    var searchInput = searchForm && searchForm.querySelector('input[name="search"]');
    var heading = document.querySelector("main h1");

    var items = [];
    var selected = 0;
    var select = function (index) {
      items.forEach(function (item, i) {
        item.li.setAttribute("aria-selected", String(i === index));
      });
      selected = index;
      if (items[index]) {
        input.setAttribute("aria-activedescendant", items[index].li.id);
        items[index].li.scrollIntoView({ block: "nearest" });
      } else {
        input.removeAttribute("aria-activedescendant");
      }
    };
    var add = function (label, detail, icon, run) {
      var li = document.createElement("li");
      li.className = "command-item";
      li.setAttribute("role", "option");
      li.id = "command-" + items.length;
      var a = document.createElement("a");
      a.href = run.url || "#";
      a.tabIndex = -1;
      if (icon) a.appendChild(icon.cloneNode(true));
      var text = document.createElement("span");
      text.textContent = label;
      a.appendChild(text);
      if (detail) {
        var small = document.createElement("small");
        small.textContent = detail;
        a.appendChild(small);
      }
      a.addEventListener("click", function (event) {
        if (run.action) {
          event.preventDefault();
          run.action();
        }
      });
      li.appendChild(a);
      list.appendChild(li);
      items.push({ li: li, link: a });
    };
    var render = function () {
      var query = input.value.trim().toLowerCase();
      list.innerHTML = "";
      items = [];
      if (query && searchInput) {
        add("Search " + (heading ? heading.textContent.trim() : "this list") + " for “" + input.value.trim() + "”",
          "", null, {
            action: function () {
              searchInput.value = input.value.trim();
              dialog.close();
              searchForm.submit();
            }
          });
      }
      entries.forEach(function (entry) {
        var haystack = (entry.label + " " + entry.group).toLowerCase();
        if (!query || haystack.indexOf(query) !== -1) add(entry.label, entry.group, entry.icon, { url: entry.url });
      });
      empty.hidden = items.length > 0;
      select(items.length ? 0 : -1);
    };
    var show = function () {
      input.value = "";
      render();
      dialog.showModal();
      input.focus();
    };

    open.addEventListener("click", show);
    document.addEventListener("keydown", function (event) {
      if (shortcut(event, "k")) {
        event.preventDefault();
        if (dialog.open) dialog.close();
        else show();
      }
    });
    input.addEventListener("input", render);
    input.addEventListener("keydown", function (event) {
      if (event.key === "ArrowDown" && items.length) {
        event.preventDefault();
        select((selected + 1) % items.length);
      } else if (event.key === "ArrowUp" && items.length) {
        event.preventDefault();
        select((selected - 1 + items.length) % items.length);
      } else if (event.key === "Enter" && items[selected]) {
        event.preventDefault();
        items[selected].link.click();
      }
    });
    dialog.addEventListener("click", function (event) {
      if (event.target === dialog) dialog.close();
    });
  }

  function filterChips() {
    // A filter chip opens its panel of choices; one is open at a time, and a
    // click elsewhere or Escape closes it.
    var chips = document.querySelectorAll("details[data-chip]");
    if (!chips.length) return;
    document.addEventListener("click", function (event) {
      chips.forEach(function (chip) {
        if (chip.open && !chip.contains(event.target)) chip.open = false;
      });
    });
    chips.forEach(function (chip) {
      // A range typed in and a preset chosen would both be sent, and only one
      // can count: choosing one clears the other.
      var range = chip.querySelector("input.range-input");
      if (range) {
        var presets = chip.querySelectorAll("input[type=radio]");
        presets.forEach(function (preset) {
          preset.addEventListener("change", function () { range.value = ""; });
        });
        range.addEventListener("input", function () {
          if (range.value) presets.forEach(function (preset) { preset.checked = false; });
        });
      }
      chip.addEventListener("toggle", function () {
        if (!chip.open) return;
        chips.forEach(function (other) {
          if (other !== chip) other.open = false;
        });
      });
      chip.addEventListener("keydown", function (event) {
        if (event.key === "Escape" && chip.open) {
          chip.open = false;
          chip.querySelector("summary").focus();
        }
      });
    });
  }

  function sidebarCounts() {
    // The numbers beside the menu are asked for once the page is up, so a
    // page never waits for them; a count that is not there leaves its slot empty.
    var slots = document.querySelectorAll("[data-count]");
    if (!slots.length || !window.fetch) return;
    fetch(base + "/counts", { headers: { Accept: "application/json" }, credentials: "same-origin" })
      .then(function (r) { return r.ok ? r.json() : null; })
      .then(function (body) {
        if (!body || !body.counts) return;
        slots.forEach(function (slot) {
          var count = body.counts[slot.getAttribute("data-count")];
          if (!count) return;
          slot.textContent = count.text;
          if (count.capped) slot.title = "More than " + count.value;
          slot.hidden = false;
        });
      })
      .catch(function () {});
  }

  ready(function () {
    sidebar();
    accountMenu();
    commandMenu();
    filterChips();
    sidebarCounts();

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

    // Select every row of a table, count the ticked ones and run the bulk
    // action the menu holds on them (Run waits for both).
    document.querySelectorAll("input[data-select-all]").forEach(function (all) {
      var table = all.closest("table");
      var form = all.closest("form");
      var count = document.querySelector("[data-selected-count]");
      var menu = document.getElementById("bulk-action");
      var run = document.querySelector("[data-bulk-run]");
      var boxes = function () {
        return table.querySelectorAll('tbody input[type="checkbox"][name="ids"]');
      };
      var update = function () {
        var n = Array.prototype.filter.call(boxes(), function (b) { return b.checked; }).length;
        if (count) count.textContent = n ? n + " selected" : "";
        all.checked = n > 0 && n === boxes().length;
        all.indeterminate = n > 0 && n < boxes().length;
        if (run && menu) {
          var option = menu.options[menu.selectedIndex];
          var danger = !!option && option.hasAttribute("data-destructive");
          run.disabled = !(n > 0 && menu.value);
          run.classList.toggle("btn-danger", danger);
          run.classList.toggle("btn-secondary", !danger);
        }
      };
      all.addEventListener("change", function () {
        boxes().forEach(function (b) { b.checked = all.checked; });
        update();
      });
      table.addEventListener("change", function (e) {
        if (e.target.name === "ids") update();
      });
      if (menu) menu.addEventListener("change", update);
      if (run && menu) {
        run.addEventListener("click", function (event) {
          var option = menu.options[menu.selectedIndex];
          var question = option && option.getAttribute("data-confirm");
          if (!question) return;
          if (!window.confirm(question)) {
            event.preventDefault();
            return;
          }
          if (form && !form.querySelector('input[name="_confirmed"]')) {
            var input = document.createElement("input");
            input.type = "hidden";
            input.name = "_confirmed";
            input.value = "1";
            form.appendChild(input);
          }
        });
      }
      update();
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

    // A form with files is sent by the script, so that the session's CSRF
    // token goes in a header (a plain multipart post has no place for it that
    // the session layer reads). The page that answers replaces this one.
    document.querySelectorAll('form[enctype="multipart/form-data"]').forEach(function (form) {
      form.addEventListener("submit", function (event) {
        if (!window.fetch || !window.FormData) return;
        event.preventDefault();
        var data = new FormData(form);
        if (event.submitter && event.submitter.name) data.append(event.submitter.name, event.submitter.value);
        fetch(form.getAttribute("action") || location.href, {
          method: "POST",
          credentials: "same-origin",
          headers: { Accept: "text/html", "X-CSRF-Token": csrf },
          body: data
        })
          .then(function (r) {
            return r.text().then(function (html) {
              // The answer is one of the panel's own pages, rendered by the server.
              // nosemgrep: navree-raw-html
              document.open();
              // nosemgrep: navree-raw-html
              document.write(html);
              document.close();
              if (r.url) history.replaceState(null, "", r.url);
            });
          })
          .catch(function () { form.submit(); });
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
            // The server renders Markdown with raw HTML shown as text and only safe link schemes
            // (navree-helm markdown.rs), so this is not user-controlled markup.
            // nosemgrep: navree-raw-html
            preview.innerHTML = body.html;
            preview.hidden = false;
            area.hidden = true;
            button.textContent = "Edit";
          });
      });
    });
  });
})();
