import type { ReactNode } from "react";

/**
 * A small inline label.
 *
 * - `<Badge feature="queue" />` says which Cargo feature of `navree` a page
 *   or section needs ("Feature: queue").
 * - `<Badge>Experimental</Badge>`, with `tone="accent"` or `tone="warning"`
 *   when it should stand out.
 */
export function Badge({
  feature,
  tone = "default",
  children,
}: {
  feature?: string;
  tone?: "default" | "accent" | "warning";
  children?: ReactNode;
}) {
  const tones = {
    default: "border-line-2 text-fg-2",
    accent: "border-accent/40 text-accent",
    warning: "border-amber/40 text-amber",
  } as const;
  return (
    <span
      className={`badge mx-0.5 inline-flex translate-y-[-1px] items-center gap-1.5 rounded-full border align-middle text-[0.8125rem] font-medium ${
        feature ? "py-0.5 pr-0.5 pl-2.5 leading-5" : "px-2.5 py-px leading-6"
      } ${tones[tone]}`}
    >
      {feature ? (
        <>
          <span className="text-fg-3">Feature</span>
          <code className="badge-code">{feature}</code>
        </>
      ) : (
        children
      )}
    </span>
  );
}
