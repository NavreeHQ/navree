import Link from "next/link";
import type { ComponentProps, ReactNode } from "react";

import { Badge } from "./badge";
import { Callout } from "./callout";
import { CodeBlock } from "./code-block";
import { Diagram } from "./diagram";
import { FileTree } from "./file-tree";
import { LinkCard, LinkCards } from "./link-card";
import { Step, Steps } from "./steps";
import { CodeGroup, Tab, Tabs } from "./tabs";
import { UiA11y, UiAnatomy, UiAssets, UiChoose, UiCsp, UiFlagWeights, UiGrid, UiPreview, UiProps, UiTokens } from "@/components/ui-docs";

/**
 * Headings link to themselves (the whole heading is the anchor link), so
 * readers can copy a link to any section.
 */
function heading(Tag: "h2" | "h3" | "h4") {
  return function Heading({ id, children }: { id?: string; children?: ReactNode }) {
    if (!id) return <Tag>{children}</Tag>;
    return (
      <Tag id={id}>
        <a href={`#${id}`} className="heading-anchor">
          {children}
        </a>
      </Tag>
    );
  };
}

/** A root-relative link to a file (`/llms.txt`, `/docs/x.md`), not a page: the router can't prefetch it. */
const isFile = (href: string) => /\.[a-z0-9]+$/i.test(href.split(/[?#]/)[0]);

function Anchor({ href = "", children, ...rest }: ComponentProps<"a">) {
  if (href.startsWith("/") && !isFile(href)) {
    return (
      <Link href={href} {...rest}>
        {children}
      </Link>
    );
  }
  const external = /^https?:\/\//.test(href);
  return (
    <a href={href} {...rest} {...(external ? { rel: "noreferrer" } : {})}>
      {children}
    </a>
  );
}

/** Tables scroll sideways on narrow screens instead of overflowing. */
function Table(props: ComponentProps<"table">) {
  return (
    <div className="table-wrap" role="region" aria-label="Table" tabIndex={0}>
      <table {...props} />
    </div>
  );
}

/**
 * Everything MDX files can use without importing it. See AUTHORING.md for
 * what each one is for.
 */
export const mdxComponents = {
  h2: heading("h2"),
  h3: heading("h3"),
  h4: heading("h4"),
  a: Anchor,
  table: Table,
  pre: CodeBlock,
  Badge,
  Callout,
  CodeGroup,
  Diagram,
  FileTree,
  LinkCard,
  LinkCards,
  Step,
  Steps,
  Tab,
  Tabs,
  UiA11y,
  UiAnatomy,
  UiAssets,
  UiChoose,
  UiCsp,
  UiFlagWeights,
  UiGrid,
  UiPreview,
  UiProps,
  UiTokens,
};
