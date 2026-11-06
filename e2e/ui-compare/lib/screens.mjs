// The four screens as the harness sees them: how to find each control on each build,
// what each step does, and the state each step must end in. SCREENS.md says the same in
// words; the data comes from lib/data.mjs.
//
// An action is one of
//   { kind: "click", sel, hasText? }   press and release on the first drawn match
//   { kind: "type", sel, text }        focus the match, select its text, type over it
//   { kind: "clear", sel }             focus the match, select its text, delete it
//   { kind: "key", key }               press one key
// and an expectation is a probe (see lib/inpage.js).
import { CITIES, PAGE_SIZE, tableView } from "./data.mjs";

/** `value` is a selector for both builds, or `{ navree, react }`. */
const by = (value, entrant) => (typeof value === "string" ? value : value[entrant]);

const SEARCH_TEXT = "kapoor";
const CITY_IN_SELECT = "Ahmedabad";
const CITY_IN_COMBOBOX = "Ludhiana";
const COMBOBOX_TEXT = "Ludh";

// ---------------------------------------------------------------- table

const T = "#cmp-table";
const table = {
  rows: `${T} tbody tr[data-key]`,
  firstCell: `${T} tbody tr[data-key] td:first-child`,
  search: "[role=search] input:not([type=hidden])",
  sortAmount: {
    navree: `${T} th[data-col=amount] a[data-slot=sort-link]`,
    react: `${T} th[data-col=amount] button[data-sort]`,
  },
  sortedAmount: `${T} th[data-col=amount][aria-sort=ascending]`,
  next: 'nav[aria-label=Pagination] [aria-label="Next page"]',
  page: "nav[aria-label=Pagination] [aria-current=page]",
};

/** The rows and the first name the table must show. */
const shows = (view) => [
  { sel: table.rows, count: Math.min(view.total, PAGE_SIZE) },
  { sel: table.firstCell, nth: 0, text: view.first },
];

function tableScreen() {
  const base = tableView();
  const filtered = tableView({ q: SEARCH_TEXT });
  const sorted = tableView({ sort: "asc" });
  const nextPage = tableView({ sort: "asc", page: 2 });
  const sortedFiltered = tableView({ sort: "asc", q: SEARCH_TEXT });
  const typeSearch = () => ({ kind: "type", sel: table.search, text: SEARCH_TEXT });
  return {
    key: "table",
    title: "Table",
    ready: () => ({ all: shows(base) }),
    first: {
      key: "filter",
      label: "Filter by typing",
      act: typeSearch,
      expect: () => ({ all: shows(filtered) }),
    },
    undo: {
      key: "clear",
      label: "Clear the search",
      act: () => ({ kind: "clear", sel: table.search }),
      expect: () => ({ all: shows(base) }),
    },
    chain: [
      {
        key: "sort",
        label: "Sort by amount",
        act: (entrant) => ({ kind: "click", sel: by(table.sortAmount, entrant) }),
        expect: () => ({ all: [{ sel: table.sortedAmount, exists: true }, ...shows(sorted)] }),
      },
      {
        key: "next",
        label: "Next page",
        act: () => ({ kind: "click", sel: table.next }),
        expect: () => ({ all: [{ sel: table.page, nth: 0, text: "2" }, ...shows(nextPage)] }),
      },
      {
        key: "filter",
        label: "Filter by typing",
        act: typeSearch,
        expect: () => ({ all: shows(sortedFiltered) }),
      },
    ],
    parity: [
      {
        label: "the headings",
        read: `[...document.querySelectorAll(${JSON.stringify(`${T} thead th`)})].map((th) => th.textContent.replace(/\\s+/g, " ").trim().toLowerCase()).slice(0, 4)`,
        expect: ["name", "city", "amount", "status"],
      },
      {
        label: "the first page of rows",
        read: `[...document.querySelectorAll(${JSON.stringify(table.rows)})].map((tr) => [...tr.querySelectorAll("td")].slice(0, 4).map((td) => td.textContent.replace(/\\s+/g, " ").trim()))`,
        expect: base.rows.map((row) => [row.name, row.city, row.amount, row.status]),
      },
    ],
  };
}

// ---------------------------------------------------------------- select

const selectPage = {
  trigger: "#cmp-select-trigger",
  popup: { navree: "#cmp-select-popup", react: "[data-slot=select-content]" },
  options: { navree: "#cmp-select-list [role=option]", react: "[data-slot=select-content] [role=option]" },
  comboOpen: { navree: "#cmp-combo-input", react: "#cmp-combo-trigger" },
  comboType: { navree: "#cmp-combo-input", react: "[data-slot=popover-content] [cmdk-input]" },
  comboPopup: { navree: "#cmp-combo-popup", react: "[data-slot=popover-content]" },
  comboOptions: {
    navree: "#cmp-combo-list [role=option]:not([hidden])",
    react: "[data-slot=popover-content] [role=option]",
  },
  comboValue: {
    navree: { sel: "#cmp-combo-input", prop: ["value", CITY_IN_COMBOBOX] },
    react: { sel: "#cmp-combo-trigger", text: CITY_IN_COMBOBOX },
  },
};

function selectScreen() {
  const openSelect = {
    key: "select-open",
    label: "Open the select",
    act: () => ({ kind: "click", sel: selectPage.trigger }),
    expect: (entrant) => ({ sel: by(selectPage.popup, entrant), visible: true }),
  };
  const closeIt = (popup) => ({
    key: "close",
    label: "Close the list",
    act: () => ({ kind: "key", key: "Escape" }),
    expect: (entrant) => ({ sel: by(popup, entrant), visible: false }),
  });
  return {
    key: "select",
    title: "Select and combobox",
    ready: () => ({ sel: selectPage.trigger, visible: true }),
    first: openSelect,
    undo: closeIt(selectPage.popup),
    chain: [
      openSelect,
      {
        key: "select-choose",
        label: "Choose in the select",
        act: (entrant) => ({ kind: "click", sel: by(selectPage.options, entrant), hasText: CITY_IN_SELECT }),
        expect: () => ({ sel: selectPage.trigger, text: CITY_IN_SELECT }),
      },
      {
        key: "combo-open",
        label: "Open the combobox",
        act: (entrant) => ({ kind: "click", sel: by(selectPage.comboOpen, entrant) }),
        expect: (entrant) => ({ sel: by(selectPage.comboPopup, entrant), visible: true }),
      },
      {
        key: "combo-type",
        label: "Type in the combobox",
        act: (entrant) => ({ kind: "type", sel: by(selectPage.comboType, entrant), text: COMBOBOX_TEXT }),
        expect: (entrant) => ({ sel: by(selectPage.comboOptions, entrant), nth: 0, text: CITY_IN_COMBOBOX }),
      },
      {
        key: "combo-choose",
        label: "Choose in the combobox",
        act: (entrant) => ({ kind: "click", sel: by(selectPage.comboOptions, entrant), hasText: CITY_IN_COMBOBOX }),
        expect: (entrant) => selectPage.comboValue[entrant],
      },
    ],
    parity: [
      {
        label: "the cities of the select",
        steps: [openSelect],
        read: (entrant) => `[...document.querySelectorAll(${JSON.stringify(by(selectPage.options, entrant))})].map((el) => el.textContent.replace(/\\s+/g, " ").trim())`,
        expect: CITIES,
        after: closeIt(selectPage.popup),
      },
      {
        label: "the cities of the combobox",
        steps: [
          {
            key: "combo-open",
            label: "Open the combobox",
            act: (entrant) => ({ kind: "click", sel: by(selectPage.comboOpen, entrant) }),
            expect: (entrant) => ({ sel: by(selectPage.comboPopup, entrant), visible: true }),
          },
        ],
        read: (entrant) => `[...document.querySelectorAll(${JSON.stringify(by(selectPage.comboOptions, entrant))})].map((el) => el.textContent.replace(/\\s+/g, " ").trim())`,
        expect: CITIES,
        after: closeIt(selectPage.comboPopup),
      },
    ],
  };
}

// ---------------------------------------------------------------- dates

const datesPage = {
  open: { navree: "#cmp-date [data-ui-act=open]", react: "#cmp-date-trigger" },
  popup: { navree: "#cmp-date-pop", react: "[data-slot=popover-content]" },
  next: {
    navree: "#cmp-date-pop [data-ui-cal=next]",
    react: '[data-slot=popover-content] button[aria-label*="next month" i]',
  },
  caption: {
    navree: "#cmp-date-pop [data-slot=calendar-caption]",
    react: "[data-slot=popover-content] [role=status]",
  },
  day18: {
    navree: { sel: '#cmp-date-pop button[data-date="2026-12-18"]' },
    react: { sel: "[data-slot=popover-content] table button", hasText: "18" },
  },
  label: { navree: "#cmp-date [data-ui-label]", react: "#cmp-date-trigger" },
};

function datesScreen() {
  const open = {
    key: "date-open",
    label: "Open the picker",
    act: (entrant) => ({ kind: "click", sel: by(datesPage.open, entrant) }),
    expect: (entrant) => ({
      all: [
        { sel: by(datesPage.popup, entrant), visible: true },
        { sel: by(datesPage.caption, entrant), nth: 0, text: "November 2026" },
      ],
    }),
  };
  return {
    key: "dates",
    title: "Date picker",
    ready: (entrant) => ({ sel: by(datesPage.open, entrant), visible: true }),
    first: open,
    undo: {
      key: "close",
      label: "Close the calendar",
      act: () => ({ kind: "key", key: "Escape" }),
      expect: (entrant) => ({ sel: by(datesPage.popup, entrant), visible: false }),
    },
    chain: [
      open,
      {
        key: "date-next",
        label: "Next month",
        act: (entrant) => ({ kind: "click", sel: by(datesPage.next, entrant) }),
        expect: (entrant) => ({ sel: by(datesPage.caption, entrant), nth: 0, text: "December 2026" }),
      },
      {
        key: "date-choose",
        label: "Choose a day",
        act: (entrant) => ({ kind: "click", ...datesPage.day18[entrant] }),
        expect: (entrant) => ({ sel: by(datesPage.label, entrant), nth: 0, text: "12/18/2026" }),
      },
    ],
    parity: [
      {
        label: "the empty picker",
        read: (entrant) => `[...document.querySelectorAll(${JSON.stringify(by(datesPage.label, entrant))})].map((el) => el.textContent.replace(/\\s+/g, " ").trim())`,
        expect: ["Pick a date"],
      },
    ],
  };
}

// ---------------------------------------------------------------- tabs

const tabsPage = {
  trigger: "#cmp-menu-trigger",
  menu: "[role=menu]",
  chosen: "#cmp-chosen",
};

function tabsScreen() {
  const openMenu = {
    key: "menu-open",
    label: "Open the menu",
    act: () => ({ kind: "click", sel: tabsPage.trigger }),
    expect: () => ({ sel: tabsPage.menu, visible: true }),
  };
  return {
    key: "tabs",
    title: "Tabs and menu",
    ready: () => ({ sel: tabsPage.trigger, visible: true }),
    first: openMenu,
    undo: {
      key: "close",
      label: "Close the menu",
      act: () => ({ kind: "key", key: "Escape" }),
      expect: () => ({ sel: tabsPage.menu, visible: false }),
    },
    chain: [
      {
        key: "tab-switch",
        label: "Switch tab",
        act: () => ({ kind: "click", sel: "[role=tab]", hasText: "Activity" }),
        expect: () => ({
          all: [
            { sel: "[role=tab][aria-selected=true]", nth: 0, text: "Activity" },
            { sel: "[role=tabpanel]", hasText: "The activity panel.", visible: true },
          ],
        }),
      },
      openMenu,
      {
        key: "menu-choose",
        label: "Choose a menu item",
        act: () => ({ kind: "click", sel: "[role=menuitem]", hasText: "Duplicate" }),
        expect: () => ({ sel: tabsPage.chosen, nth: 0, text: "Chosen: Duplicate" }),
      },
    ],
    parity: [
      {
        label: "the tabs",
        read: '[...document.querySelectorAll("[role=tab]")].map((el) => el.textContent.replace(/\\s+/g, " ").trim())',
        expect: ["Overview", "Activity", "Settings"],
      },
      {
        label: "the status line",
        read: '[...document.querySelectorAll("#cmp-chosen")].map((el) => el.textContent.replace(/\\s+/g, " ").trim())',
        expect: ["Nothing chosen yet."],
      },
      {
        label: "the menu",
        steps: [openMenu],
        read: '[...document.querySelectorAll("[role=menuitem]")].map((el) => el.textContent.replace(/\\s+/g, " ").trim())',
        expect: ["Edit", "Duplicate", "Archive"],
        after: {
          key: "close",
          label: "Close the menu",
          act: () => ({ kind: "key", key: "Escape" }),
          expect: () => ({ sel: tabsPage.menu, visible: false }),
        },
      },
    ],
  };
}

export const SCREENS = [tableScreen(), selectScreen(), datesScreen(), tabsScreen()];
