import type { ReactNode } from "react";

/**
 * `<Steps>` with `<Step title="...">` children: a numbered procedure, for
 * content that really is a sequence (install, configure, run).
 */
export function Steps({ children }: { children?: ReactNode }) {
  return <ol className="steps">{children}</ol>;
}

export function Step({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <li className="step">
      <p className="step-title">{title}</p>
      <div className="step-body">{children}</div>
    </li>
  );
}
