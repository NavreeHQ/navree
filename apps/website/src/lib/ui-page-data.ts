import fs from "node:fs";
import path from "node:path";

/**
 * Data for the `/ui/` page, read at build time from the files that own it,
 * so the page can't disagree with them:
 *
 * - the roadmap from the table in `content/docs/ui/coming-next.mdx`;
 * - the speed budgets from `crates/navree-ui/tests/budgets.rs`, whose
 *   asserts fail the toolkit's build when a file grows past its ceiling.
 *
 * Either file changing shape fails the site's build instead of showing
 * something stale.
 */

export type RoadmapStatus = "Shipped" | "Building" | "Next" | "Later";
export type RoadmapItem = { order: number; text: string; status: RoadmapStatus };

const WEBSITE = process.cwd();

/** Every row of the "Coming next" table, in order. Links become their text; code spans stay. */
export function uiRoadmap(): RoadmapItem[] {
  const source = fs.readFileSync(path.join(WEBSITE, "content/docs/ui/coming-next.mdx"), "utf8");
  const items: RoadmapItem[] = [];
  for (const line of source.split("\n")) {
    const m = /^\|\s*(\d+)\s*\|\s*(.+?)\s*\|\s*(Shipped|Building|Next|Later)\s*\|\s*$/.exec(line);
    if (m) items.push({ order: Number(m[1]), text: m[2].replace(/\[([^\]]+)\]\([^)]+\)/g, "$1"), status: m[3] as RoadmapStatus });
  }
  if (items.length < 3) throw new Error("ui-page: no roadmap table in content/docs/ui/coming-next.mdx");
  return items;
}

/** A ceiling and the toolkit files it covers (`"js"` is every script). */
export type Budget = { label: string; files: string[] | "js"; ceilingKb: number };

const budgetsSource = () => fs.readFileSync(path.join(WEBSITE, "../../crates/navree-ui/tests/budgets.rs"), "utf8");

/** The body of the test named `test` in `budgets.rs`, up to the next test. */
function budgetTest(source: string, test: string): string {
  const start = source.indexOf(`fn ${test}(`);
  if (start < 0) throw new Error(`ui-page: no test ${test} in crates/navree-ui/tests/budgets.rs`);
  const next = source.indexOf("#[test]", start);
  return source.slice(start, next < 0 ? undefined : next);
}

/** The names in the first `[...]` list of a test, as `["core", "sidebar"]`. */
function budgetList(body: string, what: string): string[] {
  const list = /\[([^\]]+)\]/.exec(body)?.[1];
  if (!list) throw new Error(`ui-page: no file list in the ${what} budget in crates/navree-ui/tests/budgets.rs`);
  return [...list.matchAll(/"([a-z0-9-]+)"/g)].map((m) => m[1]);
}

/** The size ceilings (gzip) that `crates/navree-ui/tests/budgets.rs` asserts. */
export function uiBudgets(): Budget[] {
  const source = budgetsSource();
  const ceiling = (pattern: RegExp, what: string) => {
    const m = pattern.exec(source);
    if (!m) throw new Error(`ui-page: no ${what} budget in crates/navree-ui/tests/budgets.rs`);
    return Number(m[1]);
  };
  return [
    { label: "The stylesheet", files: ["navree-ui"], ceilingKb: ceiling(/size\("navree-ui"\) <= (\d+) \* 1024/, "stylesheet") },
    { label: "The core runtime", files: ["core"], ceilingKb: ceiling(/size\("core"\) <= (\d+) \* 1024/, "core") },
    { label: "The table module", files: ["table"], ceilingKb: ceiling(/size\("table"\) <= (\d+) \* 1024/, "table") },
    {
      label: "A typical CRUD page's script",
      // The files the test adds up, read from it: the page's script grows when the test's list does.
      files: budgetList(budgetTest(source, "a_typical_crud_page_loads_at_most_30_kb_gzip_of_script"), "CRUD page"),
      ceilingKb: ceiling(/total <= (\d+) \* 1024/, "CRUD page"),
    },
    {
      label: "The heaviest page's script",
      // The inbox's conversation page, built in the test: the files it loads are read from it.
      files: budgetList(budgetTest(source, /fn (the_heaviest_page_loads_at_most_\d+_kb_gzip_of_script)\(/.exec(source)?.[1] ?? "the_heaviest_page"), "heaviest page"),
      ceilingKb: ceiling(/fn the_heaviest_page_loads_at_most_\d+_kb_gzip_of_script\(\)[^]*?total <= (\d+) \* 1024/, "heaviest page"),
    },
  ];
}

/** What a toolkit file weighs once compressed, as the build measured it. */
export type AssetSize = { name: string; ext: string; gzip: number | null; brotli: number | null };

/** One row of the docs' budget table. */
export type BudgetRow = { label: string; ceiling: string; measured: string | null };

const kbOf = (bytes: number) => `${(bytes / 1024).toFixed(1)} KB`;

/**
 * Every ceiling `budgets.rs` holds, as rows with the size now: one for each
 * script and the stylesheet, one for the module stylesheets, the numbering plans,
 * a typical CRUD page and all the modules together. A test added to the file
 * shows here with no edit, and one that changes shape fails the site's build.
 * Node runs this for the Markdown copies of the docs as well.
 */
export function uiBudgetRows(assets: AssetSize[]): BudgetRow[] {
  const source = budgetsSource();
  const find = (name: string) => assets.find((a) => a.name === name);
  const gzip = (name: string) => find(name)?.gzip ?? 0;
  const code = (names: string[]) => names.map((n) => `\`${n}\``).join(", ");
  const rows: BudgetRow[] = [];

  const files = [...source.matchAll(/assert!\(size\("([a-z0-9-]+)"\) <= (?:(\d+) \* )?1024/g)].map((m) => ({
    name: m[1],
    kbs: m[2] ? Number(m[2]) : 1,
  }));
  if (!files.length) throw new Error("ui-page: no per-file budgets in crates/navree-ui/tests/budgets.rs");
  const row = (name: string, label: string, kbs: number) => rows.push({ label, ceiling: `${kbs} KB`, measured: kbOf(gzip(name)) });
  for (const f of files.filter((x) => x.name === "navree-ui")) row(f.name, "The stylesheet", f.kbs);
  for (const f of files.filter((x) => x.name === "core")) row(f.name, "The `core` runtime", f.kbs);
  for (const f of files.filter((x) => x.name !== "navree-ui" && x.name !== "core")) row(f.name, `The \`${f.name}\` module`, f.kbs);

  const styles = budgetTest(source, "the_modules_styles_are_at_most_3_kb_gzip_each");
  const styleCeiling = /<= (\d+) \* 1024/.exec(styles)?.[1];
  if (!styleCeiling) throw new Error("ui-page: no module stylesheet budget in crates/navree-ui/tests/budgets.rs");
  const styleNames = budgetList(styles, "module stylesheets");
  rows.push({
    label: `Each module's stylesheet (${code(styleNames)})`,
    ceiling: `${styleCeiling} KB`,
    measured: `up to ${kbOf(Math.max(...styleNames.map(gzip)))}`,
  });

  const plans = /brotli <= (\d+) \* 1024/.exec(source)?.[1];
  if (!plans) throw new Error("ui-page: no numbering plans budget in crates/navree-ui/tests/budgets.rs");
  rows.push({
    label: "The numbering plans the `phone` module loads on first use",
    ceiling: `${plans} KB (brotli)`,
    measured: kbOf(find("phone-data")?.brotli ?? 0),
  });

  for (const b of uiBudgets().filter((x) => x.label.startsWith("A typical") || x.label.startsWith("The heaviest"))) {
    const names = b.files === "js" ? assets.filter((a) => a.ext === "js").map((a) => a.name) : b.files;
    const total = names.reduce((n, name) => n + gzip(name), 0);
    rows.push(
      b.label.startsWith("The heaviest")
        ? { label: `The heaviest page's script, the inbox's conversation (${code(names)})`, ceiling: `${b.ceilingKb} KB`, measured: kbOf(total) }
        : { label: `A typical CRUD page's script (${code(names)})`, ceiling: `${b.ceilingKb} KB`, measured: kbOf(total) },
    );
  }

  if (!source.includes("fn a_page_with_nothing_interactive_ships_no_script(")) {
    throw new Error("ui-page: no no-script test in crates/navree-ui/tests/budgets.rs");
  }
  rows.push({ label: "A page with nothing interactive", ceiling: "no `<script>` at all", measured: null });
  return rows;
}

/**
 * The names of the modules still to come, from the roadmap table: the words
 * before the colon of every row that isn't Shipped. A sentence about what is
 * still to come is written from these, so it can't outlive the row.
 */
export function uiPending(): string[] {
  return uiRoadmap()
    .filter((item) => item.status !== "Shipped")
    .map((item) => item.text.split(":")[0].trim());
}

/** `Charts, rich text editing and islands`: names as a sentence lists them, only the first capitalised. */
export function pendingSentence(names: string[]): string {
  const shown = names.map((name, i) => (i === 0 || /^[A-Z]{2}/.test(name) ? name : name.charAt(0).toLowerCase() + name.slice(1)));
  return shown.length < 2 ? (shown[0] ?? "") : `${shown.slice(0, -1).join(", ")} and ${shown.at(-1)}`;
}

/**
 * The render-time ceilings, in milliseconds, that the release tests in
 * `crates/navree-ui/tests/budgets.rs` assert: each test's
 * `Duration::from_millis(n)`, read from the test with that name.
 */
export function uiRenderCeilings() {
  const source = fs.readFileSync(path.join(WEBSITE, "../../crates/navree-ui/tests/budgets.rs"), "utf8");
  const ms = (test: string) => {
    const m = new RegExp(`fn ${test}\\(\\)[\\s\\S]*?Duration::from_millis\\((\\d+)\\)`).exec(source);
    if (!m) throw new Error(`ui-page: no render-time test ${test} in crates/navree-ui/tests/budgets.rs`);
    return Number(m[1]);
  };
  return {
    thousandRows: ms("a_thousand_rows_render_in_under_3_ms"),
    hundredRowFeed: ms("a_hundred_row_feed_renders_in_under_1_ms"),
    hundredEventTimeline: ms("a_hundred_event_timeline_renders_in_under_1_ms"),
    phoneInput: ms("a_page_with_a_phone_input_renders_in_under_1_ms"),
    tenThousandOptions: ms("ten_thousand_options_render_in_under_40_ms"),
  };
}
