// The sidebar: collapse to icons or off the canvas (remembered in a cookie, so
// the first paint is right), Ctrl/Cmd+B, and an off-canvas sheet on phones. A
// sidebar that never folds, or links in the top bar, fold nothing on wide screens.
import { all, attr, closest, every, has, on, one, set, setCookie, toggle as flip, unset } from "./dom.js";

const COOKIE = "navree_ui_sidebar";
const PHONE = matchMedia("(max-width: 767px)");

const wrapper = () => one("[data-slot=sidebar-wrapper]");

function closeSheet() {
  const shell = wrapper();
  if (shell) unset(shell, "data-mobile-open");
}

function toggle() {
  const shell = wrapper();
  if (!shell) return;
  if (PHONE.matches) {
    const open = flip(shell, "data-mobile-open");
    if (open) /** @type {HTMLElement | undefined} */ (all(shell, "[data-slot=sidebar] a,[data-slot=sidebar] button")[0])?.focus();
    return;
  }
  // On wide screens the links in the top bar, or a sidebar that never folds, stay as they are.
  if (attr(shell, "data-layout") === "header" || attr(shell, "data-collapse") === "never") return;
  const collapsed = flip(shell, "data-collapsed");
  setCookie(`${COOKIE}=${collapsed ? "collapsed" : "expanded"}; Path=/; Max-Age=31536000; SameSite=Lax`);
  for (const trigger of every("[data-ui-sidebar-trigger]")) set(trigger, "aria-expanded", String(!collapsed));
}

on(document, "click", (e) => {
  if (closest(e.target, "[data-ui-sidebar-trigger]")) toggle();
  else if (closest(e.target, "[data-slot=sidebar-overlay]")) closeSheet();
  else if (PHONE.matches && closest(e.target, "[data-slot=sidebar] a")) closeSheet();
});
on(document, "keydown", (e) => {
  if (e.key.toLowerCase() === "b" && (e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey) {
    e.preventDefault();
    toggle();
  } else if (e.key === "Escape") {
    const shell = wrapper();
    if (!shell || !has(shell, "data-mobile-open")) return;
    closeSheet();
    /** @type {HTMLElement | null} */ (one("[data-ui-sidebar-trigger]"))?.focus();
  }
});
