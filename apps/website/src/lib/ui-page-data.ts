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

/** The size ceilings (gzip) that `crates/navree-ui/tests/budgets.rs` asserts. */
export function uiBudgets(): Budget[] {
  const source = fs.readFileSync(path.join(WEBSITE, "../../crates/navree-ui/tests/budgets.rs"), "utf8");
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
      files: ["core", "sidebar", "overlay", "table"],
      ceilingKb: ceiling(/total <= (\d+) \* 1024/, "CRUD page"),
    },
    { label: "Every module together", files: "js", ceilingKb: ceiling(/total < (\d+) \* 1024/, "all modules") },
  ];
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
