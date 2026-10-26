import { highlight } from "@/lib/mdx";

import { HomeSection, SectionHeading } from "./parts";
import { showcases } from "./showcase-data";
import { ShowcaseTabs, type ShowcasePanel } from "./showcase-tabs";

/** Strips Shiki's `<pre>` wrapper so the panel can provide its own frame. */
function inner(html: string): string {
  return html.replace(/^<pre[^>]*>/, "").replace(/<\/pre>\s*$/, "");
}

/** The code section: real code, highlighted at build time. */
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
  return (
    <HomeSection id="code">
      <SectionHeading
        id="code"
        eyebrow="The code"
        title="Every Navree app reads the same way."
        lead="Modules state their boundaries, routes state who may call them, and types carry everything else. Most of this code comes straight from the reference CRM in the repository."
      />
      <div className="mt-12 lg:mt-14">
        <ShowcaseTabs panels={panels} />
      </div>
    </HomeSection>
  );
}
