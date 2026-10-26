/**
 * `<FileTree>{`...`}</FileTree>` keeps its indentation.
 *
 * MDX removes up to two leading spaces from every line of an expression, so
 * the template string a `<FileTree>` receives has lost its first level of
 * nesting (`config/` and `  app.toml` arrive as siblings). The original text
 * is still in the source, at the expression's position: these helpers read
 * it back from there. Pure functions with no imports, so both the site (MDX)
 * and `scripts/agent-files.ts` (Markdown for agents) can use them.
 */

type Positioned = {
  type: string;
  name?: string | null;
  children?: Positioned[];
  position?: { start: { offset?: number }; end: { offset?: number } };
  value?: string;
  data?: Record<string, unknown>;
};

/**
 * The template string of a `{`...`}` expression node as written in
 * `source`, or `null` when the node isn't a plain template string.
 */
export function fileTreeText(source: string, expression: Positioned): string | null {
  const from = expression.position?.start.offset;
  const to = expression.position?.end.offset;
  if (from === undefined || to === undefined) return null;
  const match = /^\{\s*`([\s\S]*)`\s*\}$/.exec(source.slice(from, to).trim());
  if (!match || match[1].includes("${")) return null;
  return match[1].replace(/\\`/g, "`").replace(/\\\$/g, "$").replace(/\\\\/g, "\\");
}

function isFileTree(node: Positioned): boolean {
  return (node.type === "mdxJsxFlowElement" || node.type === "mdxJsxTextElement") && node.name === "FileTree";
}

function isExpression(node: Positioned): boolean {
  return node.type === "mdxFlowExpression" || node.type === "mdxTextExpression";
}

/** Every `<FileTree>` in `tree`, with its expression children. */
function fileTrees(node: Positioned, out: Positioned[] = []): Positioned[] {
  if (isFileTree(node)) out.push(node);
  for (const kid of node.children ?? []) fileTrees(kid, out);
  return out;
}

/** The expression children of a `<FileTree>`, also inside a paragraph. */
function expressions(node: Positioned): Positioned[] {
  return (node.children ?? []).flatMap((kid) => (isExpression(kid) ? [kid] : kid.type === "paragraph" ? expressions(kid) : []));
}

/**
 * A remark plugin for `@mdx-js/mdx`: gives each `<FileTree>` its template
 * string with the indentation from the source.
 */
export function remarkFileTreeSource() {
  return (tree: Positioned, file: { value: unknown }) => {
    const source = String(file.value);
    for (const element of fileTrees(tree)) {
      for (const expression of expressions(element)) {
        const value = fileTreeText(source, expression);
        if (value === null) continue;
        expression.value = JSON.stringify(value);
        expression.data = {
          ...expression.data,
          estree: {
            type: "Program",
            sourceType: "module",
            comments: [],
            body: [{ type: "ExpressionStatement", expression: { type: "Literal", value, raw: JSON.stringify(value) } }],
          },
        };
      }
    }
  };
}
