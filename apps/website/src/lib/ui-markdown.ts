/**
 * The UI docs' components as Markdown, for the docs' Markdown copies and
 * `llms-full.txt` (see `agent-markdown.ts`). Reads the same files the pages
 * do, in `content/ui/`, which `cargo run -p ui-docs` writes from navree-ui.
 *
 * Node runs this with the types stripped: relative `.ts` imports only.
 */
import fs from "node:fs";
import path from "node:path";

import { choices, type Choose } from "../components/ui-docs/choices.ts";

type Slot = { slot: string; tag: string; count: number; children: Slot[] };
type Example = { id: string; title: string; code: string; slots: Slot[]; captions: string[] };
type Component = { id: string; name: string; url: string; summary: string; group: string | null; anatomy: string | null };
type Group = { id: string; title: string; components: string[] };
type Method = { name: string; generics: string; args: string; doc: string };
type TypeDoc = {
  name: string;
  kind: "struct" | "enum";
  summary: string;
  constructors: Method[];
  methods: Method[];
  variants: { name: string; doc: string; default: boolean }[];
  shared: Method[];
};
type Toolkit = {
  csp: string;
  assets: { name: string; ext: string; bytes: number; gzip: number | null; brotli: number | null }[];
  tokens: { name: string; value: string }[];
};

const dir = () => path.join(process.cwd(), "content", "ui");
const read = <T>(file: string): T => JSON.parse(fs.readFileSync(path.join(dir(), file), "utf8")) as T;

let cached: { examples: Example[]; components: Component[]; groups: Group[]; api: Record<string, TypeDoc>; toolkit: Toolkit } | null =
  null;

function data() {
  if (!cached) {
    const catalog = read<{ examples: Example[]; components: Component[]; groups: Group[] }>("catalog.json");
    cached = { ...catalog, api: read("api.json"), toolkit: read("toolkit.json") };
  }
  return cached;
}

const cell = (text: string) => text.replace(/\|/g, "\\|").replace(/\n/g, " ");

/** `<UiPreview example>`: the example's Rust. */
export function previewMarkdown(id: string): string {
  const example = data().examples.find((e) => e.id === id);
  if (!example) return "";
  return `\`\`\`rust title="${example.title}"\n${example.code}\n\`\`\``;
}

/** `<UiProps type>`: the type's summary and table. */
export function propsMarkdown(type: string): string {
  const doc = data().api[type];
  if (!doc) return "";
  const rows: string[] = [];
  if (doc.kind === "enum") {
    rows.push("| Variant | Means |", "|---|---|");
    for (const v of doc.variants) rows.push(`| \`${doc.name}::${v.name}\`${v.default ? " (default)" : ""} | ${cell(v.doc)} |`);
  } else {
    rows.push("| Constructor or method | Does |", "|---|---|");
    for (const m of doc.constructors) rows.push(`| \`${cell(`${doc.name}::${m.name}${m.generics}(${m.args})`)}\` | ${cell(m.doc)} |`);
    for (const m of doc.methods) rows.push(`| \`${cell(`.${m.name}${m.generics}(${m.args})`)}\` | ${cell(m.doc)} |`);
  }
  const shared = doc.shared.length
    ? `\n\nAlso, as on every component: ${doc.shared.map((m) => `\`.${m.name}(${m.args})\``).join(", ")}.`
    : "";
  return `${doc.summary}\n\n${rows.join("\n")}${shared}`;
}

const LAYOUT = new Set(["row", "stack", "grid", "container"]);

function slotLines(slots: Slot[], depth: number, out: string[]): string[] {
  for (const s of slots) {
    out.push(`${"  ".repeat(depth)}- \`${s.slot}\` (\`<${s.tag}>\`)${s.count > 1 && depth > 0 ? ` × ${s.count}` : ""}`);
    slotLines(s.children, depth + 1, out);
  }
  return out;
}

/** `<UiAnatomy component>`: the parts as a nested list. */
export function anatomyMarkdown(id: string): string {
  const component = data().components.find((c) => c.id === id);
  const example = data().examples.find((e) => e.id === component?.anatomy);
  if (!component || !example) return "";
  let parts = example.slots;
  while (id !== "layout" && parts.length && parts.every((p) => LAYOUT.has(p.slot))) parts = parts.flatMap((p) => p.children);
  return `The parts of ${component.name}, each an element with \`data-slot\`:\n\n${slotLines(parts, 0, []).join("\n")}`;
}

/** `<UiChoose name>`: the question and its answers. */
export function chooseMarkdown(name: string): string {
  const spec = (choices as Record<string, Choose>)[name];
  if (!spec) return "";
  return `*${spec.title}. ${spec.question}*\n\n${spec.options.map((o) => `- [${o.name}](${o.href}): ${o.when}.`).join("\n")}`;
}

/** `<UiGrid>`: every component, by group. */
export function gridMarkdown(): string {
  const { groups, components } = data();
  return groups
    .map((g) => {
      const items = g.components
        .map((id) => components.find((c) => c.id === id))
        .filter((c): c is Component => Boolean(c))
        .map((c) => `- [${c.name}](${c.url}): ${c.summary}`);
      return `### ${g.title}\n\n${items.join("\n")}`;
    })
    .join("\n\n");
}

const kb = (n: number | null) => (n == null ? "n/a" : `${(n / 1024).toFixed(1)} KB`);

/** `<UiAssets>`: the toolkit's files and their sizes. */
export function assetsMarkdown(): string {
  const rows = ["| File | Brotli | Gzip | Raw |", "|---|---:|---:|---:|"];
  for (const name of ["navree-ui", "core", "overlay", "sidebar", "table"]) {
    const a = data().toolkit.assets.find((x) => x.name === name);
    if (a) rows.push(`| \`${a.name}.${a.ext}\` | ${kb(a.brotli)} | ${kb(a.gzip)} | ${kb(a.bytes)} |`);
  }
  return rows.join("\n");
}

/** `<UiCsp>`: the policy. */
export function cspMarkdown(): string {
  return `\`\`\`text\n${data().toolkit.csp.split("; ").join(";\n")}\n\`\`\``;
}

/** `<UiTokens>`: the colours. */
export function tokensMarkdown(): string {
  const rows = ["| Token | Value |", "|---|---|"];
  for (const t of data().toolkit.tokens) if (t.value.startsWith("#")) rows.push(`| \`--${t.name}\` | \`${t.value}\` |`);
  return rows.join("\n");
}
