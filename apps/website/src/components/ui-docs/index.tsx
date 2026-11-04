/**
 * The UI docs' components, usable in MDX without an import:
 *
 * - `<UiPreview example="button/variants" />`: a live preview of an example
 *   rendered by navree-ui, with Preview and Code tabs.
 * - `<UiAnatomy component="button" />`: the component's `data-slot` parts.
 * - `<UiProps type="Button" />`: a type's constructors, methods or variants,
 *   read from the toolkit's source.
 * - `<UiChoose name="overlay" current="Dialog" />`: "which one should I use?".
 * - `<UiA11y>`: keyboard and accessibility notes, written in Markdown inside.
 * - `<UiGrid />`: every component as a small live preview, by group.
 * - `<UiAssets />`: what ships to the browser, measured from the files.
 * - `<UiTokens />`: the colour tokens as swatches.
 * - `<UiFlagWeights />`: what the flags weigh, from the committed artwork.
 *
 * Everything they show comes from `content/ui/`, which `cargo run -p ui-docs`
 * writes from the toolkit.
 */
import Link from "next/link";
import type { ReactNode } from "react";

import { DiagramFigure, type DiagramSpec } from "@/components/diagrams";
import type { DiagramEdge, DiagramNode } from "@/components/diagrams/kit";
import { ArrowRightIcon } from "@/components/icons";
import { InlineMarkdown } from "@/components/inline-markdown";
import { highlight } from "@/lib/mdx";
import { cn } from "@/lib/utils";

import { ownSlots } from "./anatomy";
import { choices, type Choose, type ChooseName } from "./choices";
import {
  asset,
  component as findComponent,
  components,
  example as findExample,
  groups,
  kb,
  startHeight,
  toolkitInfo,
  typeDoc,
  type Method,
  type Slot,
  type TypeDoc,
} from "./data";
import { LiveDemo } from "./live-demo";
import { PreviewCard } from "./preview-card";
import { PreviewFrame } from "./preview-frame";
import { flagWeightsText } from "./weights";

// ------------------------------------------------------------------ preview

/**
 * The height a whole page that fills its frame gets on a phone, where it is drawn at
 * the phone's own width and scrolls inside, rather than scaled to a picture too small
 * to read.
 */
const PHONE_HEIGHT = 640;

/**
 * `<UiPreview example="button/variants" />`. With `defer`, the frame loads only
 * after the page has, as it nears the screen: for a whole page drawn below the fold.
 * With `live`, an address under `/demo/ui/`, the frame shows that page of the live
 * demo app when the demo answers, and the example's picture when it doesn't.
 */
export async function UiPreview({
  example: id,
  defer = false,
  live,
}: {
  example: string;
  defer?: boolean;
  live?: string;
}) {
  const example = findExample(id);
  const html = await highlight(example.code, "rust");
  const owner = findComponent(example.component);
  if (live && !live.startsWith("/demo/ui/")) throw new Error(`UI docs: ${id}: a live preview is a page under /demo/ui/, not ${live}`);
  const title = `${owner.name}: ${example.title}, rendered by navree-ui`;
  const picture = (
    <PreviewFrame
      src={example.src}
      title={title}
      height={startHeight(example)}
      width={example.width}
      fit={example.fit}
      phoneHeight={example.fit ? undefined : PHONE_HEIGHT}
      defer={defer}
    />
  );
  return (
    <PreviewCard
      label={`${owner.name}: ${example.title}`}
      src={example.src}
      codeText={example.code}
      preview={live ? <LiveDemo live={live} title={title} picture={picture} /> : picture}
      code={<div dangerouslySetInnerHTML={{ __html: html }} />}
    />
  );
}

// ------------------------------------------------------------------ anatomy

function SlotBox({ slot, depth }: { slot: Slot; depth: number }) {
  return (
    <li
      className={cn(
        "ui-slot min-w-0 rounded-lg border p-2.5",
        depth === 0 ? "border-accent/60 bg-[rgb(61_255_143/0.04)]" : "border-line-2 bg-ink-2",
        depth > 0 && "border-dashed",
      )}
    >
      <div className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
        <code className={cn("font-mono text-[0.8125rem]", depth === 0 ? "text-accent" : "text-fg")}>{slot.slot}</code>
        <span className="font-mono text-xs text-fg-3">
          &lt;{slot.tag}&gt;{slot.count > 1 ? ` × ${slot.count}` : ""}
        </span>
      </div>
      {slot.children.length ? (
        <ul className={cn("mt-2.5 flex flex-col gap-2", slot.children.length > 1 && "sm:flex-row sm:flex-wrap")}>
          {slot.children.map((child, i) => (
            <SlotBox key={`${child.slot}-${i}`} slot={child} depth={depth + 1} />
          ))}
        </ul>
      ) : null}
    </li>
  );
}

function slotNames(slots: Slot[], out = new Set<string>()): Set<string> {
  for (const s of slots) {
    out.add(s.slot);
    slotNames(s.children, out);
  }
  return out;
}

/** `<UiAnatomy component="button" />`. */
export function UiAnatomy({ component: id }: { component: string }) {
  const owner = findComponent(id);
  if (!owner.anatomy) throw new Error(`UI docs: ${id} has no anatomy example`);
  const parts = ownSlots(findExample(owner.anatomy).slots, id);
  const names = [...slotNames(parts)];
  return (
    <figure className="not-prose my-7 overflow-hidden rounded-2xl border border-line bg-ink-1 shadow-bezel">
      <div className="relative p-4 sm:p-6">
        <div
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle,var(--color-line-2)_1px,transparent_1.25px)] [mask-image:radial-gradient(ellipse_at_center,black_35%,transparent_85%)] bg-size-[16px_16px] opacity-50"
        />
        <ul className="relative flex flex-col gap-2" aria-label={`The parts of ${owner.name}`}>
          {parts.map((slot, i) => (
            <SlotBox key={`${slot.slot}-${i}`} slot={slot} depth={0} />
          ))}
        </ul>
      </div>
      <figcaption className="flex flex-wrap items-center justify-between gap-x-6 gap-y-2 border-t border-line px-4 py-3">
        <span className="text-[0.8125rem] font-medium text-fg-2">The parts of {owner.name}</span>
        <span className="text-xs text-fg-3">
          {names.length} {names.length === 1 ? "part" : "parts"}, each an element with <code className="font-mono">data-slot</code>
        </span>
      </figcaption>
    </figure>
  );
}

// -------------------------------------------------------------------- props

function signature(owner: string, m: Method, constructor: boolean): string {
  return constructor ? `${owner}::${m.name}${m.generics}(${m.args})` : `.${m.name}${m.generics}(${m.args})`;
}

function MethodRows({ owner, methods, constructor }: { owner: string; methods: Method[]; constructor: boolean }) {
  return methods.map((m) => (
    <tr key={`${constructor ? "c" : "m"}-${m.name}`}>
      <td className="pr-3!">
        <code className="ui-sig">{signature(owner, m, constructor)}</code>
      </td>
      <td>
        <InlineMarkdown text={m.doc} />
      </td>
    </tr>
  ));
}

function TypeTable({ doc }: { doc: TypeDoc }) {
  if (doc.kind === "enum") {
    return (
      <div className="table-wrap ui-props" role="region" aria-label={`${doc.name} variants`} tabIndex={0}>
        <table>
          <thead>
            <tr>
              <th scope="col">Variant</th>
              <th scope="col">Means</th>
            </tr>
          </thead>
          <tbody>
            {doc.variants.map((v) => (
              <tr key={v.name}>
                <td className="pr-3!">
                  <code className="ui-sig">
                    {doc.name}::{v.name}
                  </code>
                  {v.default ? <span className="ml-2 text-xs text-fg-3">default</span> : null}
                </td>
                <td>
                  <InlineMarkdown text={v.doc} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    );
  }
  return (
    <div className="table-wrap ui-props" role="region" aria-label={`${doc.name} API`} tabIndex={0}>
      <table>
        <thead>
          <tr>
            <th scope="col">{doc.constructors.length ? "Constructor or method" : "Method"}</th>
            <th scope="col">Does</th>
          </tr>
        </thead>
        <tbody>
          <MethodRows owner={doc.name} methods={doc.constructors} constructor />
          <MethodRows owner={doc.name} methods={doc.methods} constructor={false} />
        </tbody>
      </table>
    </div>
  );
}

/**
 * `<UiProps type="Button" />`: the type's summary and its table, read from
 * the toolkit's source.
 */
export function UiProps({ type }: { type: string }) {
  const doc = typeDoc(type);
  return (
    <div className="ui-props-block my-6">
      <p className="my-0! text-[0.9375rem] text-fg-2">
        <InlineMarkdown text={doc.summary} />{" "}
        <span className="text-fg-3">
          Defined in <code className="inline-code">{doc.file.replace("crates/navree-ui/src/", "")}</code>.
        </span>
      </p>
      <TypeTable doc={doc} />
      {doc.shared.length ? (
        <p className="-mt-3! text-sm text-fg-3">
          Also, as on every component:{" "}
          {doc.shared.map((m, i) => (
            <span key={m.name}>
              {i ? ", " : ""}
              <code className="inline-code">
                .{m.name}({m.args})
              </code>
            </span>
          ))}
          .
        </p>
      ) : null}
    </div>
  );
}

// ------------------------------------------------------------------ choose

function chooseSpec(spec: Choose, current?: string): DiagramSpec {
  for (const o of spec.options) {
    if (o.when.length > 34) throw new Error(`UI docs: "${o.when}" is longer than 34 characters`);
  }
  if (spec.question.length > 24) throw new Error(`UI docs: "${spec.question}" is longer than 24 characters`);
  const n = spec.options.length;
  const tone = (name: string) => (name === current ? "accent" : "default") as DiagramNode["tone"];
  // Wide: the question on the left, the answers stacked on the right.
  const wideHeight = n * 72 - 16;
  const wideNodes: DiagramNode[] = [
    { id: "q", x: 0, y: Math.round((wideHeight - 40) / 2 / 8) * 8, w: 208, h: 40, title: spec.question },
    ...spec.options.map((o, i) => ({
      id: `o${i}`,
      x: 328,
      y: i * 72,
      w: 264,
      title: o.name,
      mono: o.mono,
      sub: o.when,
      tone: tone(o.name),
    })),
  ];
  const wideEdges: DiagramEdge[] = spec.options.map((_, i) => ({ from: "q.right", to: `o${i}.left`, bend: 264 }));
  // Narrow: the question on top, the answers below a line down the left.
  const narrowNodes: DiagramNode[] = [
    { id: "q", x: 0, y: 0, w: 272, h: 40, title: spec.question },
    ...spec.options.map((o, i) => ({
      id: `o${i}`,
      x: 32,
      y: 64 + i * 72,
      w: 240,
      title: o.name,
      mono: o.mono,
      sub: o.when,
      tone: tone(o.name),
    })),
  ];
  const narrowEdges: DiagramEdge[] = spec.options.map((_, i) => ({
    from: "q.bottom",
    to: `o${i}.left`,
    via: [
      [136, 52],
      [12, 52],
      [12, 64 + i * 72 + 28],
    ],
  }));
  const desc = `${spec.question} ${spec.options.map((o) => `${o.name}: ${o.when}.`).join(" ")}`;
  return {
    title: spec.title,
    desc,
    legend: [],
    wide: { width: 592, height: wideHeight, nodes: wideNodes, edges: wideEdges },
    narrow: { width: 272, height: 64 + n * 72 - 16, nodes: narrowNodes, edges: narrowEdges },
  };
}

/** `<UiChoose name="overlay" current="Dialog" />`: which one should I use? */
export function UiChoose({ name, current }: { name: ChooseName; current?: string }) {
  const spec: Choose = choices[name];
  if (!spec) throw new Error(`UI docs: no selection diagram "${name}"`);
  return (
    <div className="not-prose my-7">
      <DiagramFigure spec={chooseSpec(spec, current)} name={`choose-${name}`} maxScale={1.15} />
      <ul className="mt-3 flex flex-wrap gap-x-4 gap-y-1.5 px-1 text-sm" aria-label="Go to">
        {spec.options
          .filter((o) => o.name !== current)
          .map((o) => (
            <li key={o.name}>
              <Link href={o.href} className="group inline-flex items-center gap-1 text-fg-2 transition-colors hover:text-fg">
                <span className={o.mono ? "font-mono text-[0.8125rem]" : undefined}>{o.name}</span>
                <ArrowRightIcon className="size-3.5 text-fg-3 transition-transform group-hover:translate-x-0.5" />
              </Link>
            </li>
          ))}
      </ul>
    </div>
  );
}

// --------------------------------------------------------------- a11y notes

/** `<UiA11y>` with Markdown inside: the keyboard table and the notes. */
export function UiA11y({ children }: { children?: ReactNode }) {
  return <div className="ui-a11y my-7 rounded-2xl border border-line bg-ink-1 px-5 py-1 shadow-bezel">{children}</div>;
}

// --------------------------------------------------------------------- grid

/** `<UiGrid />`: every component as a small live preview, by group. */
export function UiGrid() {
  return (
    <div className="not-prose">
      {groups.map((group) => (
        <section key={group.id} className="mt-10 first:mt-6" aria-labelledby={`ui-group-${group.id}`}>
          <h2 id={`ui-group-${group.id}`} className="font-sans text-[1.0625rem] font-semibold text-fg">
            <a href={`#ui-group-${group.id}`} className="heading-anchor">
              {group.title}
            </a>
          </h2>
          <ul className="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {group.components.map((cid) => {
              const owner = findComponent(cid);
              const thumb = findExample(owner.thumb);
              return (
                <li key={cid} className="min-w-0">
                  <Link
                    href={owner.url}
                    className="group flex h-full flex-col overflow-hidden rounded-2xl border border-line bg-ink-1 shadow-bezel transition-colors duration-200 hover:border-line-2"
                  >
                    <div className="relative h-[9.5rem] overflow-hidden border-b border-line bg-ink-0" inert>
                      <PreviewFrame
                        src={thumb.src}
                        title=""
                        height={thumb.height}
                        width={thumb.width}
                        fit={false}
                        thumb
                        className={thumb.width ? undefined : "h-full"}
                      />
                    </div>
                    <div className="flex flex-1 flex-col gap-1 px-4 py-3">
                      <span className="flex items-center justify-between gap-2 font-medium text-fg">
                        {owner.name}
                        <ArrowRightIcon className="size-4 shrink-0 text-fg-3 transition-[transform,color] duration-300 group-hover:translate-x-0.5 group-hover:text-accent" />
                      </span>
                      <span className="line-clamp-2 text-[0.875rem] leading-snug text-fg-3">
                        <InlineMarkdown text={owner.summary} />
                      </span>
                    </div>
                  </Link>
                </li>
              );
            })}
          </ul>
        </section>
      ))}
    </div>
  );
}

/** Every component's name and page, for the Markdown copy of the grid. */
export const gridEntries = () => components.filter((c) => c.group);

// ------------------------------------------------------------------- assets

const loadedWhen: Record<string, string> = {
  "navree-ui": "Every page",
  core: "A form, a region or a command",
  overlay: "A popover, tooltip or hover card",
  sidebar: "The app shell",
  table: "A data table",
};

/** `<UiFlagWeights />`: the weights of the flags, written from the measured numbers. */
export function UiFlagWeights() {
  return <p>{flagWeightsText(toolkitInfo.flag_weights)}</p>;
}

/** `<UiAssets />`: the toolkit's files, measured from the files themselves. */
export function UiAssets() {
  const rows = ["navree-ui", "core", "overlay", "sidebar", "table"].map((name) => asset(name));
  return (
    <div className="table-wrap" role="region" aria-label="What ships to the browser" tabIndex={0}>
      <table>
        <thead>
          <tr>
            <th scope="col">File</th>
            <th scope="col">Loaded by</th>
            <th scope="col" className="text-right">
              Brotli
            </th>
            <th scope="col" className="text-right">
              Gzip
            </th>
            <th scope="col" className="text-right">
              Raw
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((a) => (
            <tr key={a.name}>
              <td>
                <code className="inline-code">
                  {a.name}.{a.ext}
                </code>
              </td>
              <td>{loadedWhen[a.name]}</td>
              <td className="numeric text-right">{kb(a.brotli)}</td>
              <td className="numeric text-right">{kb(a.gzip)}</td>
              <td className="numeric text-right text-fg-3">{kb(a.bytes)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** The content security policy every page carries, one directive per line. */
export async function UiCsp() {
  const html = await highlight(toolkitInfo.csp.split("; ").join(";\n"), "text");
  return (
    <figure className="code-block" data-copy-root>
      <figcaption className="code-block-bar">
        <span className="code-block-title">Content-Security-Policy</span>
      </figcaption>
      <div dangerouslySetInnerHTML={{ __html: html }} />
    </figure>
  );
}

// ------------------------------------------------------------------- tokens

const tokenUse: Record<string, string> = {
  "ink-0": "The page",
  "ink-1": "Cards, the sidebar, dialogs",
  "ink-2": "Hover, inputs, chips",
  "ink-3": "Active items, tooltips",
  line: "Every 1px border",
  "line-2": "Control borders",
  "line-3": "Strong borders, scrollbars",
  fg: "Headings, active text",
  "fg-1": "Body text",
  "fg-2": "Secondary text",
  "fg-3": "Captions, labels",
  accent: "The main action, focus, good news",
  "accent-ink": "Text on the accent",
  amber: "Warnings",
  danger: "Errors, destructive actions",
  bar: "Chart bars",
};

/** `<UiTokens />`: the colour tokens, from `navree_ui::tokens`. */
export function UiTokens() {
  const colours = toolkitInfo.tokens.filter((t) => t.value.startsWith("#"));
  return (
    <ul className="not-prose my-7 grid gap-2 sm:grid-cols-2" aria-label="Colour tokens">
      {colours.map((t) => (
        <li key={t.name} className="flex items-center gap-3 rounded-xl border border-line bg-ink-1 p-2.5">
          <span
            aria-hidden="true"
            className="size-10 shrink-0 rounded-md border border-line-2"
            style={{ background: t.value }}
          />
          <span className="flex min-w-0 flex-col">
            <span className="flex items-baseline gap-2">
              <code className="font-mono text-[0.8125rem] text-fg">--{t.name}</code>
              <span className="font-mono text-xs text-fg-3">{t.value}</span>
            </span>
            <span className="truncate text-[0.8125rem] text-fg-3">{tokenUse[t.name] ?? ""}</span>
          </span>
        </li>
      ))}
    </ul>
  );
}
