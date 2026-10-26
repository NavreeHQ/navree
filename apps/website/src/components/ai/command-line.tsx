import { CopyButton } from "@/components/mdx/copy-button";
import { cn } from "@/lib/utils";

/**
 * One command (or URL) in a mono field with a copy button. `prompt` is the
 * symbol in front: `$` for a shell command, nothing for a URL.
 */
export function CommandLine({
  command,
  prompt = "$",
  label = "Copy command",
  className,
}: {
  command: string;
  prompt?: string;
  label?: string;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex min-h-11 min-w-0 items-center gap-2.5 rounded-xl border border-line bg-ink-1 py-1.5 pr-1.5 pl-4 font-mono text-[0.8125rem] shadow-bezel",
        className,
      )}
    >
      {prompt ? (
        <span aria-hidden className="text-fg-3 select-none">
          {prompt}
        </span>
      ) : null}
      {/* Long commands wrap on narrow screens rather than hide their end. */}
      <code className="min-w-0 flex-1 leading-snug text-fg [overflow-wrap:anywhere]">
        {command}
      </code>
      <CopyButton text={command} label={label} className="shrink-0" />
    </div>
  );
}
