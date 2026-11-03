/**
 * Checks the pages that answer questions: run after `pnpm build`.
 *
 *   node scripts/check-aeo.ts
 *
 * - The home page, `/ui/`, `/ai/` and the introduction of the docs each have
 *   a `FAQPage` with at least four questions;
 * - every answer is 40 to 80 words, in the data and on the page, and the
 *   visible answer is the same text as the one in the structured data;
 * - every question is a heading (`<h3>`) on the page, and none repeats;
 * - the installation and first-app pages carry a `HowTo` whose steps each
 *   point at an id that exists on the page, in the page's order.
 */
import { finish, nodes, pages, requireOut, typesOf, type Page } from "./lib/site-html.ts";

requireOut("check-aeo");

const MIN_WORDS = 40;
const MAX_WORDS = 80;
const faqPages = ["/", "/ui/", "/ai/", "/docs/prologue/introduction/"];
const howToPages = ["/docs/getting-started/installation/", "/docs/getting-started/your-first-app/"];

const problems: string[] = [];
const all = pages();
const words = (text: string) => text.split(/\s+/).filter(Boolean).length;
const collapse = (text: string) => text.replace(/\s+/g, " ").trim();

const idsOf = (page: Page) => new Set([...page.html.matchAll(/\sid="([^"]+)"/g)].map((m) => m[1]));

let questions = 0;
for (const url of faqPages) {
  const page = all.find((p) => p.url === url);
  if (!page) {
    problems.push(`${url}: the page is missing`);
    continue;
  }
  const faq = nodes(page).filter((n) => typesOf(n).includes("FAQPage"));
  if (faq.length !== 1) {
    problems.push(`${url}: ${faq.length} FAQPage nodes, expected 1`);
    continue;
  }
  const items = (faq[0].mainEntity ?? []) as { name?: string; acceptedAnswer?: { text?: string } }[];
  if (items.length < 4) problems.push(`${url}: ${items.length} questions in the FAQPage, expected at least 4`);
  const headings = [...page.body.matchAll(/<h3\b[^>]*>([^]*?)<\/h3>/g)].map((m) => collapse(m[1].replace(/<[^>]+>/g, "")));
  const seen = new Set<string>();
  for (const item of items) {
    questions++;
    const question = item.name ?? "";
    const answer = item.acceptedAnswer?.text ?? "";
    const n = words(answer);
    if (n < MIN_WORDS || n > MAX_WORDS) problems.push(`${url}: the answer to "${question}" is ${n} words (${MIN_WORDS}-${MAX_WORDS})`);
    if (seen.has(question)) problems.push(`${url}: "${question}" is asked twice`);
    seen.add(question);
    if (!question.endsWith("?")) problems.push(`${url}: "${question}" is not a question`);
    if (!headings.some((h) => collapse(h.replace(/&#x27;/g, "'").replace(/&amp;/g, "&")) === question)) {
      problems.push(`${url}: "${question}" is not an <h3> on the page`);
    }
    // The visible answer is the same words as the structured one.
    const visible = page.text.replace(/\s+/g, "");
    if (!visible.includes(answer.replace(/\s+/g, ""))) problems.push(`${url}: the answer to "${question}" on the page differs from the structured data`);
  }
}

let steps = 0;
for (const url of howToPages) {
  const page = all.find((p) => p.url === url);
  if (!page) {
    problems.push(`${url}: the page is missing`);
    continue;
  }
  const howTo = nodes(page).filter((n) => typesOf(n).includes("HowTo"));
  if (howTo.length !== 1) {
    problems.push(`${url}: ${howTo.length} HowTo nodes, expected 1`);
    continue;
  }
  const list = (howTo[0].step ?? []) as { name?: string; text?: string; url?: string; position?: number }[];
  if (list.length < 3) problems.push(`${url}: ${list.length} steps in the HowTo, expected at least 3`);
  const ids = idsOf(page);
  const order = [...page.html.matchAll(/\sid="([^"]+)"/g)].map((m) => m[1]);
  let last = -1;
  for (const step of list) {
    steps++;
    const anchor = step.url?.split("#")[1] ?? "";
    if (!anchor || !ids.has(anchor)) problems.push(`${url}: the step "${step.name}" points at #${anchor}, which is not on the page`);
    const at = order.indexOf(anchor);
    if (at !== -1 && at < last) problems.push(`${url}: the step "${step.name}" is out of the page's order`);
    if (at !== -1) last = at;
    if (!step.text || words(step.text) < 6) problems.push(`${url}: the step "${step.name}" has too little text`);
  }
}

finish("check-aeo", problems, `${faqPages.length} pages with FAQs (${questions} answers of ${MIN_WORDS}-${MAX_WORDS} words, each also visible as written), ${howToPages.length} HowTo pages (${steps} steps with real anchors)`);
