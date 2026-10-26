import { Children, Fragment, isValidElement, type ReactElement, type ReactNode } from "react";

import { TabsShell } from "./tabs-shell";

type TabProps = { label: string; children?: ReactNode };

/** One panel of `<Tabs>`. */
export function Tab({ children }: TabProps) {
  return <div className="tab-content">{children}</div>;
}

function elements(children: ReactNode): ReactElement[] {
  return Children.toArray(children).filter(isValidElement);
}

/** Shiki hands each highlighted block over wrapped in a fragment; this finds the `<pre>`. */
function unwrap(element: ReactElement): ReactElement {
  let current = element;
  while (current.type === Fragment) {
    const inner = elements((current.props as { children?: ReactNode }).children)[0];
    if (!inner) break;
    current = inner;
  }
  return current;
}

/**
 * `<Tabs>` with `<Tab label="...">` children: alternatives the reader picks
 * from (Cargo or the CLI, SQLite or PostgreSQL). Markdown and code blocks
 * work inside each tab.
 */
export function Tabs({ children }: { children?: ReactNode }) {
  const tabs = elements(children);
  const labels = tabs.map((tab, i) => String((tab.props as Partial<TabProps>).label ?? `Tab ${i + 1}`));
  return <TabsShell labels={labels}>{tabs}</TabsShell>;
}

/**
 * `<CodeGroup>` around several fenced code blocks: one framed block with a
 * tab per file. Each tab is named after its block's `title="..."` (or its
 * language when it has none).
 */
export function CodeGroup({ children }: { children?: ReactNode }) {
  const blocks = elements(children);
  const labels = blocks.map((block, i) => {
    const props = unwrap(block).props as { "data-title"?: string; "data-language"?: string };
    return props["data-title"] ?? props["data-language"] ?? `Example ${i + 1}`;
  });
  return (
    <TabsShell labels={labels} variant="code">
      {blocks}
    </TabsShell>
  );
}
