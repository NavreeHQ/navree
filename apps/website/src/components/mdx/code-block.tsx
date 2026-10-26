import type { ComponentProps } from "react";

import { CopyButton } from "./copy-button";

const languageNames: Record<string, string> = {
  rust: "Rust",
  toml: "TOML",
  shellscript: "Shell",
  sh: "Shell",
  bash: "Shell",
  shell: "Shell",
  zsh: "Shell",
  console: "Shell",
  ts: "TypeScript",
  rs: "Rust",
  yml: "YAML",
  env: ".env",
  j2: "Jinja",
  typescript: "TypeScript",
  tsx: "TSX",
  json: "JSON",
  sql: "SQL",
  http: "HTTP",
  html: "HTML",
  jinja: "Jinja",
  yaml: "YAML",
  dotenv: ".env",
  text: "Text",
};

type PreProps = ComponentProps<"pre"> & { "data-title"?: string; "data-language"?: string };

/**
 * Every fenced code block in the docs. Shiki has already highlighted it;
 * this adds the frame: the filename title (```rust title="src/main.rs")
 * or the language, and a copy button.
 */
export function CodeBlock({ "data-title": title, "data-language": language, children, className, style, ...rest }: PreProps) {
  // Shiki sets the theme's colours inline; the frame provides them.
  void style;
  const caption = title ?? (language ? (languageNames[language] ?? language) : null);
  return (
    <figure className="code-block" data-copy-root data-title={title} data-language={language}>
      <figcaption className="code-block-bar">
        <span className={title ? "code-block-title" : "code-block-lang"}>{caption}</span>
        <CopyButton />
      </figcaption>
      <pre className={className} {...rest} tabIndex={0}>
        {children}
      </pre>
    </figure>
  );
}
