import { transformerNotationDiff, transformerNotationHighlight } from "@shikijs/transformers";

import { CopyButton } from "@/components/mdx/copy-button";
import { getHighlighter } from "@/lib/mdx";
import { cn } from "@/lib/utils";

/**
 * Real code from the CRM, highlighted at build time, in the docs'
 * code-block frame: the file name, a copy button and the code.
 * `// [!code ++]` marks an added line.
 */
export async function CodePanel({
  file,
  lang,
  source,
  className,
  wrap = false,
}: {
  file: string;
  lang: "rust" | "ts" | "sh" | "toml";
  source: string;
  className?: string;
  /** Wrap long lines with a hanging indent instead of scrolling them. */
  wrap?: boolean;
}) {
  const shiki = await getHighlighter();
  const html = shiki.codeToHtml(source.trimEnd(), {
    lang: lang === "ts" ? "typescript" : lang === "sh" ? "shellscript" : lang,
    theme: "navree-night",
    transformers: [transformerNotationDiff(), transformerNotationHighlight()],
  });
  const body = wrap ? html.replace("<pre ", "<pre data-wrap ") : html;
  return (
    <figure className={cn("code-block my-0 min-w-0 shadow-bezel", className)} data-copy-root>
      <figcaption className="code-block-bar">
        <span className="code-block-title truncate">{file}</span>
        <CopyButton label={`Copy ${file}`} />
      </figcaption>
      {/* Shiki's own output, highlighted at build time. */}
      <div
        className="scroll-thin [&_pre]:text-[0.75rem] [&_pre]:leading-[1.7] sm:[&_pre]:text-[0.8125rem]"
        dangerouslySetInnerHTML={{ __html: body }}
      />
    </figure>
  );
}

/** One copyable shell command on a single line, with a `$` prompt that isn't copied. */
export function CommandLine({ command, comment, className }: { command: string; comment?: string; className?: string }) {
  return (
    <div
      className={cn(
        "flex min-h-11 min-w-0 items-center gap-1 rounded-[1.375rem] border border-line bg-ink-0 py-1.5 pr-1 pl-4",
        className,
      )}
    >
      {/* Wraps at spaces on a phone rather than hiding the end of the command. */}
      <code className="min-w-0 flex-1 font-mono text-xs leading-relaxed text-fg-1 sm:text-[0.8125rem]">
        <span className="text-fg-3 select-none">$ </span>
        {command}
        {comment ? <span className="text-fg-3 select-none">{`  # ${comment}`}</span> : null}
      </code>
      <CopyButton text={command} label={`Copy ${command}`} />
    </div>
  );
}
