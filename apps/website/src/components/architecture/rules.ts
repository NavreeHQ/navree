/**
 * The rules every Navree app follows, as `/architecture/` shows them under
 * "Built in". Each rule is a card: the rule, two sentences, its proof (tests
 * and measurements, each with the file that holds it) and the docs page that
 * explains it.
 *
 * A card is shown only when its proof is in the repository at build time:
 * every file and symbol it names is read here, from the code itself. When
 * one is missing, the card is left out and the build says which and why, so
 * a rule reaches the site with the code that keeps it, never before. A
 * measurement file that is there but has changed shape fails the build
 * instead of showing a stale number. Numbers are read from the files that
 * own them, never typed in, and written in Indian digit grouping (`indian`
 * in `src/lib/format.ts`), as the rest of the site writes counts.
 *
 * Server components only: this reads the repository with `node:fs`.
 */
import fs from "node:fs";
import path from "node:path";

import GithubSlugger from "github-slugger";

import { sections } from "@content/docs/manifest";
import { lessonId, lessons } from "@/content/about";
import { indian } from "@/lib/format";

/** The repository's root, from `apps/website`. */
const REPO = path.resolve(process.cwd(), "..", "..");

/** One piece of proof: what it is (plain text with `code` spans) and the repository file that holds it. */
export type Proof = { what: string; file: string; note?: string };

export type Rule = {
  /** The card's id on the page, and its anchor: `/architecture/#lazy-by-default`. */
  id: string;
  /** The rule, as the card's heading. */
  title: string;
  /** Two sentences: what holds, then how. Plain text with `code` spans. */
  text: string;
  proof: Proof[];
  /** The docs page that explains it. */
  docs: { href: string; label: string };
  /** The lesson on `/about/` behind it, or the lessons as a whole. */
  story: { href: string; label: string };
};

/** A rule, or what is missing for it. */
type Built = Omit<Rule, "story"> | { id: string; missing: string };

/** A file of the repository, or null when there is none. */
function read(file: string): string | null {
  try {
    return fs.readFileSync(path.join(REPO, file), "utf8");
  } catch {
    return null;
  }
}

/** The file, or the first of `needles` it doesn't hold; null when all are there. */
function lacking(file: string, ...needles: string[]): string | null {
  const text = read(file);
  if (text === null) return file;
  const missing = needles.find((needle) => !text.includes(needle));
  return missing === undefined ? null : `${missing} in ${file}`;
}

/**
 * What a docs link needs: the page in the manifest, its file, and the heading
 * its `#anchor` names. Null when all are there.
 */
function docsLacking(href: string): string | null {
  const m = /^\/docs\/([^/]+)\/([^/#]+)\/(?:#(.+))?$/.exec(href);
  if (!m) return `a docs link of the form /docs/<section>/<page>/: ${href}`;
  const [, section, page, anchor] = m;
  if (!sections.some((s) => s.slug === section && s.pages.includes(page))) return `${section}/${page} in content/docs/manifest.ts`;
  const file = `apps/website/content/docs/${section}/${page}.mdx`;
  const text = read(file);
  if (text === null) return file;
  if (!anchor) return null;
  const slugger = new GithubSlugger();
  const ids = [...text.replace(/```[^]*?```/g, "").matchAll(/^#{2,4} (.+)$/gm)].map((h) => slugger.slug(h[1].trim()));
  return ids.includes(anchor) ? null : `the heading #${anchor} in ${file}`;
}

/** Each check in turn: the first thing missing, or null when the proof is complete. */
function first(...checks: (() => string | null)[]): string | null {
  for (const check of checks) {
    const missing = check();
    if (missing) return missing;
  }
  return null;
}


/** A JSON file of the repository, or null when there is none. */
function json(file: string): unknown {
  const text = read(file);
  return text === null ? null : JSON.parse(text);
}

/** The number at `at` (`"a.b"`) in `value`. The file is there, so anything else is a change of shape: the build fails. */
function numberAt(value: unknown, at: string, file: string): number {
  const found = at
    .split(".")
    .reduce<unknown>((v, key) => (v && typeof v === "object" ? (v as Record<string, unknown>)[key] : undefined), value);
  if (typeof found === "number") return found;
  // A count written for reading ("5,499,392" or "54,99,392") counts too.
  if (typeof found === "string" && /^\d[\d,]*$/.test(found)) return Number(found.replace(/,/g, ""));
  throw new Error(`architecture: ${file} has no number at ${at}`);
}

const measuredOn = (value: unknown) => (typeof value === "string" && /^\d{4}-\d\d-\d\d$/.test(value) ? `measured ${value}` : undefined);

// ---------------------------------------------------------------- numbers

/** The store example app at scale (`content/showcase/store.json`, written by its browser suite). */
function storeProof(): Proof | null {
  const file = "apps/website/content/showcase/store.json";
  const data = json(file) as { measured_on?: unknown; pages?: { name: string }[] } | null;
  if (data === null) return null;
  const orders = numberAt(data, "rows.orders", file);
  const page = data.pages?.find((p) => p.name === "Orders");
  if (!page) throw new Error(`architecture: ${file} has no Orders page`);
  const paint = numberAt(page, "desktop.lcp_ms", file);
  return { what: `The store: ${indian(orders)} orders, and the orders page paints in ${indian(paint)}\u00a0ms`, file, note: measuredOn(data.measured_on) };
}

/**
 * The support inbox example app at scale: `content/showcase/inbox.json`, or
 * the object its browser suite writes into `inbox-numbers.ts`.
 */
function inboxProof(): Proof | null {
  let file = "apps/website/content/showcase/inbox.json";
  let data = json(file);
  if (data === null) {
    file = "apps/website/src/components/showcase/inbox-numbers.ts";
    const text = read(file);
    if (text === null) return null;
    const object = /=\s*(\{[^]*\})\s*as const;?\s*$/.exec(text);
    if (!object) throw new Error(`architecture: ${file} holds no numbers object`);
    data = JSON.parse(object[1]);
  }
  const messages = numberAt(data, "messages", file);
  const paint = numberAt(data, "firstScreen.largest_paint_ms", file);
  const bytes = numberAt(data, "firstScreen.bytes_total", file);
  const rows = numberAt(data, "flat.maxRows", file);
  // How many conversations: in the numbers, or the app's full scale.
  const stated = (data as { conversations?: unknown }).conversations;
  const scale = /pub const FULL: i64 = ([\d_]+);/.exec(read("apps/inbox-demo/src/scale.rs") ?? "");
  const conversations = typeof stated === "number" ? stated : scale ? Number(scale[1].replace(/_/g, "")) : null;
  const what = conversations ? `${indian(conversations)} conversations and ${indian(messages)} messages` : `${indian(messages)} messages`;
  return {
    what: `The support inbox: ${what}. The first screen paints in ${indian(paint)}\u00a0ms with ${indian(bytes)}\u00a0bytes, and the page never holds more than ${indian(rows)} rows`,
    file,
    note: measuredOn((data as { measured?: unknown }).measured),
  };
}

/** The toolkit's measured script sizes, and the ceilings `budgets.rs` asserts. */
function scriptBudgets() {
  const source = read("crates/navree-ui/tests/budgets.rs");
  const toolkit = json("apps/website/content/ui/toolkit.json") as { assets?: { name: string; ext: string; gzip: number | null }[] } | null;
  if (source === null || !toolkit?.assets) return null;
  const modules = toolkit.assets.filter((asset) => asset.ext === "js");
  const ceilings = new Map([...source.matchAll(/size\("([\w-]+)"\) <= (\d+) \* 1024/g)].map((m) => [m[1], Number(m[2])]));
  const all = /fn all_modules_together_are_under_\d+_kb_gzip\(\)[^]*?total < (\d+) \* 1024/.exec(source);
  const core = modules.find((m) => m.name === "core");
  const coreCeiling = ceilings.get("core");
  if (!core?.gzip || !coreCeiling || !all) return null;
  return {
    core: core.gzip,
    coreCeiling,
    own: modules.filter((m) => ceilings.has(m.name)).length,
    modules: modules.length,
    total: modules.reduce((sum, m) => sum + (m.gzip ?? 0), 0),
    allCeiling: Number(all[1]),
  };
}

/** The contrast a theme's text must reach, from `PairKind::Text` in `theme/pairs.rs`. */
function textContrast(): string | null {
  const m = /PairKind::Text => (\d+(?:\.\d+)?)/.exec(read("crates/navree-ui/src/theme/pairs.rs") ?? "");
  return m ? m[1] : null;
}

/** What `PAGE_CSP` in `view.rs` lacks to allow no inline code at all; null when it allows none. */
function policyLacking(): string | null {
  const file = "crates/navree-ui/src/view.rs";
  const m = /pub const PAGE_CSP: &str = "([^"]+)";/.exec(read(file) ?? "");
  if (!m) return `PAGE_CSP in ${file}`;
  const policy = m[1];
  if (/unsafe|'nonce-|'sha\d+-/.test(policy)) return `a PAGE_CSP that allows no inline code in ${file}`;
  const directive = ["default-src 'none'", "script-src 'self'", "style-src 'self'"].find((d) => !policy.includes(d));
  return directive ? `${directive} in PAGE_CSP in ${file}` : null;
}

// ------------------------------------------------------------------ rules

function serverAuthority(): Built {
  const id = "server-authority";
  const docs = { href: "/docs/ui/overview/#what-ships-to-the-browser", label: "What ships to the browser" };
  const budgets = scriptBudgets();
  const missing = first(
    () => lacking("crates/navree-ui/tests/budgets.rs", "fn a_page_with_nothing_interactive_ships_no_script()", 'assert!(!page.to_html().contains("<script"));'),
    () => (budgets ? null : "core and all-modules budgets in crates/navree-ui/tests/budgets.rs and content/ui/toolkit.json"),
    () => docsLacking(docs.href),
  );
  if (missing || !budgets) return { id, missing: missing ?? "budgets" };
  const most = budgets.own === budgets.modules ? "every module has" : budgets.own * 2 > budgets.modules ? "most modules have" : "some modules have";
  return {
    id,
    title: "The server is the authority; the browser gets small modules",
    text: `Pages are typed Rust components that the server renders to HTML, and a page with nothing interactive ships no script. Script comes in small modules, each loaded only by a page that uses it, and ${most} a size ceiling that fails the build when it is crossed.`,
    proof: [
      { what: "Test `a_page_with_nothing_interactive_ships_no_script`", file: "crates/navree-ui/tests/budgets.rs" },
      {
        what: `The core runtime is ${indian(budgets.core)}\u00a0bytes gzip, under its ${budgets.coreCeiling}\u00a0KB ceiling. Of the ${budgets.modules} modules, ${budgets.own} have a ceiling of their own, and all ${budgets.modules} together are ${indian(budgets.total)}\u00a0bytes, under ${budgets.allCeiling}\u00a0KB`,
        file: "apps/website/content/ui/toolkit.json",
      },
    ],
    docs,
  };
}

function lazyByDefault(): Built {
  const id = "lazy-by-default";
  const docs = { href: "/docs/orm/pagination-and-streaming/#cursor-pagination", label: "Cursor pagination" };
  const missing = first(
    () => lacking("crates/navree-orm/src/pagination.rs", "pub async fn cursor_paginate<", "pub struct CursorPage<", "no count and no offset"),
    () => lacking("crates/navree-orm/tests/orm/pagination.rs", "async fn cursor_pages_walk_forward_and_back()"),
    () => lacking("crates/navree-ui/src/table/orm.rs", "fn keyset("),
    () => lacking("crates/navree-ui/src/table/mod.rs", "pub cursor: bool"),
    () => docsLacking(docs.href),
  );
  if (missing) return { id, missing };
  const scale = [storeProof(), inboxProof()].filter((proof): proof is Proof => proof !== null);
  return {
    id,
    title: scale.length ? "Lazy by default, at real scale" : "Lazy by default",
    text: "A list loads a page at a time, and the DataTable sorts, filters and pages on the server. Cursor pages go by keyset, with no count and no offset, so the thousandth page costs what the first does.",
    proof: [
      { what: "`Builder::cursor_paginate`: the rows after the last one seen, no count and no offset", file: "crates/navree-orm/src/pagination.rs" },
      { what: "Test `cursor_pages_walk_forward_and_back`", file: "crates/navree-orm/tests/orm/pagination.rs" },
      { what: "`keyset`: the DataTable's cursor pages", file: "crates/navree-ui/src/table/orm.rs" },
      ...scale,
    ],
    docs,
  };
}

function liveUpdates(): Built {
  const id = "live-updates";
  const docs = { href: "/docs/ui/regions/#live-from-a-stream", label: "Live from a stream" };
  const morph = "focus, caret, selection and typed values survive";
  const table = "live refresh: new rows arrive; typing, menus, selection and the URL are kept";
  const missing = first(
    () => lacking("crates/navree-ui/src/table/region.rs", "pub fn live_from(", "its payload is never read"),
    () => lacking("crates/navree-ui/runtime/src/morph.js", "Focus, caret, selection"),
    () => lacking("e2e/ui/tests/morph.test.mjs", morph),
    () => lacking("e2e/ui/tests/datatable.test.mjs", table),
    () => docsLacking(docs.href),
  );
  if (missing) return { id, missing };
  return {
    id,
    title: "Live updates don't take the page away from you",
    text: "A live region follows a stream of server-sent events, and an event only tells it to fetch its HTML again, so a stream can't put markup into the page. The new HTML morphs in place, keeping focus, the caret, the selection and what you typed.",
    proof: [
      { what: "`Region::live_from`", file: "crates/navree-ui/src/table/region.rs" },
      { what: `Test \`${morph}\``, file: "e2e/ui/tests/morph.test.mjs" },
      { what: `Test \`${table}\``, file: "e2e/ui/tests/datatable.test.mjs" },
    ],
    docs,
  };
}

function noInjection(): Built {
  const id = "no-injection";
  const docs = { href: "/docs/ui/overview/#security", label: "Security" };
  const hostile = "a field with any name leaves the form working";
  const missing = first(
    policyLacking,
    () => lacking("crates/navree-ui/src/view.rs", "fn no_inline_code_is_allowed_by_the_policy()"),
    () => lacking("crates/navree-ui/runtime/src/dom.js", "goes through the prototype"),
    () => lacking("e2e/ui/tests/hostile.test.mjs", hostile, '["firefox", firefox]'),
    () => docsLacking(docs.href),
  );
  if (missing) return { id, missing };
  return {
    id,
    title: "A front end that can't be talked into running code",
    text: "Every page carries a strict same-origin content security policy that allows no inline script or style at all. The runtime reads the page through the DOM's own prototypes, so an element named to trick it, such as a field called `method`, changes nothing.",
    proof: [
      { what: "`PAGE_CSP` and test `no_inline_code_is_allowed_by_the_policy`", file: "crates/navree-ui/src/view.rs" },
      { what: "Every read through the DOM's own prototypes", file: "crates/navree-ui/runtime/src/dom.js" },
      { what: `Test \`${hostile}\`, in Chromium and Firefox`, file: "e2e/ui/tests/hostile.test.mjs" },
    ],
    docs,
  };
}

function refusedForms(): Built {
  const id = "refused-forms";
  const docs = { href: "/docs/ui/forms/#what-was-posted", label: "What was posted" };
  const missing = first(
    () => lacking("crates/navree-ui/src/form.rs", "pub fn values_of(", "the runtime keeps the page"),
    () =>
      lacking(
        "crates/navree-ui/tests/forms_tables.rs",
        "fn a_rejected_form_keeps_what_was_typed_and_escapes_it()",
        "fn a_handler_reads_what_was_posted_from_a_refusal()",
        "fn a_password_is_never_among_the_values_a_refusal_gives_back()",
      ),
    () => docsLacking(docs.href),
  );
  if (missing) return { id, missing };
  return {
    id,
    title: "A refused form keeps what was typed",
    text: "When the server refuses a form, the handler can read what was posted, never a password, and with JavaScript off the form comes back holding it beside the errors. With JavaScript on, the page stays as it is and only the errors appear.",
    proof: [
      { what: "Tests `a_rejected_form_keeps_what_was_typed_and_escapes_it` and `a_handler_reads_what_was_posted_from_a_refusal`", file: "crates/navree-ui/tests/forms_tables.rs" },
      { what: "Test `a_password_is_never_among_the_values_a_refusal_gives_back`", file: "crates/navree-ui/tests/forms_tables.rs" },
    ],
    docs,
  };
}

function errorsArePages(): Built {
  const id = "errors-are-pages";
  const docs = { href: "/docs/ui/errors-and-states/", label: "Errors and states" };
  const inShell = "a signed-in user gets the error inside the shell: sidebar, top bar, the short title as the heading";
  const data = "a route that is data keeps its JSON error, and so does the API, for a browser too";
  const missing = first(
    () => lacking("crates/navree-http/src/error_pages.rs", "pub trait ErrorPages", "pub enum RouteKind"),
    () => lacking("crates/navree-macros/src/route.rs", '"json_only"'),
    () => lacking("e2e/ui/tests/pages.test.mjs", inShell, data),
    () => docsLacking(docs.href),
  );
  if (missing) return { id, missing };
  return {
    id,
    title: "Errors are pages, APIs stay JSON",
    text: "An error on a page route is drawn as a page, inside the app's shell when the user is signed in. A route marked `#[json_only]`, and any client that asks for JSON alone, gets the JSON error body unchanged.",
    proof: [
      { what: "`ErrorPages` and `RouteKind`", file: "crates/navree-http/src/error_pages.rs" },
      { what: `Test \`${inShell}\``, file: "e2e/ui/tests/pages.test.mjs" },
      { what: `Test \`${data}\``, file: "e2e/ui/tests/pages.test.mjs" },
    ],
    docs,
  };
}

function checkedThemes(): Built {
  const id = "checked-themes";
  const docs = { href: "/docs/ui/themes/", label: "Themes" };
  const ratio = textContrast();
  const missing = first(
    () => (ratio ? null : "PairKind::Text in crates/navree-ui/src/theme/pairs.rs"),
    () => lacking("crates/navree-ui/src/theme/mod.rs", "The server writes it on `<html>`"),
    () => lacking("crates/navree-ui/src/theme/codec.rs", "fn a_stored_theme_whose_text_fails_is_refused_with_the_pairs()"),
    () => lacking("crates/navree-ui/tests/drawn_pairs.rs"),
    () => docsLacking(docs.href),
  );
  if (missing) return { id, missing };
  return {
    id,
    title: "Themes the server sets, with contrast checked",
    text: `A theme is a typed set of tokens, and the server writes light, dark or the device's choice on the page, so the first paint is already right. A theme whose text falls below ${ratio}:1 contrast is refused, and a test reads the stylesheet to find every pair it draws.`,
    proof: [
      { what: "Test `a_stored_theme_whose_text_fails_is_refused_with_the_pairs`", file: "crates/navree-ui/src/theme/codec.rs" },
      { what: "Every pair the stylesheet draws, found and checked", file: "crates/navree-ui/tests/drawn_pairs.rs" },
    ],
    docs,
  };
}

function rankedLists(): Built {
  const id = "ranked-lists";
  const docs = { href: "/docs/ui/drag-and-drop/", label: "Drag and drop" };
  const test = "a_placed_move_locks_its_lists_in_order_before_it_reads_anything";
  const missing = first(
    () => lacking("crates/navree-ui/src/sortable/ranked.rs", "pub trait RankedLists", "fn lock(", "reads only what it needs"),
    () => lacking("crates/navree-ui/src/sortable/orm.rs", "pub async fn lock_lists", "FOR UPDATE"),
    () => lacking("crates/navree-ui/tests/sortable.rs", `fn ${test}()`),
    () => docsLacking(docs.href),
  );
  if (missing) return { id, missing };
  return {
    id,
    title: "Moves in ranked lists lock before they read",
    text: "A move locks the lists it touches, in a fixed order, before it reads anything, so two moves at once can't lose a card or wait on each other forever. Then it reads only what it needs: where the card is, the list's size and rules, and the cards beside where it goes.",
    proof: [
      { what: "`RankedLists::lock`", file: "crates/navree-ui/src/sortable/ranked.rs" },
      { what: "`lock_lists`: `SELECT ... FOR UPDATE` on PostgreSQL and MySQL, one list at a time in sorted order", file: "crates/navree-ui/src/sortable/orm.rs" },
      { what: `Test \`${test}\``, file: "crates/navree-ui/tests/sortable.rs" },
    ],
    docs,
  };
}

/** The rules in the order the page shows them. */
const builders = [serverAuthority, lazyByDefault, liveUpdates, noInjection, refusedForms, errorsArePages, checkedThemes, rankedLists];

/** Where a rule's story is told: its lesson on `/about/`, or the lessons as a whole. */
function storyOf(id: string): Rule["story"] {
  const lesson = lessons.find((l) => l.rules?.includes(id));
  if (!lesson) return { href: "/about/#lessons", label: "The lessons behind Navree" };
  return { href: `/about/#${lessonId(lesson)}`, label: `The lesson: ${lesson.title.replace(/\.$/, "")}` };
}

let cache: Rule[] | null = null;

/** The rules whose proof is in the repository, in order. The build logs each one it leaves out. */
export function rules(): Rule[] {
  if (cache) return cache;
  const shown: Rule[] = [];
  for (const build of builders) {
    const built = build();
    if ("missing" in built) {
      console.warn(`architecture: the rule "${built.id}" is left out until its proof is in the repository: no ${built.missing}`);
      continue;
    }
    shown.push({ ...built, story: storyOf(built.id) });
  }
  cache = shown;
  return shown;
}

/** The card a lesson on `/about/` links to: the first of its rules that `/architecture/` shows. */
export function ruleFor(ids: string[] | undefined): Rule | null {
  if (!ids) return null;
  return rules().find((rule) => ids.includes(rule.id)) ?? null;
}
