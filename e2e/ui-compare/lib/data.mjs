// The screens' data and the states the harness expects, made from the text files in
// ../data the same way the Navree page and the React build make theirs
// (../SCREENS.md says how). The harness reads the pages against this model, so a page
// that drifts from the data is caught before anything is measured.
import { readFileSync } from "node:fs";

const read = (name) =>
  readFileSync(new URL(`../data/${name}`, import.meta.url), "utf8")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);

export const CITIES = read("cities.txt");
const FIRST_NAMES = read("first-names.txt");
const SURNAMES = read("surnames.txt");
const STATUSES = ["Paid", "Pending", "Overdue", "Refunded"];

for (const [name, list, count] of [
  ["cities.txt", CITIES, 300],
  ["first-names.txt", FIRST_NAMES, 40],
  ["surnames.txt", SURNAMES, 25],
]) {
  if (list.length !== count) throw new Error(`data/${name} holds ${list.length} lines and the screens need ${count}`);
}

/** 2129334 as ₹21,29,334: the last three digits, then groups of two. */
export function rupees(amount) {
  const digits = String(amount);
  if (digits.length <= 3) return "₹" + digits;
  const head = digits.slice(0, -3);
  const tail = digits.slice(-3);
  let out = "";
  for (let at = 0; at < head.length; at++) {
    if (at > 0 && (head.length - at) % 2 === 0) out += ",";
    out += head[at];
  }
  return "₹" + out + "," + tail;
}

export const PAYMENTS = Array.from({ length: 500 }, (_, at) => {
  const amount = ((at * 7919 + 211) % 503) * 4217 + 12500;
  return {
    id: at + 1,
    name: `${FIRST_NAMES[at % 40]} ${SURNAMES[(at * 7 + Math.floor(at / 40)) % 25]}`,
    city: CITIES[(at * 17) % 300],
    rupees: amount,
    amount: rupees(amount),
    status: STATUSES[(at * 3 + Math.floor(at / 5)) % 4],
  };
});

export const PAGE_SIZE = 50;

/**
 * What the table shows for a sort, a search and a page: `sort` is "asc" or "desc" on the
 * amount (or null for the order of the data), `q` the text searched for in the names.
 */
export function tableView({ sort = null, q = "", page = 1 } = {}) {
  const needle = q.toLowerCase();
  let found = PAYMENTS.filter((row) => row.name.toLowerCase().includes(needle));
  if (sort) {
    found = [...found].sort((a, b) => a.rupees - b.rupees);
    if (sort === "desc") found.reverse();
  }
  const pages = Math.max(1, Math.ceil(found.length / PAGE_SIZE));
  const shown = found.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE);
  return { total: found.length, pages, page, rows: shown, first: shown[0]?.name ?? null };
}
