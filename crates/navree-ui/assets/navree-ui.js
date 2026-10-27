/*
 * navree-ui: the behavior of Navree's server-rendered tools. No framework,
 * no inline scripts (the pages run under `script-src 'self'`): everything is
 * wired through data attributes on the markup the navree-ui templates render.
 *
 *   Sidebar      [data-ui-shell] with data-sidebar="expanded|collapsed";
 *                [data-ui-sidebar-trigger] toggles; Ctrl/Cmd+B toggles; the
 *                state persists in localStorage and the `navree_ui_sidebar`
 *                cookie (so the server renders it without a flash); on
 *                phones the sidebar is an off-canvas sheet.
 *   Menus        [data-ui-menu-trigger] with aria-controls -> .ui-menu
 *   Dialogs      [data-ui-dialog-open="id"], [data-ui-dialog-close]
 *   Palette      Ctrl/Cmd+K or [data-ui-command-open] -> #ui-command
 *   Tabs         [role=tablist] inside [data-ui-tabs]
 *   Toasts       NavreeUI.toast({ title, description, variant }),
 *                [data-ui-toast-trigger], and server toasts in #ui-toaster
 *   Tables       [data-ui-select-all], [data-ui-select-row], selection bar
 *   Per page     select[data-ui-per-page] navigates to data-url ({per_page})
 */
(() => {
  "use strict";

  const doc = document;
  const root = doc.documentElement;
  const COOKIE = "navree_ui_sidebar";
  const STORE = "navree-ui:sidebar";
  const mobile = window.matchMedia("(max-width: 767.98px)");
  const isMac = /Mac|iPhone|iPad|iPod/.test(navigator.platform || navigator.userAgent || "");

  const $ = (selector, scope = doc) => scope.querySelector(selector);
  const $$ = (selector, scope = doc) => Array.from(scope.querySelectorAll(selector));
  const focusable =
    'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

  function store(key, value) {
    try {
      if (value === undefined) return window.localStorage.getItem(key);
      window.localStorage.setItem(key, value);
    } catch (_) {
      /* storage can be unavailable (private mode, file: URLs in some browsers) */
    }
    return null;
  }

  // Shortcut labels follow the platform: "Ctrl" or "⌘".
  for (const el of $$("[data-ui-mod]")) el.textContent = isMac ? "⌘" : "Ctrl";

  /* ---------------------------------------------------------------------- */
  /* Sidebar                                                                 */
  /* ---------------------------------------------------------------------- */

  const shell = $("[data-ui-shell]");
  const sidebar = shell && $("[data-ui-sidebar]", shell);
  let lastTrigger = null;

  function collapsed() {
    return !!shell && shell.dataset.sidebar === "collapsed";
  }

  function syncTriggers() {
    const expanded = mobile.matches ? shell.dataset.mobile === "open" : !collapsed();
    for (const trigger of $$("[data-ui-sidebar-trigger]")) {
      trigger.setAttribute("aria-expanded", String(expanded));
    }
  }

  function setCollapsed(value, persist = true) {
    if (!shell) return;
    shell.dataset.sidebar = value ? "collapsed" : "expanded";
    if (persist) {
      const state = value ? "collapsed" : "expanded";
      store(STORE, state);
      doc.cookie = `${COOKIE}=${state}; path=/; max-age=31536000; samesite=lax`;
    }
    hideTooltip();
    syncTriggers();
  }

  function openSheet() {
    if (!shell) return;
    lastTrigger = doc.activeElement;
    shell.dataset.mobile = "open";
    doc.body.style.overflow = "hidden";
    syncTriggers();
    const target = $('[aria-current="page"]', sidebar) || $(focusable, sidebar);
    if (target) window.setTimeout(() => target.focus({ preventScroll: true }), 60);
  }

  function closeSheet(restoreFocus = true) {
    if (!shell || shell.dataset.mobile !== "open") return;
    delete shell.dataset.mobile;
    doc.body.style.overflow = "";
    syncTriggers();
    if (restoreFocus && lastTrigger && lastTrigger.focus) lastTrigger.focus();
  }

  function toggleSidebar() {
    if (!shell) return;
    if (mobile.matches) {
      if (shell.dataset.mobile === "open") closeSheet();
      else openSheet();
    } else {
      setCollapsed(!collapsed());
    }
  }

  if (shell) {
    // The server renders the cookie's state; without a cookie (a first visit,
    // a file: URL) the stored preference wins, applied without animation.
    const saved = store(STORE);
    if ((saved === "collapsed" || saved === "expanded") && !shell.hasAttribute("data-sidebar-from-cookie")) {
      if (saved !== shell.dataset.sidebar) {
        shell.classList.add("ui-no-transition");
        setCollapsed(saved === "collapsed", false);
        window.requestAnimationFrame(() =>
          window.requestAnimationFrame(() => shell.classList.remove("ui-no-transition")),
        );
      }
    }
    syncTriggers();

    for (const trigger of $$("[data-ui-sidebar-trigger]")) {
      trigger.addEventListener("click", toggleSidebar);
    }
    const overlay = $("[data-ui-overlay]", shell);
    if (overlay) overlay.addEventListener("click", () => closeSheet());

    mobile.addEventListener("change", () => {
      closeSheet(false);
      syncTriggers();
    });

    // A link followed inside the sheet closes it.
    sidebar.addEventListener("click", (event) => {
      if (mobile.matches && event.target.closest("a[href]")) closeSheet(false);
    });

    // Collapsible items: toggle their sub-items; when the sidebar is collapsed
    // to icons, follow the item's own link instead.
    for (const button of $$("[data-ui-collapsible]", sidebar)) {
      button.addEventListener("click", () => {
        if (collapsed() && !mobile.matches && button.dataset.href) {
          window.location.href = button.dataset.href;
          return;
        }
        const open = button.getAttribute("aria-expanded") !== "true";
        button.setAttribute("aria-expanded", String(open));
        const list = doc.getElementById(button.getAttribute("aria-controls"));
        if (list) list.hidden = !open;
      });
    }
  }

  /* Tooltips beside the items of the collapsed sidebar. */
  let tooltip = null;

  function showTooltip(target) {
    if (!collapsed() || mobile.matches) return;
    const text = target.dataset.tooltip;
    if (!text) return;
    if (!tooltip) {
      tooltip = doc.createElement("div");
      tooltip.className = "ui-tooltip";
      tooltip.setAttribute("role", "tooltip");
      doc.body.appendChild(tooltip);
    }
    tooltip.textContent = text;
    tooltip.hidden = false;
    const rect = target.getBoundingClientRect();
    const height = tooltip.offsetHeight;
    tooltip.style.left = `${Math.round(rect.right + 8)}px`;
    tooltip.style.top = `${Math.round(rect.top + rect.height / 2 - height / 2)}px`;
  }

  function hideTooltip() {
    if (tooltip) tooltip.hidden = true;
  }

  if (sidebar) {
    for (const el of $$("[data-tooltip]", sidebar)) {
      el.addEventListener("mouseenter", () => showTooltip(el));
      el.addEventListener("focus", () => showTooltip(el));
      el.addEventListener("mouseleave", hideTooltip);
      el.addEventListener("blur", hideTooltip);
    }
    sidebar.addEventListener("scroll", hideTooltip, true);
  }

  /* ---------------------------------------------------------------------- */
  /* Menus                                                                   */
  /* ---------------------------------------------------------------------- */

  let openMenu = null;

  function menuItems(menu) {
    return $$('[role="menuitem"], [role="menuitemcheckbox"], [role="menuitemradio"]', menu).filter(
      (item) => !item.hasAttribute("disabled") && item.getAttribute("aria-disabled") !== "true",
    );
  }

  function placeMenu(trigger, menu) {
    const gap = 6;
    const margin = 8;
    const rect = trigger.getBoundingClientRect();
    let side = trigger.dataset.side || "bottom";
    const align = trigger.dataset.align || "start";
    if (side === "right" && mobile.matches) side = "top";
    if (trigger.hasAttribute("data-match-width")) menu.style.minWidth = `${Math.max(rect.width, 192)}px`;
    menu.style.left = "0px";
    menu.style.top = "0px";
    const width = menu.offsetWidth;
    const height = menu.offsetHeight;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    let left;
    let top;
    if (side === "right") {
      left = rect.right + gap;
      top = align === "end" ? rect.bottom - height : rect.top;
      if (left + width > vw - margin) left = rect.left - gap - width;
    } else {
      left = align === "end" ? rect.right - width : rect.left;
      top = side === "top" ? rect.top - gap - height : rect.bottom + gap;
      if (side !== "top" && top + height > vh - margin && rect.top - gap - height > margin) {
        top = rect.top - gap - height;
      }
      if (side === "top" && top < margin) top = rect.bottom + gap;
    }
    left = Math.min(Math.max(left, margin), vw - width - margin);
    top = Math.min(Math.max(top, margin), vh - height - margin);
    menu.style.left = `${Math.round(left)}px`;
    menu.style.top = `${Math.round(top)}px`;
    menu.classList.add("is-positioned");
  }

  function closeMenu(restoreFocus = true) {
    if (!openMenu) return;
    const { trigger, menu } = openMenu;
    menu.hidden = true;
    menu.classList.remove("is-positioned");
    trigger.setAttribute("aria-expanded", "false");
    openMenu = null;
    if (restoreFocus) trigger.focus();
  }

  function showMenu(trigger, focusLast = false) {
    const menu = doc.getElementById(trigger.getAttribute("aria-controls"));
    if (!menu) return;
    closeMenu(false);
    hideTooltip();
    // Menus render next to their trigger; they move to <body> so that no
    // scrolling or clipping ancestor (the sidebar) cuts them off.
    if (menu.parentElement !== doc.body) doc.body.appendChild(menu);
    menu.hidden = false;
    trigger.setAttribute("aria-expanded", "true");
    placeMenu(trigger, menu);
    openMenu = { trigger, menu };
    const items = menuItems(menu);
    const item = focusLast ? items[items.length - 1] : items[0];
    if (item) item.focus();
  }

  for (const trigger of $$("[data-ui-menu-trigger]")) {
    trigger.setAttribute("aria-haspopup", "menu");
    trigger.setAttribute("aria-expanded", "false");
    trigger.addEventListener("click", (event) => {
      event.preventDefault();
      if (openMenu && openMenu.trigger === trigger) closeMenu();
      else showMenu(trigger);
    });
    trigger.addEventListener("keydown", (event) => {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        showMenu(trigger, event.key === "ArrowUp");
      }
    });
  }

  doc.addEventListener("keydown", (event) => {
    if (!openMenu) return;
    const items = menuItems(openMenu.menu);
    const index = items.indexOf(doc.activeElement);
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      closeMenu();
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      const next = items[(index + step + items.length) % items.length];
      if (next) next.focus();
    } else if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      const next = event.key === "Home" ? items[0] : items[items.length - 1];
      if (next) next.focus();
    } else if (event.key === "Tab") {
      closeMenu(false);
    }
  }, true);

  doc.addEventListener("click", (event) => {
    if (!openMenu) return;
    const { trigger, menu } = openMenu;
    if (trigger.contains(event.target)) return;
    if (menu.contains(event.target)) {
      if (event.target.closest('[role^="menuitem"]')) closeMenu(false);
      return;
    }
    closeMenu(false);
  });

  window.addEventListener("resize", () => closeMenu(false));
  window.addEventListener("scroll", () => closeMenu(false), true);

  /* ---------------------------------------------------------------------- */
  /* Dialogs                                                                 */
  /* ---------------------------------------------------------------------- */

  function openDialog(id, opener) {
    const dialog = typeof id === "string" ? doc.getElementById(id) : id;
    if (!dialog || dialog.open) return;
    closeMenu(false);
    dialog._opener = opener || doc.activeElement;
    dialog.showModal();
    const auto = $("[autofocus]", dialog) || $(".ui-dialog__body " + focusable.split(",")[0], dialog);
    if (auto) auto.focus();
  }

  function closeDialog(dialog, value) {
    if (dialog && dialog.open) dialog.close(value);
  }

  doc.addEventListener("click", (event) => {
    const opener = event.target.closest("[data-ui-dialog-open]");
    if (opener) {
      event.preventDefault();
      openDialog(opener.dataset.uiDialogOpen, opener);
      return;
    }
    const closer = event.target.closest("[data-ui-dialog-close]");
    if (closer) {
      event.preventDefault();
      closeDialog(closer.closest("dialog"));
    }
  });

  for (const dialog of $$("dialog.ui-dialog")) {
    // A click on the backdrop (the dialog element itself) closes it.
    dialog.addEventListener("click", (event) => {
      if (event.target === dialog) closeDialog(dialog);
    });
    dialog.addEventListener("close", () => {
      const opener = dialog._opener;
      dialog._opener = null;
      if (opener && opener.focus && doc.contains(opener)) opener.focus();
    });
  }

  /* ---------------------------------------------------------------------- */
  /* Command palette                                                         */
  /* ---------------------------------------------------------------------- */

  const palette = $("#ui-command");
  const paletteInput = palette && $(".ui-command__input", palette);
  const paletteEmpty = palette && $(".ui-command__empty", palette);
  let paletteActive = -1;

  function paletteVisible() {
    return $$(".ui-command__item", palette).filter((item) => !item.hidden);
  }

  function paletteSelect(index) {
    const items = paletteVisible();
    for (const item of items) item.setAttribute("aria-selected", "false");
    if (!items.length) {
      paletteActive = -1;
      paletteInput.removeAttribute("aria-activedescendant");
      return;
    }
    paletteActive = (index + items.length) % items.length;
    const item = items[paletteActive];
    item.setAttribute("aria-selected", "true");
    paletteInput.setAttribute("aria-activedescendant", item.id);
    item.scrollIntoView({ block: "nearest" });
  }

  function paletteFilter() {
    const terms = paletteInput.value.trim().toLowerCase().split(/\s+/).filter(Boolean);
    let shown = 0;
    for (const group of $$(".ui-command__group", palette)) {
      let groupShown = 0;
      for (const item of $$(".ui-command__item", group)) {
        const text = (item.dataset.search || item.textContent).toLowerCase();
        const match = terms.every((term) => text.includes(term));
        item.hidden = !match;
        if (match) groupShown += 1;
      }
      group.hidden = groupShown === 0;
      shown += groupShown;
    }
    if (paletteEmpty) paletteEmpty.hidden = shown !== 0;
    paletteSelect(0);
  }

  function openCommand() {
    if (!palette) return;
    if (palette.open) {
      paletteInput.focus();
      return;
    }
    closeMenu(false);
    if (shell && shell.dataset.mobile === "open") closeSheet(false);
    palette._opener = doc.activeElement;
    paletteInput.value = "";
    paletteFilter();
    palette.showModal();
    paletteInput.focus();
  }

  function runItem(item) {
    if (!item) return;
    palette.close();
    const href = item.dataset.href;
    if (href) window.location.href = href;
    else if (item.dataset.uiDialogOpen) openDialog(item.dataset.uiDialogOpen);
  }

  if (palette) {
    $$(".ui-command__item", palette).forEach((item, index) => {
      if (!item.id) item.id = `ui-command-item-${index}`;
      item.addEventListener("click", () => runItem(item));
      item.addEventListener("mousemove", () => {
        const index = paletteVisible().indexOf(item);
        if (index !== paletteActive) paletteSelect(index);
      });
    });
    paletteInput.addEventListener("input", paletteFilter);
    paletteInput.addEventListener("keydown", (event) => {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        paletteSelect(paletteActive + (event.key === "ArrowDown" ? 1 : -1));
      } else if (event.key === "Enter") {
        event.preventDefault();
        runItem(paletteVisible()[paletteActive]);
      }
    });
    palette.addEventListener("click", (event) => {
      if (event.target === palette) palette.close();
    });
    palette.addEventListener("close", () => {
      const opener = palette._opener;
      palette._opener = null;
      if (opener && opener.focus && doc.contains(opener)) opener.focus();
    });
  }

  for (const button of $$("[data-ui-command-open]")) {
    button.addEventListener("click", (event) => {
      event.preventDefault();
      openCommand();
    });
  }

  /* ---------------------------------------------------------------------- */
  /* Keyboard shortcuts                                                      */
  /* ---------------------------------------------------------------------- */

  doc.addEventListener("keydown", (event) => {
    const mod = isMac ? event.metaKey : event.ctrlKey;
    if (mod && !event.altKey && !event.shiftKey) {
      const key = event.key.toLowerCase();
      if (key === "b" && shell) {
        event.preventDefault();
        toggleSidebar();
        return;
      }
      if (key === "k" && palette) {
        event.preventDefault();
        if (palette.open) palette.close();
        else openCommand();
        return;
      }
    }
    if (event.key === "Escape" && shell && shell.dataset.mobile === "open" && !openMenu) {
      closeSheet();
    }
  });

  /* ---------------------------------------------------------------------- */
  /* Tabs                                                                    */
  /* ---------------------------------------------------------------------- */

  for (const tabs of $$("[data-ui-tabs]")) {
    const list = $('[role="tablist"]', tabs);
    if (!list) continue;
    const all = $$('[role="tab"]', list);
    const select = (tab, focus) => {
      for (const other of all) {
        const selected = other === tab;
        other.setAttribute("aria-selected", String(selected));
        other.tabIndex = selected ? 0 : -1;
        const panel = doc.getElementById(other.getAttribute("aria-controls"));
        if (panel) panel.hidden = !selected;
      }
      if (focus) tab.focus();
    };
    for (const tab of all) {
      tab.addEventListener("click", () => select(tab, false));
      tab.addEventListener("keydown", (event) => {
        const index = all.indexOf(tab);
        let next = null;
        if (event.key === "ArrowRight") next = all[(index + 1) % all.length];
        else if (event.key === "ArrowLeft") next = all[(index - 1 + all.length) % all.length];
        else if (event.key === "Home") next = all[0];
        else if (event.key === "End") next = all[all.length - 1];
        if (next) {
          event.preventDefault();
          select(next, true);
        }
      });
    }
  }

  /* ---------------------------------------------------------------------- */
  /* Toasts                                                                  */
  /* ---------------------------------------------------------------------- */

  const ICONS = {
    success:
      '<circle cx="12" cy="12" r="10"/><path d="m9 12 2 2 4-4"/>',
    error: '<circle cx="12" cy="12" r="10"/><line x1="12" x2="12" y1="8" y2="12"/><line x1="12" x2="12.01" y1="16" y2="16"/>',
    warning:
      '<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/>',
    info: '<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>',
    close: '<path d="M18 6 6 18"/><path d="m6 6 12 12"/>',
  };

  function svg(name) {
    return `<svg class="ui-icon" xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICONS[name]}</svg>`;
  }

  function toaster() {
    let list = $("#ui-toaster");
    if (!list) {
      list = doc.createElement("ol");
      list.id = "ui-toaster";
      list.className = "ui-toaster";
      list.setAttribute("aria-live", "polite");
      doc.body.appendChild(list);
    }
    return list;
  }

  function dismiss(toast) {
    if (!toast || toast.classList.contains("is-leaving")) return;
    toast.classList.add("is-leaving");
    window.setTimeout(() => toast.remove(), 170);
  }

  function arm(toast) {
    const close = $("[data-ui-toast-close]", toast);
    if (close) close.addEventListener("click", () => dismiss(toast));
    const duration = Number(toast.dataset.duration || 5000);
    if (duration > 0) {
      let timer = window.setTimeout(() => dismiss(toast), duration);
      toast.addEventListener("mouseenter", () => window.clearTimeout(timer));
      toast.addEventListener("mouseleave", () => {
        timer = window.setTimeout(() => dismiss(toast), 2000);
      });
    }
  }

  function toast({ title, description = "", variant = "info", duration = 5000 } = {}) {
    const item = doc.createElement("li");
    const kind = ["success", "error", "warning", "info"].includes(variant) ? variant : "info";
    item.className = `ui-toast ui-toast--${kind}`;
    item.setAttribute("role", kind === "error" ? "alert" : "status");
    item.dataset.duration = String(duration);
    item.innerHTML = `${svg(kind)}<div><p class="ui-toast__title"></p><p class="ui-toast__desc"></p></div><button type="button" class="ui-btn ui-btn--ghost ui-btn--icon ui-btn--sm ui-toast__close" data-ui-toast-close aria-label="Dismiss">${svg("close")}</button>`;
    $(".ui-toast__title", item).textContent = title || "";
    const desc = $(".ui-toast__desc", item);
    if (description) desc.textContent = description;
    else desc.remove();
    const list = toaster();
    list.appendChild(item);
    while (list.children.length > 4) list.firstElementChild.remove();
    arm(item);
    return item;
  }

  for (const item of $$("#ui-toaster .ui-toast")) arm(item);

  for (const trigger of $$("[data-ui-toast-trigger]")) {
    trigger.addEventListener("click", () =>
      toast({
        title: trigger.dataset.toastTitle,
        description: trigger.dataset.toastDescription,
        variant: trigger.dataset.toastVariant,
      }),
    );
  }

  /* ---------------------------------------------------------------------- */
  /* Tables: row selection and rows per page                                 */
  /* ---------------------------------------------------------------------- */

  for (const table of $$("[data-ui-table]")) {
    const all = $("[data-ui-select-all]", table);
    const rows = $$("[data-ui-select-row]", table);
    const bar = table.id ? doc.querySelector(`[data-ui-selection-bar="${table.id}"]`) : null;
    const count = bar && $("[data-ui-selection-count]", bar);
    // The bar floats over the page only while its table is on screen.
    let inView = true;
    const sync = () => {
      const checked = rows.filter((row) => row.checked).length;
      for (const row of rows) {
        const tr = row.closest("tr");
        if (tr) tr.toggleAttribute("data-selected", row.checked);
      }
      if (all) {
        all.checked = checked > 0 && checked === rows.length;
        all.indeterminate = checked > 0 && checked < rows.length;
      }
      if (bar) bar.hidden = checked === 0 || !inView;
      if (count) count.textContent = `${checked} selected`;
    };
    if (bar && "IntersectionObserver" in window) {
      new IntersectionObserver((entries) => {
        inView = entries[entries.length - 1].isIntersecting;
        sync();
      }).observe(table.closest(".ui-table-wrap") || table);
    }
    if (all) {
      all.addEventListener("change", () => {
        for (const row of rows) row.checked = all.checked;
        sync();
      });
    }
    for (const row of rows) row.addEventListener("change", sync);
    sync();
  }

  for (const select of $$("select[data-ui-per-page]")) {
    select.addEventListener("change", () => {
      const url = select.dataset.url || "";
      if (url) window.location.href = url.replace("{per_page}", encodeURIComponent(select.value));
    });
  }

  root.classList.add("ui-ready");

  window.NavreeUI = {
    toast,
    openDialog,
    closeDialog,
    openCommand,
    toggleSidebar,
    setSidebarCollapsed: (value) => setCollapsed(!!value),
  };
})();
