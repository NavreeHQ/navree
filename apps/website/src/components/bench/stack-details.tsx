import { ChevronIcon, DownloadIcon, ExternalIcon } from "@/components/icons";
import type { Framework } from "@/lib/bench-types";
import { repoBlob, repoTree } from "@site";

/**
 * One disclosure per implementation: its stack as recorded in its README
 * (`bench/frameworks/<dir>/README.md`), with links to the source, the README
 * and the raw results. The highlighted framework starts open.
 */
export function StackDetails({ frameworks, highlight }: { frameworks: Framework[]; highlight: string }) {
  return (
    <div className="stack-list">
      {frameworks.map((f) => (
        <details key={f.name} className="stack-item" open={f.name === highlight}>
          <summary className="stack-summary">
            <ChevronIcon className="stack-chevron" />
            <span className="min-w-0 flex-1">
              <span className={`font-medium ${f.name === highlight ? "text-accent" : "text-fg"}`}>{f.label}</span>
              <span className="ml-2 text-sm text-fg-3">
                {f.language}, {f.category === "micro" ? "micro-framework" : "full-stack"}
              </span>
            </span>
            <span className="hidden text-sm text-fg-3 sm:inline">
              {f.conformance.passed !== null
                ? `${f.conformance.passed}/${(f.conformance.passed ?? 0) + (f.conformance.failed ?? 0)} checks`
                : f.conformance.ok
                  ? "conformance passed"
                  : "conformance failed"}
            </span>
          </summary>
          <div className="stack-body">
            {f.stack.length ? (
              <table className="stack-table">
                <caption className="sr-only">{f.label}: the stack</caption>
                <tbody>
                  {f.stack.map((row) => (
                    <tr key={row.label}>
                      <th scope="row">{row.label}</th>
                      {/* README cells are escaped at build time; only code, bold and links survive. */}
                      <td dangerouslySetInnerHTML={{ __html: row.html }} />
                    </tr>
                  ))}
                </tbody>
              </table>
            ) : (
              <p className="text-sm text-fg-2">The README has no stack table.</p>
            )}
            {f.note ? <p className="mt-4 text-sm leading-relaxed text-fg-2">{f.note}</p> : null}
            <div className="mt-5 flex flex-wrap gap-x-6 gap-y-2 text-sm">
              <a href={repoTree(f.sourceDir)} className="inline-flex items-center gap-1.5 text-fg-2 hover:text-fg">
                Source <ExternalIcon className="size-3.5" />
              </a>
              <a href={repoBlob(`${f.sourceDir}/README.md`)} className="inline-flex items-center gap-1.5 text-fg-2 hover:text-fg">
                README and judgement calls <ExternalIcon className="size-3.5" />
              </a>
              <a href={f.rawHref} download className="inline-flex items-center gap-1.5 text-fg-2 hover:text-fg">
                Raw results <DownloadIcon className="size-3.5" />
              </a>
            </div>
          </div>
        </details>
      ))}
    </div>
  );
}
