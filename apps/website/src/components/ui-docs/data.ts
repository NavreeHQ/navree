/**
 * The UI docs' data, written by `cargo run -p ui-docs` into `content/ui/`
 * from navree-ui itself: every example with its Rust code and parts, the
 * props tables read from the toolkit's source, and the toolkit's files and
 * tokens. `cargo test -p ui-docs` fails when these files are stale.
 *
 * Server components only: the files are read at build time and never ship to
 * the browser.
 */
import api from "@content/ui/api.json";
import catalog from "@content/ui/catalog.json";
import toolkit from "@content/ui/toolkit.json";

export type Slot = { slot: string; tag: string; count: number; children: Slot[] };

export type UiExample = {
  id: string;
  component: string;
  slug: string;
  title: string;
  kind: "single" | "gallery";
  code: string;
  src: string;
  height: number;
  layout: "center" | "start" | "fill";
  cells: string;
  show: boolean;
  live: boolean;
  width: number | null;
  scripts: string[];
  captions: string[];
  slots: Slot[];
};

export type UiComponent = {
  id: string;
  name: string;
  url: string;
  summary: string;
  group: string | null;
  types: string[];
  thumb: string;
  anatomy: string | null;
  examples: string[];
};

export type UiGroup = { id: string; title: string; components: string[] };

export type Method = { name: string; generics: string; args: string; doc: string };
export type Variant = { name: string; doc: string; default: boolean };
export type TypeDoc = {
  name: string;
  kind: "struct" | "enum";
  file: string;
  summary: string;
  constructors: Method[];
  methods: Method[];
  variants: Variant[];
  shared: Method[];
};

export type ToolkitAsset = {
  name: string;
  ext: string;
  file: string;
  source: string;
  bytes: number;
  gzip: number | null;
  brotli: number | null;
};

export const groups = catalog.groups as UiGroup[];
export const components = catalog.components as UiComponent[];
const examples = new Map((catalog.examples as UiExample[]).map((e) => [e.id, e]));
const types = api as unknown as Record<string, TypeDoc>;

export const toolkitInfo = toolkit as {
  build: string;
  csp: string;
  mount: string;
  assets: ToolkitAsset[];
  tokens: { name: string; value: string }[];
  icons: number;
};

/** An example by `component/slug`; a typo fails the build. */
export function example(id: string): UiExample {
  const found = examples.get(id);
  if (!found) throw new Error(`UI docs: no example "${id}" in content/ui/catalog.json (run \`cargo run -p ui-docs\`)`);
  return found;
}

export function component(id: string): UiComponent {
  const found = components.find((c) => c.id === id);
  if (!found) throw new Error(`UI docs: no component "${id}" in content/ui/catalog.json`);
  return found;
}

export function typeDoc(name: string): TypeDoc {
  const found = types[name];
  if (!found) throw new Error(`UI docs: no type "${name}" in content/ui/api.json`);
  return found;
}

export function asset(name: string): ToolkitAsset {
  const found = toolkitInfo.assets.find((a) => a.name === name);
  if (!found) throw new Error(`UI docs: no toolkit asset "${name}"`);
  return found;
}

/** `6.3 KB` from bytes. */
export function kb(bytes: number | null | undefined): string {
  if (bytes == null) return "n/a";
  return `${(bytes / 1024).toFixed(1)} KB`;
}
