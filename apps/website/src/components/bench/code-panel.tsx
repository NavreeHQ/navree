import { CopyButton } from "@/components/mdx/copy-button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

export type CodePane = {
  id: string;
  /** The tab label. */
  label: string;
  /** The caption over the code: a file name or a request line. */
  caption: string;
  /** Highlighted at build time (Shiki), without its `<pre>`. */
  html: string;
  note?: string;
};

/**
 * Code in a double-bezel frame with line tabs over it: the benchmark app's
 * source, a response, a request. Each pane copies.
 */
export function CodePanel({ panes, label }: { panes: CodePane[]; label: string }) {
  return (
    <div className="bezel">
      <div className="bezel-core">
        <Tabs defaultValue={panes[0].id} className="gap-0">
          <div className="scroll-thin overflow-x-auto border-b border-line px-3 [scrollbar-width:none]">
            <TabsList variant="line" aria-label={label} className="h-11">
              {panes.map((p) => (
                <TabsTrigger key={p.id} value={p.id} className="font-mono text-[0.8125rem] font-normal">
                  {p.label}
                </TabsTrigger>
              ))}
            </TabsList>
          </div>
          {panes.map((p) => (
            <TabsContent key={p.id} value={p.id}>
              <figure className="m-0" data-copy-root>
                <figcaption className="flex items-center justify-between gap-4 border-b border-line py-1 pr-1.5 pl-5">
                  <span className="truncate font-mono text-[0.75rem] text-fg-3">{p.caption}</span>
                  <CopyButton />
                </figcaption>
                <div className="code-block my-0 rounded-none border-0 bg-transparent">
                  <pre
                    className="shiki navree-night scroll-thin max-h-[30rem] overflow-y-auto"
                    tabIndex={0}
                    dangerouslySetInnerHTML={{ __html: p.html }}
                  />
                </div>
                {p.note ? <p className="border-t border-line px-5 py-3 text-xs leading-relaxed text-fg-3">{p.note}</p> : null}
              </figure>
            </TabsContent>
          ))}
        </Tabs>
      </div>
    </div>
  );
}
