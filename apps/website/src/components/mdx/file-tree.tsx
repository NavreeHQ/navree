import type { ReactNode } from "react";

import { FileIcon, FolderIcon } from "@/components/icons";

type Node = { name: string; comment: string | null; highlight: boolean; dir: boolean; children: Node[] };

/**
 * Parses an indented listing (two spaces per level):
 *
 *     src/
 *       main.rs          # the entry point
 *       deals/
 *         mod.rs  *      # a trailing `*` highlights the entry
 *
 * A name ending in `/` is a directory; `# text` is a comment.
 */
function parse(source: string): Node[] {
  const root: Node = { name: "", comment: null, highlight: false, dir: true, children: [] };
  const stack: { depth: number; node: Node }[] = [{ depth: -1, node: root }];
  for (const raw of source.split(/\r?\n/)) {
    if (!raw.trim()) continue;
    const depth = Math.floor((raw.length - raw.trimStart().length) / 2);
    let line = raw.trim();
    let comment: string | null = null;
    const hash = line.search(/\s#\s/);
    if (hash !== -1) {
      comment = line.slice(hash).replace(/^\s#\s/, "").trim();
      line = line.slice(0, hash).trim();
    }
    const highlight = line.endsWith("*");
    if (highlight) line = line.slice(0, -1).trim();
    const node: Node = { name: line.replace(/\/$/, ""), comment, highlight, dir: line.endsWith("/"), children: [] };
    while (stack.length > 1 && stack[stack.length - 1].depth >= depth) stack.pop();
    stack[stack.length - 1].node.children.push(node);
    stack.push({ depth, node });
  }
  return root.children;
}

function text(children: ReactNode): string {
  if (typeof children === "string") return children;
  if (Array.isArray(children)) return children.map(text).join("");
  return "";
}

function Entries({ nodes }: { nodes: Node[] }) {
  return (
    <ul>
      {nodes.map((node, i) => (
        <li key={`${node.name}-${i}`} className={node.highlight ? "is-highlighted" : undefined}>
          <span className="file-tree-entry">
            {node.dir ? <FolderIcon className="file-tree-icon is-dir" /> : <FileIcon className="file-tree-icon" />}
            <span className={node.dir ? "file-tree-name is-dir" : "file-tree-name"}>
              {node.name}
              {node.dir ? "/" : ""}
            </span>
            {node.comment ? <span className="file-tree-comment">{node.comment}</span> : null}
          </span>
          {node.children.length ? <Entries nodes={node.children} /> : null}
        </li>
      ))}
    </ul>
  );
}

/**
 * `<FileTree>` around a template string:
 *
 *     <FileTree>{`
 *     src/
 *       main.rs   # boots the app
 *     `}</FileTree>
 */
export function FileTree({ children, title }: { children?: ReactNode; title?: string }) {
  const nodes = parse(text(children));
  return (
    <figure className="file-tree" aria-label={title ?? "Files"}>
      {title ? <figcaption className="file-tree-caption">{title}</figcaption> : null}
      <Entries nodes={nodes} />
    </figure>
  );
}
