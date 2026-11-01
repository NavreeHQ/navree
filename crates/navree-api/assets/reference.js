// The API reference page: search, the contents menu on small screens, and the
// "Try it" form. The page is complete to read without this script. Nothing here
// builds HTML from text: elements are created and text goes in as text.
(function () {
  "use strict";

  document.documentElement.classList.add("js");

  var searchForm = document.querySelector(".search");
  var input = document.getElementById("search");
  var count = document.getElementById("search-count");
  var nav = document.getElementById("nav");
  var menu = document.getElementById("menu");
  var articles = Array.prototype.slice.call(
    document.querySelectorAll("article[data-search]")
  );
  var sections = Array.prototype.slice.call(document.querySelectorAll("main .group"));
  var navLinks = Array.prototype.slice.call(nav.querySelectorAll("a[data-target]"));
  var linkByTarget = {};
  navLinks.forEach(function (link) {
    linkByTarget[link.dataset.target] = link;
  });

  // Search: every word must appear in the operation or the schema.
  function applySearch() {
    var words = input.value.toLowerCase().split(/\s+/).filter(Boolean);
    var shown = 0;
    articles.forEach(function (article) {
      var text = article.dataset.search;
      var match = words.every(function (word) {
        return text.indexOf(word) !== -1;
      });
      article.hidden = !match;
      var link = linkByTarget[article.id];
      if (link) {
        link.parentElement.hidden = !match;
      }
      if (match) {
        shown += 1;
      }
    });
    sections.forEach(function (section) {
      section.hidden = !section.querySelector("article:not([hidden])");
    });
    Array.prototype.forEach.call(nav.querySelectorAll("details"), function (group) {
      var visible = group.querySelector("li:not([hidden])");
      group.hidden = !visible;
      if (words.length > 0 && visible) {
        group.open = true;
      }
    });
    count.textContent = words.length > 0 ? shown + " of " + articles.length : "";
  }

  function visibleLinks() {
    return navLinks.filter(function (link) {
      return link.parentElement && !link.parentElement.hidden && link.offsetParent !== null;
    });
  }

  function jumpTo(link) {
    var target = document.getElementById(link.dataset.target);
    if (!target) {
      return;
    }
    window.location.hash = link.dataset.target;
    target.setAttribute("tabindex", "-1");
    target.focus({ preventScroll: true });
    closeMenu();
  }

  searchForm.hidden = false;
  input.addEventListener("input", applySearch);
  searchForm.addEventListener("submit", function (event) {
    event.preventDefault();
    var first = articles.filter(function (article) {
      return !article.hidden;
    })[0];
    if (first && linkByTarget[first.id]) {
      jumpTo(linkByTarget[first.id]);
    }
  });

  // Keyboard: "/" finds, Esc clears, the arrow keys walk the contents.
  document.addEventListener("keydown", function (event) {
    var active = document.activeElement;
    var typing =
      active &&
      (active.tagName === "INPUT" || active.tagName === "TEXTAREA" || active.tagName === "SELECT");
    if (event.key === "/" && !typing && !event.ctrlKey && !event.metaKey && !event.altKey) {
      event.preventDefault();
      input.focus();
      input.select();
      return;
    }
    if (event.key === "Escape" && active === input) {
      input.value = "";
      applySearch();
      return;
    }
    if (event.key === "Escape" && nav.classList.contains("open")) {
      closeMenu();
      menu.focus();
      return;
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") {
      return;
    }
    if (active !== input && !(active && nav.contains(active) && active.tagName === "A")) {
      return;
    }
    openMenu();
    var links = visibleLinks();
    if (links.length === 0) {
      return;
    }
    event.preventDefault();
    var index = links.indexOf(active);
    if (event.key === "ArrowDown") {
      links[Math.min(index + 1, links.length - 1)].focus();
    } else if (index <= 0) {
      input.focus();
    } else {
      links[index - 1].focus();
    }
  });

  // The contents menu on small screens.
  function openMenu() {
    nav.classList.add("open");
    menu.setAttribute("aria-expanded", "true");
  }

  function closeMenu() {
    nav.classList.remove("open");
    menu.setAttribute("aria-expanded", "false");
  }

  menu.hidden = false;
  menu.addEventListener("click", function () {
    if (nav.classList.contains("open")) {
      closeMenu();
    } else {
      openMenu();
    }
  });
  nav.addEventListener("click", function (event) {
    if (event.target.closest && event.target.closest("a")) {
      closeMenu();
    }
  });

  // The contents follow the operation the address points at.
  function markCurrent() {
    navLinks.forEach(function (link) {
      link.removeAttribute("aria-current");
    });
    var current = linkByTarget[window.location.hash.slice(1)];
    if (current) {
      current.setAttribute("aria-current", "true");
      var group = current.closest("details");
      if (group) {
        group.open = true;
      }
    }
  }
  window.addEventListener("hashchange", markCurrent);
  markCurrent();

  // Try it. The token stays in this variable: it's never stored.
  var token = "";
  var servers = (document.body.dataset.servers || "").split(" ").filter(Boolean);

  function element(tag, className, text) {
    var node = document.createElement(tag);
    if (className) {
      node.className = className;
    }
    if (text !== undefined) {
      node.textContent = text;
    }
    return node;
  }

  function field(id, label, control) {
    var wrap = element("div", "field");
    var title = element("label", "", label);
    title.htmlFor = id;
    control.id = id;
    wrap.appendChild(title);
    wrap.appendChild(control);
    return wrap;
  }

  function cookie(name) {
    var prefix = name + "=";
    var parts = document.cookie.split("; ");
    for (var i = 0; i < parts.length; i += 1) {
      if (parts[i].indexOf(prefix) === 0) {
        return decodeURIComponent(parts[i].slice(prefix.length));
      }
    }
    return "";
  }

  function pretty(text, contentType) {
    if ((contentType || "").indexOf("json") === -1) {
      return text;
    }
    try {
      return JSON.stringify(JSON.parse(text), null, 2);
    } catch (_) {
      return text;
    }
  }

  function buildForm(article) {
    var method = article.dataset.method;
    var path = article.dataset.path;
    var params = JSON.parse(article.dataset.params || "[]");
    var id = article.id;

    var form = element("form", "try");
    var server = null;
    if (servers.length > 0) {
      server = element("select");
      server.appendChild(new Option("This server", ""));
      servers.forEach(function (url) {
        server.appendChild(new Option(url, url));
      });
      form.appendChild(field(id + "-server", "Server", server));
    }
    var inputs = {};
    params.forEach(function (name) {
      var control = element("input");
      control.required = true;
      control.autocomplete = "off";
      inputs[name] = control;
      form.appendChild(field(id + "-param-" + name, name, control));
    });
    var tokenInput = null;
    if (article.dataset.secured) {
      tokenInput = element("input");
      tokenInput.type = "password";
      tokenInput.autocomplete = "off";
      tokenInput.value = token;
      tokenInput.placeholder = "Leave empty to use this browser's session";
      tokenInput.addEventListener("input", function () {
        token = tokenInput.value;
      });
      form.appendChild(field(id + "-token", "Bearer token", tokenInput));
    }
    var body = null;
    if (article.dataset.body !== undefined) {
      body = element("textarea");
      body.value = article.dataset.body;
      body.spellcheck = false;
      form.appendChild(field(id + "-body", "Request body (JSON)", body));
    }
    var actions = element("div", "actions");
    var send = element("button", "primary", "Send");
    send.type = "submit";
    actions.appendChild(send);
    var line = element("code", "", method + " " + path);
    actions.appendChild(line);
    form.appendChild(actions);
    var outcome = element("div", "outcome");
    outcome.setAttribute("aria-live", "polite");
    form.appendChild(outcome);

    var running = null;
    form.addEventListener("submit", function (event) {
      event.preventDefault();
      if (running) {
        running.abort();
      }
      var url = path;
      params.forEach(function (name) {
        url = url.split("{" + name + "}").join(encodeURIComponent(inputs[name].value));
      });
      var base = server ? server.value.replace(/\/$/, "") : "";
      var headers = { Accept: "application/json" };
      var init = { method: method, headers: headers, credentials: "same-origin" };
      if (tokenInput && tokenInput.value) {
        headers.Authorization = "Bearer " + tokenInput.value;
      }
      if (body && method !== "GET" && method !== "HEAD") {
        headers["Content-Type"] = "application/json";
        init.body = body.value;
      }
      var xsrf = cookie("XSRF-TOKEN");
      if (xsrf && !base && method !== "GET" && method !== "HEAD") {
        headers["X-XSRF-TOKEN"] = xsrf;
      }
      running = new AbortController();
      init.signal = running.signal;
      outcome.textContent = "Sending...";
      var started = performance.now();
      fetch(base + url, init)
        .then(function (response) {
          return response.text().then(function (text) {
            var elapsed = Math.round(performance.now() - started);
            showResponse(outcome, response, text, elapsed);
          });
        })
        .catch(function (error) {
          if (error.name !== "AbortError") {
            outcome.textContent = "";
            outcome.appendChild(element("p", "line", "No response"));
            outcome.appendChild(
              element(
                "p",
                "note",
                error.message + ". A server on another origin must allow this page with CORS."
              )
            );
          }
        });
    });
    return form;
  }

  function showResponse(outcome, response, text, elapsed) {
    outcome.textContent = "";
    outcome.appendChild(
      element("p", "line", response.status + " " + response.statusText + " (" + elapsed + " ms)")
    );
    var details = element("details");
    details.appendChild(element("summary", "", "Response headers"));
    var lines = [];
    response.headers.forEach(function (value, name) {
      lines.push(name + ": " + value);
    });
    details.appendChild(element("pre", "", lines.join("\n")));
    outcome.appendChild(details);
    if (text) {
      outcome.appendChild(element("pre", "", pretty(text, response.headers.get("content-type"))));
    }
  }

  Array.prototype.forEach.call(document.querySelectorAll("article.operation"), function (article) {
    var open = element("button", "try-open", "Try it");
    open.type = "button";
    open.setAttribute("aria-expanded", "false");
    var form = null;
    open.addEventListener("click", function () {
      if (!form) {
        form = buildForm(article);
        article.appendChild(form);
        open.setAttribute("aria-expanded", "true");
        var first = form.querySelector("input, select, textarea, button");
        if (first) {
          first.focus();
        }
        return;
      }
      form.hidden = !form.hidden;
      open.setAttribute("aria-expanded", form.hidden ? "false" : "true");
    });
    article.appendChild(open);
  });
})();
