import Link from "next/link";
import type { ReactNode } from "react";

import { ArrowRightIcon, ExternalIcon } from "@/components/icons";

/** `<LinkCards>`: a grid of `<LinkCard>`s, two per row on wide screens. */
export function LinkCards({ children }: { children?: ReactNode }) {
  return <div className="link-cards">{children}</div>;
}

/**
 * `<LinkCard href="/docs/basics/validation/" title="Validation">One line on
 * what the reader finds there.</LinkCard>`. Links starting with `http` open
 * the external site.
 */
export function LinkCard({ href, title, children }: { href: string; title: string; children?: ReactNode }) {
  const external = /^https?:\/\//.test(href);
  const body = (
    <>
      <span className="link-card-title">
        {title}
        {external ? (
          <ExternalIcon className="link-card-icon" />
        ) : (
          <ArrowRightIcon className="link-card-icon" />
        )}
      </span>
      {children ? <div className="link-card-text">{children}</div> : null}
    </>
  );
  return external ? (
    <a className="link-card" href={href} rel="noreferrer">
      {body}
    </a>
  ) : (
    <Link className="link-card" href={href}>
      {body}
    </Link>
  );
}
