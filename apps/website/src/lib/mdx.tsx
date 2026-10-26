import { evaluate } from "@mdx-js/mdx";
import rehypeShikiFromHighlighter from "@shikijs/rehype/core";
import {
  transformerMetaHighlight,
  transformerNotationDiff,
  transformerNotationHighlight,
} from "@shikijs/transformers";
import type { Element, Root } from "hast";
import { toString } from "hast-util-to-string";
import type { ReactElement } from "react";
import * as runtime from "react/jsx-runtime";
import rehypeSlug from "rehype-slug";
import remarkGfm from "remark-gfm";
import { createHighlighter, type Highlighter, type ShikiTransformer } from "shiki";
import { visit } from "unist-util-visit";

import { mdxComponents } from "@/components/mdx";
import { languageAliases, languages, navreeTheme } from "@/lib/shiki-theme";

export type Heading = { depth: 2 | 3; id: string; text: string };

let highlighter: Promise<Highlighter> | null = null;

/** One highlighter for the whole build: loading grammars is the slow part. */
export function getHighlighter(): Promise<Highlighter> {
  highlighter ??= createHighlighter({
    themes: [navreeTheme],
    langs: [...languages],
    langAlias: languageAliases,
  });
  return highlighter;
}

/**
 * Reads the fence's meta string: ```rust title="src/main.rs" {3-5}
 * puts the title and the language on the `<pre>` for the code block
 * component (`{3-5}` is handled by `transformerMetaHighlight`).
 */
const metaTransformer: ShikiTransformer = {
  name: "navree:meta",
  pre(node) {
    const raw = String((this.options.meta as { __raw?: string } | undefined)?.__raw ?? "");
    const title = /(?:title|filename)="([^"]+)"/.exec(raw)?.[1];
    if (title) node.properties["data-title"] = title;
    node.properties["data-language"] = this.options.lang;
  },
};

/** Collects the `##` and `###` headings (after rehype-slug gave them ids). */
function rehypeCollectHeadings(out: Heading[]) {
  return () => (tree: Root) => {
    visit(tree, "element", (node: Element) => {
      if ((node.tagName === "h2" || node.tagName === "h3") && node.properties?.id) {
        out.push({ depth: node.tagName === "h2" ? 2 : 3, id: String(node.properties.id), text: toString(node).trim() });
      }
    });
  };
}

/** Compiles and renders MDX at build time: highlighted code, heading ids and the headings list. */
export async function renderMdx(source: string): Promise<{ content: ReactElement; headings: Heading[] }> {
  const headings: Heading[] = [];
  const shiki = await getHighlighter();
  const { default: Content } = await evaluate(source, {
    ...runtime,
    remarkPlugins: [remarkGfm],
    rehypePlugins: [
      rehypeSlug,
      rehypeCollectHeadings(headings),
      [
        rehypeShikiFromHighlighter,
        shiki,
        {
          theme: "navree-night",
          defaultLanguage: "text",
          fallbackLanguage: "text",
          transformers: [metaTransformer, transformerMetaHighlight(), transformerNotationHighlight(), transformerNotationDiff()],
        },
      ],
    ],
  });
  return { content: <Content components={mdxComponents} />, headings };
}

/** Highlights one snippet outside MDX (the landing page's code tabs). */
export async function highlight(code: string, lang: string): Promise<string> {
  const shiki = await getHighlighter();
  return shiki.codeToHtml(code.trimEnd(), {
    lang,
    theme: "navree-night",
    transformers: [transformerNotationHighlight()],
  });
}
