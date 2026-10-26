import { highlight } from "@/lib/mdx";

import { ShowcaseTabs, type ShowcasePanel } from "./showcase-tabs";
import { showcases } from "./showcase-data";

/** Strips Shiki's `<pre>` wrapper so the panel can provide its own frame. */
function inner(html: string): string {
  return html.replace(/^<pre[^>]*>/, "").replace(/<\/pre>\s*$/, "");
}

/** The landing page's code tabs, highlighted at build time. */
export async function Showcase() {
  const panels: ShowcasePanel[] = await Promise.all(
    showcases.map(async (s) => ({
      id: s.id,
      label: s.label,
      file: s.file,
      title: s.title,
      text: s.text,
      href: s.href,
      linkText: s.linkText,
      html: inner(await highlight(s.code, "rust")),
      extra: s.extra ? { file: s.extra.file, html: inner(await highlight(s.extra.code, s.extra.lang)) } : undefined,
    })),
  );
  return <ShowcaseTabs panels={panels} />;
}
