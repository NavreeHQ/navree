import type { ThemeRegistration } from "shiki";

/**
 * "Navree Night": the code theme. Mostly calm off-whites and greys, with the
 * accent kept for attributes and macros (`#[controller]`, `#[can(..)]`),
 * which are what Navree code is about. Every token colour has at least
 * 4.5:1 contrast on the code background.
 */
const c = {
  bg: "#0a0e0c",
  fg: "#d6ded9",
  muted: "#8a968f",
  punct: "#9aa7a0",
  keyword: "#8de8b8",
  accent: "#3dff8f",
  string: "#e7d6a6",
  number: "#f0b98a",
  type: "#a9d8f5",
  fn: "#f2f5f3",
  param: "#d6ded9",
  constant: "#f0b98a",
};

export const navreeTheme: ThemeRegistration = {
  name: "navree-night",
  type: "dark",
  colors: {
    "editor.background": c.bg,
    "editor.foreground": c.fg,
  },
  settings: [
    { settings: { foreground: c.fg, background: c.bg } },
    {
      scope: ["comment", "punctuation.definition.comment", "comment.line.documentation", "comment.block.documentation"],
      settings: { foreground: c.muted, fontStyle: "italic" },
    },
    {
      scope: [
        "keyword",
        "storage",
        "storage.type",
        "storage.modifier",
        "keyword.control",
        "keyword.other",
        "keyword.operator.new",
        "keyword.operator.expression",
        "variable.language.self",
        "variable.language.this",
        "constant.language.bool",
      ],
      settings: { foreground: c.keyword },
    },
    {
      scope: [
        "meta.attribute",
        "meta.attribute.rust",
        "punctuation.definition.attribute",
        "entity.name.function.macro",
        "support.macro",
        "meta.decorator",
        "entity.name.function.decorator",
        "punctuation.decorator",
      ],
      settings: { foreground: c.accent },
    },
    {
      scope: ["string", "string.quoted", "punctuation.definition.string", "string.template", "markup.inline.raw"],
      settings: { foreground: c.string },
    },
    {
      scope: [
        "constant.numeric",
        "constant.language",
        "constant.character",
        "constant.other",
        "support.constant",
        "variable.other.constant",
      ],
      settings: { foreground: c.number },
    },
    {
      scope: [
        "entity.name.type",
        "entity.name.class",
        "entity.name.struct",
        "entity.name.enum",
        "entity.name.trait",
        "entity.other.inherited-class",
        "support.type",
        "support.class",
        "storage.type.core.rust",
        "storage.type.numeric",
        "entity.name.type.lifetime",
        "storage.modifier.lifetime",
      ],
      settings: { foreground: c.type },
    },
    {
      scope: ["entity.name.function", "support.function", "meta.function-call entity.name.function"],
      settings: { foreground: c.fn },
    },
    {
      scope: ["entity.name.namespace", "entity.name.module", "meta.path"],
      settings: { foreground: c.fg },
    },
    { scope: ["variable", "variable.parameter", "variable.other"], settings: { foreground: c.param } },
    {
      scope: ["punctuation", "keyword.operator", "meta.brace", "punctuation.separator", "punctuation.terminator"],
      settings: { foreground: c.punct },
    },
    // TOML
    { scope: ["entity.name.tag.toml", "support.type.property-name.toml"], settings: { foreground: c.type } },
    { scope: ["entity.other.attribute-name.table.toml", "punctuation.definition.table.toml"], settings: { foreground: c.keyword } },
    // JSON
    {
      scope: ["support.type.property-name.json", "support.type.property-name", "meta.object-literal.key"],
      settings: { foreground: c.type },
    },
    // Shell
    { scope: ["entity.name.command", "support.function.builtin.shell"], settings: { foreground: c.fn } },
    { scope: ["constant.other.option", "variable.parameter.option"], settings: { foreground: c.type } },
    // HTTP
    { scope: ["keyword.http", "keyword.other.http"], settings: { foreground: c.keyword } },
    // SQL
    { scope: ["keyword.other.DML.sql", "keyword.other.DDL.create.II.sql", "keyword.other.sql"], settings: { foreground: c.keyword } },
    // Markup in code
    { scope: ["markup.bold"], settings: { fontStyle: "bold" } },
    { scope: ["markup.italic"], settings: { fontStyle: "italic" } },
    { scope: ["markup.inserted"], settings: { foreground: c.accent } },
    { scope: ["markup.deleted"], settings: { foreground: "#ff9a8f" } },
  ],
};

/** The languages loaded at build time. Anything else renders as plain text. */
export const languages = ["rust", "toml", "shellscript", "typescript", "tsx", "json", "sql", "http", "html", "css", "jinja", "yaml", "dotenv"] as const;

/** Aliases authors may use in fences. */
export const languageAliases: Record<string, string> = {
  sh: "shellscript",
  bash: "shellscript",
  shell: "shellscript",
  zsh: "shellscript",
  console: "shellscript",
  ts: "typescript",
  rs: "rust",
  env: "dotenv",
  yml: "yaml",
  j2: "jinja",
};
