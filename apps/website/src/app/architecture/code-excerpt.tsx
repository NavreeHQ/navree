import { transformerNotationDiff, transformerNotationHighlight } from "@shikijs/transformers";

import { CopyButton } from "@/components/mdx/copy-button";
import { getHighlighter } from "@/lib/mdx";
import { cn } from "@/lib/utils";

/**
 * A short, highlighted code excerpt in the docs' code-block frame: the file
 * name, a copy button and the code. `// [!code ++]` and `// [!code --]`
 * mark added and removed lines.
 */
export async function CodeExcerpt({
  file,
  lang,
  source,
  className,
}: {
  file: string;
  lang: "rust" | "toml" | "ts" | "sh";
  source: string;
  className?: string;
}) {
  const shiki = await getHighlighter();
  const html = shiki.codeToHtml(source.trimEnd(), {
    lang: lang === "ts" ? "typescript" : lang === "sh" ? "shellscript" : lang,
    theme: "navree-night",
    transformers: [transformerNotationDiff(), transformerNotationHighlight()],
  });
  return (
    <figure className={cn("code-block my-0 shadow-bezel", className)} data-copy-root>
      <figcaption className="code-block-bar">
        <span className="code-block-title">{file}</span>
        <CopyButton label={`Copy ${file}`} />
      </figcaption>
      {/* Shiki's own output, highlighted at build time. */}
      <div
        className="scroll-thin [&_pre]:text-[0.75rem] [&_pre]:leading-[1.7] sm:[&_pre]:text-[0.8125rem]"
        // Shiki's output for a string that lives in this repository, highlighted at build time
        // (Shiki escapes the code it highlights).
        // nosemgrep: navree-raw-html
        dangerouslySetInnerHTML={{ __html: html }}
      />
    </figure>
  );
}
