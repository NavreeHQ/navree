// The sidebar: collapse to icons (remembered in a cookie, so the first paint
// is right), Ctrl/Cmd+B, and an off-canvas sheet on phones.
import { every, on, one, setCookie } from "./dom.js";

const COOKIE = "navree_ui_sidebar";
const PHONE = matchMedia("(max-width: 767px)");

const wrapper = () => one("[data-slot=sidebar-wrapper]");

function closeSheet() {
  wrapper()?.removeAttribute("data-mobile-open");
}

function toggle() {
  const shell = wrapper();
  if (!shell) return;
  if (PHONE.matches) {
    const open = shell.toggleAttribute("data-mobile-open");
    if (open) /** @type {HTMLElement | null} */ (shell.querySelector("[data-slot=sidebar] a,[data-slot=sidebar] button"))?.focus();
    return;
  }
  const collapsed = shell.toggleAttribute("data-collapsed");
  setCookie(`${COOKIE}=${collapsed ? "collapsed" : "expanded"}; Path=/; Max-Age=31536000; SameSite=Lax`);
  for (const trigger of every("[data-ui-sidebar-trigger]")) trigger.setAttribute("aria-expanded", String(!collapsed));
}

on(document, "click", (e) => {
  const target = /** @type {Element} */ (e.target);
  if (target.closest?.("[data-ui-sidebar-trigger]")) toggle();
  else if (target.closest?.("[data-slot=sidebar-overlay]")) closeSheet();
  else if (PHONE.matches && target.closest?.("[data-slot=sidebar] a")) closeSheet();
});
on(document, "keydown", (e) => {
  if (e.key.toLowerCase() === "b" && (e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey) {
    e.preventDefault();
    toggle();
  } else if (e.key === "Escape" && wrapper()?.hasAttribute("data-mobile-open")) {
    closeSheet();
    /** @type {HTMLElement | null} */ (one("[data-ui-sidebar-trigger]"))?.focus();
  }
});
