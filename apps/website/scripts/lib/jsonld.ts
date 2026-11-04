/**
 * Checks one page's JSON-LD against schema.org and against what the site
 * promises. Used by `scripts/check-seo.ts`.
 *
 * The vocabulary check reads `scripts/data/schemaorg-subset.json`: for each
 * type the site uses, every property schema.org defines for it (inherited
 * ones included). A property that is not there is a typo or the wrong type.
 * On top of that, a table of required properties per type says what each
 * node must have to be useful to a search engine or an AI answer, and
 * every `{ "@id": ... }` reference must point at a node on the same page.
 */
import fs from "node:fs";
import path from "node:path";

import { WEBSITE, nodes, typesOf, type Jsonld, type Page } from "./site-html.ts";

type Subset = { types: Record<string, { parents: string[]; properties: string[] }> };
const subset = JSON.parse(fs.readFileSync(path.join(WEBSITE, "scripts", "data", "schemaorg-subset.json"), "utf8")) as Subset;

/** Properties every node of a type must carry. */
const required: Record<string, string[]> = {
  Organization: ["name", "url", "logo", "sameAs"],
  WebSite: ["name", "url", "publisher", "inLanguage"],
  SoftwareApplication: [
    "name",
    "description",
    "applicationCategory",
    "operatingSystem",
    "softwareVersion",
    "license",
    "isAccessibleForFree",
    "offers",
  ],
  SoftwareSourceCode: ["name", "codeRepository", "programmingLanguage", "license", "version"],
  WebPage: ["url", "name", "description", "inLanguage", "dateModified", "isPartOf", "breadcrumb"],
  CollectionPage: ["url", "name", "description", "inLanguage", "dateModified", "isPartOf", "breadcrumb"],
  AboutPage: ["url", "name", "description", "inLanguage", "dateModified", "isPartOf", "breadcrumb", "mainEntity"],
  Person: ["name", "url", "sameAs"],
  Place: ["name"],
  TechArticle: ["headline", "description", "url", "dateModified", "author", "publisher", "mainEntityOfPage", "isPartOf"],
  BreadcrumbList: ["itemListElement"],
  ListItem: ["position", "name", "item"],
  ImageObject: ["url"],
  FAQPage: ["mainEntity", "url"],
  Question: ["name", "acceptedAnswer"],
  Answer: ["text"],
  HowTo: ["name", "description", "step"],
  HowToStep: ["name", "text", "url", "position"],
  Offer: ["price", "priceCurrency"],
  MediaObject: ["contentUrl", "encodingFormat"],
  ComputerLanguage: ["name"],
  ItemList: ["itemListElement"],
};

/** Properties whose values are URLs and must be absolute https URLs on a real host. */
const urlProps = new Set(["url", "item", "contentUrl", "codeRepository", "installUrl", "releaseNotes", "image", "sameAs", "license", "mainEntityOfPage"]);
const dateProps = new Set(["dateModified", "datePublished"]);

const isNode = (v: unknown): v is Jsonld => typeof v === "object" && v !== null && !Array.isArray(v);
const list = (v: unknown): unknown[] => (Array.isArray(v) ? v : v === undefined ? [] : [v]);

function allowed(type: string): Set<string> | null {
  const entry = subset.types[type];
  return entry ? new Set(entry.properties) : null;
}

/** Validate a node and the typed objects nested in it. */
function checkNode(node: Jsonld, where: string, problems: string[], ids: Map<string, string>): void {
  const types = typesOf(node);
  const onlyRef = Object.keys(node).length === 1 && "@id" in node;
  if (onlyRef) return;
  if (!types.length) {
    problems.push(`${where}: a node without @type: ${JSON.stringify(node).slice(0, 80)}`);
    return;
  }
  const props = new Set<string>();
  for (const type of types) {
    const set = allowed(type);
    if (!set) {
      problems.push(`${where}: @type ${type} is not in the schema.org subset (scripts/data/schemaorg-subset.json)`);
      continue;
    }
    for (const prop of set) props.add(prop);
    for (const need of required[type] ?? []) {
      const value = node[need];
      if (value === undefined || value === "" || (Array.isArray(value) && value.length === 0)) {
        problems.push(`${where}: ${type} needs "${need}"`);
      }
    }
  }
  const id = node["@id"];
  if (typeof id === "string") {
    const seen = ids.get(id);
    const summary = JSON.stringify(node);
    if (seen !== undefined && seen !== summary) problems.push(`${where}: @id ${id} is used by two different nodes`);
    ids.set(id, summary);
  }
  for (const [key, value] of Object.entries(node)) {
    if (key.startsWith("@")) continue;
    if (props.size && !props.has(key)) problems.push(`${where}: ${types.join("/")} has no property "${key}" in schema.org`);
    for (const item of list(value)) {
      if (isNode(item)) checkNode(item, `${where}.${key}`, problems, ids);
      else if (urlProps.has(key) && typeof item === "string" && !/^https:\/\/[a-z0-9.-]+\.[a-z]{2,}(\/|$|\?|#)/i.test(item)) {
        problems.push(`${where}.${key}: "${item.slice(0, 80)}" is not an absolute https URL`);
      } else if (dateProps.has(key)) {
        const when = typeof item === "string" ? Date.parse(item) : NaN;
        if (Number.isNaN(when) || !/^\d{4}-\d{2}-\d{2}/.test(String(item))) problems.push(`${where}.${key}: "${String(item)}" is not an ISO 8601 date`);
        else if (when > Date.now() + 24 * 3600 * 1000) problems.push(`${where}.${key}: "${String(item)}" is in the future`);
      }
    }
  }
}

/** Every `{ "@id": ... }` reference in `value`. */
function references(value: unknown, out: string[]): void {
  if (Array.isArray(value)) for (const v of value) references(v, out);
  else if (isNode(value)) {
    if (Object.keys(value).length === 1 && typeof value["@id"] === "string") out.push(value["@id"]);
    else for (const v of Object.values(value)) references(v, out);
  }
}

/** All problems with one page's JSON-LD. */
export function checkPageJsonld(page: Page): string[] {
  const problems: string[] = [];
  for (const [i, block] of page.jsonld.entries()) {
    if (block.error) problems.push(`JSON-LD block ${i + 1} does not parse: ${block.error}`);
    else if (block.data?.["@context"] !== "https://schema.org") problems.push(`JSON-LD block ${i + 1}: @context must be "https://schema.org"`);
  }
  const all = nodes(page);
  const ids = new Map<string, string>();
  for (const [i, node] of all.entries()) checkNode(node, `JSON-LD ${typesOf(node).join("/") || `node ${i + 1}`}`, problems, ids);

  // References resolve to a node on the page.
  const defined = new Set<string>();
  const collect = (value: unknown) => {
    if (Array.isArray(value)) value.forEach(collect);
    else if (isNode(value)) {
      if (typeof value["@id"] === "string" && Object.keys(value).length > 1) defined.add(value["@id"]);
      Object.values(value).forEach(collect);
    }
  };
  all.forEach(collect);
  const refs: string[] = [];
  references(all, refs);
  for (const ref of new Set(refs)) if (!defined.has(ref)) problems.push(`JSON-LD: a reference to ${ref}, which no node on the page defines`);

  // Breadcrumbs: positions run 1, 2, 3 ... and the last step is this page.
  for (const crumb of all.filter((n) => typesOf(n).includes("BreadcrumbList"))) {
    const items = list(crumb.itemListElement).filter(isNode);
    items.forEach((item, i) => {
      if (item.position !== i + 1) problems.push(`BreadcrumbList: step ${i + 1} has position ${String(item.position)}`);
    });
    const last = items.at(-1)?.item;
    if (page.url !== "/" && last !== page.canonical) problems.push(`BreadcrumbList: the last step is ${String(last)}, not ${page.canonical}`);
    if (items[0]?.item !== "https://navree.com/") problems.push(`BreadcrumbList: the first step is ${String(items[0]?.item)}, not the homepage`);
  }
  return problems;
}

/** The types a page's data must contain: the site graph, a page node and a breadcrumb. */
export function missingTypes(page: Page): string[] {
  const have = new Set(nodes(page).flatMap(typesOf));
  const need = ["Organization", "WebSite", "SoftwareApplication", "SoftwareSourceCode", "BreadcrumbList"];
  const missing = need.filter((t) => !have.has(t));
  if (!["WebPage", "CollectionPage", "AboutPage"].some((t) => have.has(t))) missing.push("WebPage, CollectionPage or AboutPage");
  return missing;
}
