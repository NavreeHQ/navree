//! Colours code for a code block, on the server: a small reader for each family of
//! languages marks comments, strings, numbers, keywords, types, calls and attributes,
//! and the stylesheet gives each kind its colour from the theme. The kinds follow the
//! roles of the documentation's own code theme: calm text, the accent for attributes
//! and macros, strings and numbers apart from keywords.
//!
//! The readers are forgiving, not parsers: text they do not understand stays plain.
//! Every step consumes at least one character, so the time is linear in the code, and
//! code past [`MAX_CODE`] bytes is left plain.

use crate::node::{Node, el};

/// The most code that is coloured. A longer block is shown as plain text.
pub(crate) const MAX_CODE: usize = 64 * 1024;

/// What a stretch of code is, as the stylesheet colours it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Plain,
    Comment,
    Str,
    Number,
    Keyword,
    Type,
    Func,
    Attr,
    Add,
    Del,
}

impl Kind {
    /// The value of `data-t`.
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Plain => "",
            Self::Comment => "c",
            Self::Str => "s",
            Self::Number => "n",
            Self::Keyword => "k",
            Self::Type => "t",
            Self::Func => "f",
            Self::Attr => "m",
            Self::Add => "a",
            Self::Del => "d",
        }
    }
}

/// A stretch of the code, by byte range.
type Token = (Kind, usize, usize);

/// The ways one family of languages writes itself.
#[derive(Clone, Copy)]
struct Lang {
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    quotes: &'static [u8],
    /// `"""` and `'''` strings that run over lines.
    triple: bool,
    /// A `"` string may run over lines (Rust).
    multiline_strings: bool,
    keywords: &'static [&'static str],
    types: &'static [&'static str],
    /// Keywords and types ignore case (SQL).
    insensitive: bool,
    /// `name!` is a macro and `#[...]` an attribute (Rust).
    rust: bool,
    /// `@name` is a decorator or an annotation.
    decorators: bool,
    /// A name before `(` is a call.
    calls: bool,
    /// A name that starts with a capital is a type.
    capital_types: bool,
    /// `$name` and `${name}` are variables.
    dollar: bool,
    /// A string before `:` is a key (JSON).
    json: bool,
    /// A name before `:` or `=` at the start of a line is a key.
    keys: bool,
    /// A line that starts with `[name]` is a table heading (TOML).
    tables: bool,
    /// A name may hold dashes (CSS, shell flags, keys).
    dashes: bool,
    /// Braces matter: a name before `:` inside them is a property, `#fff` a colour.
    css: bool,
}

const NONE: Lang = Lang {
    line_comments: &[],
    block_comment: None,
    quotes: &[],
    triple: false,
    multiline_strings: false,
    keywords: &[],
    types: &[],
    insensitive: false,
    rust: false,
    decorators: false,
    calls: false,
    capital_types: false,
    dollar: false,
    json: false,
    keys: false,
    tables: false,
    dashes: false,
    css: false,
};

const RUST: Lang = Lang {
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    quotes: b"\"'",
    multiline_strings: true,
    keywords: &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
        "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait", "true",
        "type", "unsafe", "use", "where", "while",
    ],
    types: &[
        "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
        "f32", "f64", "bool", "char", "str",
    ],
    rust: true,
    calls: true,
    capital_types: true,
    ..NONE
};

const SCRIPT: Lang = Lang {
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    quotes: b"\"'`",
    keywords: &[
        "abstract",
        "as",
        "async",
        "await",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "debugger",
        "declare",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "from",
        "function",
        "if",
        "implements",
        "import",
        "in",
        "instanceof",
        "interface",
        "keyof",
        "let",
        "namespace",
        "new",
        "null",
        "of",
        "private",
        "protected",
        "public",
        "readonly",
        "return",
        "satisfies",
        "static",
        "super",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "type",
        "typeof",
        "undefined",
        "var",
        "void",
        "while",
        "with",
        "yield",
    ],
    types: &[
        "string", "number", "boolean", "any", "unknown", "never", "object", "symbol", "bigint",
    ],
    decorators: true,
    calls: true,
    capital_types: true,
    ..NONE
};

const PYTHON: Lang = Lang {
    line_comments: &["#"],
    quotes: b"\"'",
    triple: true,
    keywords: &[
        "and", "as", "assert", "async", "await", "break", "case", "class", "continue", "def",
        "del", "elif", "else", "except", "False", "finally", "for", "from", "global", "if",
        "import", "in", "is", "lambda", "match", "None", "nonlocal", "not", "or", "pass", "raise",
        "return", "True", "try", "while", "with", "yield",
    ],
    types: &[
        "int", "float", "str", "bool", "list", "dict", "tuple", "set", "bytes", "object",
    ],
    decorators: true,
    calls: true,
    capital_types: true,
    ..NONE
};

const GO: Lang = Lang {
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    quotes: b"\"'`",
    keywords: &[
        "break",
        "case",
        "chan",
        "const",
        "continue",
        "default",
        "defer",
        "else",
        "fallthrough",
        "false",
        "for",
        "func",
        "go",
        "goto",
        "if",
        "import",
        "interface",
        "map",
        "nil",
        "package",
        "range",
        "return",
        "select",
        "struct",
        "switch",
        "true",
        "type",
        "var",
    ],
    types: &[
        "int", "int8", "int16", "int32", "int64", "uint", "uint8", "uint16", "uint32", "uint64",
        "float32", "float64", "string", "bool", "byte", "rune", "error", "any",
    ],
    calls: true,
    capital_types: true,
    ..NONE
};

const SHELL: Lang = Lang {
    line_comments: &["#"],
    quotes: b"\"'",
    keywords: &[
        "alias", "break", "case", "continue", "do", "done", "elif", "else", "esac", "eval", "exec",
        "exit", "export", "false", "fi", "for", "function", "if", "in", "local", "readonly",
        "return", "set", "shift", "source", "then", "trap", "true", "unset", "until", "while",
    ],
    dollar: true,
    dashes: true,
    ..NONE
};

const SQL: Lang = Lang {
    line_comments: &["--"],
    block_comment: Some(("/*", "*/")),
    quotes: b"\"'`",
    insensitive: true,
    keywords: &[
        "all",
        "alter",
        "and",
        "as",
        "asc",
        "begin",
        "between",
        "by",
        "case",
        "check",
        "commit",
        "constraint",
        "create",
        "cross",
        "default",
        "delete",
        "desc",
        "distinct",
        "drop",
        "else",
        "end",
        "exists",
        "false",
        "foreign",
        "from",
        "full",
        "group",
        "having",
        "if",
        "in",
        "index",
        "inner",
        "insert",
        "into",
        "is",
        "join",
        "key",
        "left",
        "like",
        "limit",
        "not",
        "null",
        "offset",
        "on",
        "or",
        "order",
        "outer",
        "primary",
        "references",
        "returning",
        "right",
        "rollback",
        "select",
        "set",
        "table",
        "then",
        "true",
        "union",
        "unique",
        "update",
        "values",
        "view",
        "when",
        "where",
        "with",
    ],
    types: &[
        "bigint",
        "bigserial",
        "boolean",
        "bool",
        "char",
        "date",
        "decimal",
        "double",
        "float",
        "int",
        "integer",
        "json",
        "jsonb",
        "numeric",
        "serial",
        "smallint",
        "text",
        "time",
        "timestamp",
        "timestamptz",
        "uuid",
        "varchar",
    ],
    calls: true,
    ..NONE
};

const JSON: Lang = Lang {
    quotes: b"\"",
    keywords: &["true", "false", "null"],
    json: true,
    ..NONE
};

const TOML: Lang = Lang {
    line_comments: &["#"],
    quotes: b"\"'",
    triple: true,
    keywords: &["true", "false"],
    keys: true,
    tables: true,
    dashes: true,
    ..NONE
};

const YAML: Lang = Lang {
    line_comments: &["#"],
    quotes: b"\"'",
    keywords: &["true", "false", "null", "yes", "no"],
    keys: true,
    dashes: true,
    ..NONE
};

const DOTENV: Lang = Lang {
    line_comments: &["#"],
    quotes: b"\"'",
    keys: true,
    ..NONE
};

const CSS: Lang = Lang {
    block_comment: Some(("/*", "*/")),
    quotes: b"\"'",
    calls: true,
    decorators: true,
    dashes: true,
    css: true,
    ..NONE
};

/// How a language is read.
#[derive(Clone, Copy)]
enum Reader {
    Generic(Lang),
    Html,
    Diff,
    Http,
}

/// The reader for a fence's language name, or none.
fn reader_for(name: &str) -> Option<Reader> {
    let name = name.trim().to_ascii_lowercase();
    // A fence may carry more than the name: ```rust title="x"
    let name = name.split([' ', ',', '{']).next().unwrap_or_default();
    Some(match name {
        "rust" | "rs" => Reader::Generic(RUST),
        "typescript" | "ts" | "tsx" | "javascript" | "js" | "jsx" | "mjs" | "cjs" => {
            Reader::Generic(SCRIPT)
        }
        "python" | "py" => Reader::Generic(PYTHON),
        "go" | "golang" => Reader::Generic(GO),
        "sh" | "bash" | "shell" | "shellscript" | "zsh" | "console" => Reader::Generic(SHELL),
        "sql" | "postgres" | "postgresql" | "mysql" | "sqlite" => Reader::Generic(SQL),
        "json" | "jsonc" => Reader::Generic(JSON),
        "toml" => Reader::Generic(TOML),
        "yaml" | "yml" => Reader::Generic(YAML),
        "dotenv" | "env" => Reader::Generic(DOTENV),
        "css" | "scss" => Reader::Generic(CSS),
        "html" | "xml" | "svg" | "jinja" | "j2" | "vue" => Reader::Html,
        "diff" | "patch" => Reader::Diff,
        "http" => Reader::Http,
        _ => return None,
    })
}

/// The code as nodes: plain text, and a `span` with `data-t` for each coloured stretch.
/// `None` for a language the readers do not know, or code that is too long.
pub(crate) fn highlight(language: &str, code: &str) -> Option<Vec<Node>> {
    if code.len() > MAX_CODE {
        return None;
    }
    let reader = reader_for(language)?;
    let tokens = match reader {
        Reader::Generic(lang) => lex(&lang, code),
        Reader::Html => html(code),
        Reader::Diff => diff(code),
        Reader::Http => http(code),
    };
    Some(nodes(code, &tokens))
}

fn nodes(code: &str, tokens: &[Token]) -> Vec<Node> {
    let mut out = Vec::with_capacity(tokens.len());
    for &(kind, start, end) in tokens {
        let text = code[start..end].to_owned();
        if kind == Kind::Plain {
            out.push(Node::text(text));
        } else {
            out.push(el("span").attr("data-t", kind.code()).child(text).into());
        }
    }
    out
}

/// Collects tokens, joining a stretch of the same kind that touches the last.
struct Tokens(Vec<Token>);

impl Tokens {
    fn push(&mut self, kind: Kind, start: usize, end: usize) {
        if start >= end {
            return;
        }
        if let Some(last) = self.0.last_mut()
            && last.0 == kind
            && last.2 == start
        {
            last.2 = end;
            return;
        }
        self.0.push((kind, start, end));
    }
}

/// The index after the character that starts at `at`.
fn after_char(code: &str, at: usize) -> usize {
    let mut next = at + 1;
    while next < code.len() && !code.is_char_boundary(next) {
        next += 1;
    }
    next.min(code.len())
}

fn is_word_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The end of the line that holds `at` (the index of its `\n`, or the end).
fn line_end(code: &str, at: usize) -> usize {
    code[at..].find('\n').map_or(code.len(), |n| at + n)
}

fn contains(list: &[&str], word: &str, insensitive: bool) -> bool {
    list.iter().any(|item| {
        if insensitive {
            item.eq_ignore_ascii_case(word)
        } else {
            *item == word
        }
    })
}

/// The first byte after spaces and tabs from `at`, with its index.
fn next_on_line(code: &str, mut at: usize) -> Option<(usize, u8)> {
    let bytes = code.as_bytes();
    while at < bytes.len() && matches!(bytes[at], b' ' | b'\t') {
        at += 1;
    }
    bytes.get(at).map(|byte| (at, *byte))
}

/// Whether only spaces, tabs and list dashes come between the start of the line and `at`.
fn at_line_start(code: &str, line_begin: usize, at: usize) -> bool {
    code[line_begin..at]
        .bytes()
        .all(|b| matches!(b, b' ' | b'\t' | b'-'))
}

/// Where a string that starts at `at` (a quote of `quote`) ends, past its closing quote;
/// `None` when it does not close where this language allows.
fn string_end(lang: &Lang, code: &str, at: usize) -> Option<usize> {
    let bytes = code.as_bytes();
    let quote = bytes[at];
    if lang.triple
        && (quote == b'"' || quote == b'\'')
        && bytes[at..].starts_with(&[quote, quote, quote])
    {
        let close = [quote, quote, quote];
        let mut i = at + 3;
        while i + 3 <= bytes.len() {
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if bytes[i..].starts_with(&close) {
                return Some(i + 3);
            }
            i += 1;
        }
        return Some(bytes.len());
    }
    let multi = quote == b'`' || (lang.multiline_strings && quote == b'"');
    let mut i = at + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'\n' if !multi => return None,
            byte if byte == quote => return Some(i + 1),
            _ => i += 1,
        }
    }
    if multi { Some(bytes.len()) } else { None }
}

/// Where a Rust raw string that starts at `at` (`r"`, `r#"`, `br"`) ends.
fn raw_string_end(code: &str, at: usize) -> Option<usize> {
    let bytes = code.as_bytes();
    let mut i = at;
    if bytes.get(i) == Some(&b'b') {
        i += 1;
    }
    if bytes.get(i) != Some(&b'r') {
        return None;
    }
    i += 1;
    let hashes = bytes[i..].iter().take_while(|b| **b == b'#').count();
    i += hashes;
    if bytes.get(i) != Some(&b'"') {
        return None;
    }
    let mut close = vec![b'"'];
    close.extend(std::iter::repeat_n(b'#', hashes));
    let found = bytes[i + 1..]
        .windows(close.len())
        .position(|window| window == close.as_slice());
    Some(found.map_or(bytes.len(), |n| i + 1 + n + close.len()))
}

/// Whether the quote at `at` starts a Rust lifetime or label (`'a`), not a character.
fn lifetime_end(code: &str, at: usize) -> Option<usize> {
    let bytes = code.as_bytes();
    let first = *bytes.get(at + 1)?;
    if !is_word_start(first) {
        return None;
    }
    let mut i = at + 2;
    while i < bytes.len() && is_word(bytes[i]) {
        i += 1;
    }
    // `'a'` is a character.
    if bytes.get(i) == Some(&b'\'') {
        None
    } else {
        Some(i)
    }
}

fn lex(lang: &Lang, code: &str) -> Vec<Token> {
    let bytes = code.as_bytes();
    let mut out = Tokens(Vec::new());
    let mut i = 0;
    let mut depth = 0_i32;
    // Where the current line began, found by walking each byte once.
    let (mut line_begin, mut walked) = (0, 0);
    while i < bytes.len() {
        if let Some(n) = code[walked..i].rfind('\n') {
            line_begin = walked + n + 1;
        }
        walked = i;
        let byte = bytes[i];
        let rest = &code[i..];
        // Comments.
        if let Some(open) = lang
            .line_comments
            .iter()
            .find(|open| rest.starts_with(**open))
            // A `#` inside a shell word (`$#`, `${#x}`) is not a comment.
            && !(*open == "#" && lang.dollar && i > 0 && !bytes[i - 1].is_ascii_whitespace())
        {
            let end = line_end(code, i);
            out.push(Kind::Comment, i, end);
            i = end;
            continue;
        }
        if let Some((open, close)) = lang.block_comment
            && rest.starts_with(open)
        {
            let end = rest[open.len()..]
                .find(close)
                .map_or(code.len(), |n| i + open.len() + n + close.len());
            out.push(Kind::Comment, i, end);
            i = end;
            continue;
        }
        // Rust attributes.
        if lang.rust && byte == b'#' && (rest.starts_with("#[") || rest.starts_with("#![")) {
            let open = if rest.starts_with("#![") { 2 } else { 1 };
            let mut level = 0_i32;
            let mut end = None;
            for (n, b) in bytes[i + open..].iter().enumerate().take(600) {
                match b {
                    b'[' => level += 1,
                    b']' => {
                        level -= 1;
                        if level == 0 {
                            end = Some(i + open + n + 1);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            // One that never closes is only a `#`.
            if let Some(end) = end {
                out.push(Kind::Attr, i, end);
                i = end;
                continue;
            }
        }
        // Rust raw strings.
        if lang.rust
            && (byte == b'r' || byte == b'b')
            && (i == 0 || !is_word(bytes[i - 1]))
            && let Some(end) = raw_string_end(code, i)
        {
            out.push(Kind::Str, i, end);
            i = end;
            continue;
        }
        // Strings.
        if lang.quotes.contains(&byte) {
            if lang.rust
                && byte == b'\''
                && let Some(end) = lifetime_end(code, i)
            {
                out.push(Kind::Type, i, end);
                i = end;
                continue;
            }
            if let Some(end) = string_end(lang, code, i) {
                let kind = if lang.json && next_on_line(code, end).is_some_and(|(_, b)| b == b':') {
                    Kind::Type
                } else {
                    Kind::Str
                };
                out.push(kind, i, end);
                i = end;
                continue;
            }
            // A quote that never closes is just a character.
            out.push(Kind::Plain, i, i + 1);
            i += 1;
            continue;
        }
        // Tables and headings: `[server]` in TOML, `[section]` in an ini file.
        if lang.tables && byte == b'[' && at_line_start(code, line_begin, i) {
            let end = line_end(code, i);
            let line = &code[i..end];
            if let Some(close) = line.find(']') {
                out.push(Kind::Keyword, i, i + close + 1);
                i += close + 1;
                continue;
            }
        }
        // CSS: braces, colours, properties and at-rules.
        if lang.css {
            if byte == b'{' {
                depth += 1;
            } else if byte == b'}' {
                depth = (depth - 1).max(0);
            }
            if byte == b'#' && depth > 0 {
                let mut end = i + 1;
                while end < bytes.len() && bytes[end].is_ascii_hexdigit() {
                    end += 1;
                }
                if matches!(end - i - 1, 3 | 4 | 6 | 8) {
                    out.push(Kind::Number, i, end);
                    i = end;
                    continue;
                }
            }
            if byte == b'!' && rest.starts_with("!important") {
                out.push(Kind::Keyword, i, i + 10);
                i += 10;
                continue;
            }
        }
        // Numbers.
        let after_word = i > 0 && (is_word(bytes[i - 1]) || (lang.dashes && bytes[i - 1] == b'-'));
        if byte.is_ascii_digit() && !after_word {
            let mut end = i + 1;
            while end < bytes.len() {
                let b = bytes[end];
                if b.is_ascii_alphanumeric() || b == b'_' {
                    end += 1;
                } else if b == b'.' && bytes.get(end + 1).is_some_and(u8::is_ascii_digit) {
                    end += 1;
                } else if b == b'%' && lang.css {
                    end += 1;
                } else {
                    break;
                }
            }
            out.push(Kind::Number, i, end);
            i = end;
            continue;
        }
        // Variables.
        if lang.dollar && byte == b'$' {
            let mut end = i + 1;
            if bytes.get(end) == Some(&b'{') {
                end = code[end..].find('}').map_or(end + 1, |n| end + n + 1);
            } else {
                while end < bytes.len() && is_word(bytes[end]) {
                    end += 1;
                }
            }
            out.push(Kind::Type, i, end.max(i + 1));
            i = end.max(i + 1);
            continue;
        }
        // Decorators, annotations and at-rules.
        if lang.decorators && byte == b'@' && bytes.get(i + 1).is_some_and(|b| is_word_start(*b)) {
            let mut end = i + 1;
            while end < bytes.len()
                && (is_word(bytes[end])
                    || bytes[end] == b'.'
                    || (lang.dashes && bytes[end] == b'-'))
            {
                end += 1;
            }
            out.push(Kind::Attr, i, end);
            i = end;
            continue;
        }
        // Names.
        let flag = lang.dashes
            && byte == b'-'
            && (i == 0 || !is_word(bytes[i - 1]))
            && (bytes.get(i + 1).is_some_and(|b| is_word_start(*b))
                || (bytes.get(i + 1) == Some(&b'-')
                    && bytes.get(i + 2).is_some_and(|b| is_word_start(*b))));
        if is_word_start(byte) || flag {
            let mut end = i + 1;
            while end < bytes.len()
                && (is_word(bytes[end])
                    || (lang.dashes
                        && bytes[end] == b'-'
                        && bytes.get(end + 1).is_some_and(|b| is_word(*b))))
            {
                end += 1;
            }
            let word = &code[i..end];
            let next = bytes.get(end).copied();
            let kind = if byte == b'-' {
                // A flag: `-v`, `--release`.
                Kind::Type
            } else if lang.keys
                && at_line_start(code, line_begin, i)
                && next_on_line(code, end).is_some_and(|(at, b)| {
                    (b == b':' || b == b'=')
                        && bytes.get(at + 1) != Some(&b'=')
                        && bytes.get(at + 1) != Some(&b':')
                })
            {
                Kind::Type
            } else if lang.css
                && depth > 0
                && next_on_line(code, end).is_some_and(|(_, b)| b == b':')
            {
                Kind::Type
            } else if contains(lang.keywords, word, lang.insensitive) {
                Kind::Keyword
            } else if contains(lang.types, word, lang.insensitive) {
                Kind::Type
            } else if lang.rust && next == Some(b'!') && bytes.get(end + 1) != Some(&b'=') {
                Kind::Attr
            } else if lang.calls && next == Some(b'(') {
                Kind::Func
            } else if lang.capital_types
                && byte.is_ascii_uppercase()
                && word.len() > 1
                && word.bytes().any(|b| b.is_ascii_lowercase())
            {
                Kind::Type
            } else {
                Kind::Plain
            };
            out.push(kind, i, end);
            i = end;
            continue;
        }
        let next = after_char(code, i);
        out.push(Kind::Plain, i, next);
        i = next;
    }
    out.0
}

/// HTML and XML: tags, attribute names, attribute values and comments. A Jinja or
/// Handlebars tag (`{% if %}`, `{{ name }}`) is marked too.
fn html(code: &str) -> Vec<Token> {
    let bytes = code.as_bytes();
    let mut out = Tokens(Vec::new());
    let mut i = 0;
    while i < bytes.len() {
        let rest = &code[i..];
        if rest.starts_with("<!--") {
            let end = rest[4..].find("-->").map_or(code.len(), |n| i + 4 + n + 3);
            out.push(Kind::Comment, i, end);
            i = end;
        } else if rest.starts_with("{%") || rest.starts_with("{{") || rest.starts_with("{#") {
            let close = if rest.starts_with("{%") {
                "%}"
            } else if rest.starts_with("{{") {
                "}}"
            } else {
                "#}"
            };
            let end = rest[2..].find(close).map_or(code.len(), |n| i + 2 + n + 2);
            let kind = if rest.starts_with("{#") {
                Kind::Comment
            } else {
                Kind::Attr
            };
            out.push(kind, i, end);
            i = end;
        } else if bytes[i] == b'<'
            && bytes
                .get(i + 1)
                .is_some_and(|b| is_word_start(*b) || *b == b'/' || *b == b'!' || *b == b'?')
        {
            // `<`, an optional `/`, the name.
            let mut at = i + 1;
            while at < bytes.len() && matches!(bytes[at], b'/' | b'!' | b'?') {
                at += 1;
            }
            let name_start = at;
            while at < bytes.len()
                && (is_word(bytes[at]) || matches!(bytes[at], b'-' | b':' | b'.'))
            {
                at += 1;
            }
            out.push(Kind::Plain, i, name_start);
            out.push(Kind::Keyword, name_start, at);
            i = at;
            // Attributes, up to the closing `>`.
            while i < bytes.len() && bytes[i] != b'>' {
                let b = bytes[i];
                if b == b'"' || b == b'\'' {
                    let end = rest_quote_end(code, i);
                    out.push(Kind::Str, i, end);
                    i = end;
                } else if is_word_start(b) || b == b'@' || b == b':' {
                    let start = i;
                    i += 1;
                    while i < bytes.len()
                        && (is_word(bytes[i]) || matches!(bytes[i], b'-' | b':' | b'.' | b'@'))
                    {
                        i += 1;
                    }
                    out.push(Kind::Type, start, i);
                } else if rest_starts(code, i, "{{") || rest_starts(code, i, "{%") {
                    let close = if rest_starts(code, i, "{{") {
                        "}}"
                    } else {
                        "%}"
                    };
                    let end = code[i + 2..]
                        .find(close)
                        .map_or(code.len(), |n| i + 2 + n + 2);
                    out.push(Kind::Attr, i, end);
                    i = end;
                } else {
                    let next = after_char(code, i);
                    out.push(Kind::Plain, i, next);
                    i = next;
                }
            }
        } else {
            let next = after_char(code, i);
            out.push(Kind::Plain, i, next);
            i = next;
        }
    }
    out.0
}

fn rest_starts(code: &str, at: usize, prefix: &str) -> bool {
    code[at..].starts_with(prefix)
}

/// The end of the quoted value that starts at `at`, past its closing quote or at the end.
fn rest_quote_end(code: &str, at: usize) -> usize {
    let quote = code.as_bytes()[at];
    code.as_bytes()[at + 1..]
        .iter()
        .position(|b| *b == quote)
        .map_or(code.len(), |n| at + 1 + n + 1)
}

/// A diff: added lines, removed lines and hunk headers.
fn diff(code: &str) -> Vec<Token> {
    let mut out = Tokens(Vec::new());
    let mut at = 0;
    while at < code.len() {
        let end = line_end(code, at);
        let line = &code[at..end];
        let kind = if line.starts_with("+++")
            || line.starts_with("---")
            || line.starts_with("diff ")
            || line.starts_with("index ")
        {
            Kind::Comment
        } else if line.starts_with("@@") {
            Kind::Keyword
        } else if line.starts_with('+') {
            Kind::Add
        } else if line.starts_with('-') {
            Kind::Del
        } else {
            Kind::Plain
        };
        out.push(kind, at, end);
        // The newline stays plain, so a coloured line does not run on.
        if end < code.len() {
            out.push(Kind::Plain, end, end + 1);
        }
        at = end + 1;
    }
    out.0
}

/// An HTTP message: the request or status line, and headers.
fn http(code: &str) -> Vec<Token> {
    let mut out = Tokens(Vec::new());
    let mut at = 0;
    let mut head = true;
    let mut first = true;
    while at < code.len() {
        let end = line_end(code, at);
        let line = &code[at..end];
        if head && line.trim().is_empty() {
            head = false;
        } else if head && first {
            first = false;
            let mut parts = line.splitn(3, ' ');
            let one = parts.next().unwrap_or_default();
            out.push(Kind::Keyword, at, at + one.len());
            if let Some(two) = parts.next() {
                let start = at + one.len() + 1;
                let kind = if one.starts_with("HTTP/") {
                    Kind::Number
                } else {
                    Kind::Str
                };
                out.push(Kind::Plain, at + one.len(), start);
                out.push(kind, start, start + two.len());
                if let Some(three) = parts.next() {
                    let from = start + two.len() + 1;
                    out.push(Kind::Plain, start + two.len(), from);
                    let kind = if one.starts_with("HTTP/") {
                        Kind::Plain
                    } else {
                        Kind::Keyword
                    };
                    out.push(kind, from, from + three.len());
                }
            }
        } else if head && let Some(colon) = line.find(':') {
            out.push(Kind::Type, at, at + colon);
            out.push(Kind::Plain, at + colon, end);
        } else {
            out.push(Kind::Plain, at, end);
        }
        if end < code.len() {
            out.push(Kind::Plain, end, end + 1);
        }
        at = end + 1;
    }
    out.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render;

    fn kinds(language: &str, code: &str) -> Vec<(Kind, String)> {
        let reader = reader_for(language).expect("a known language");
        let tokens = match reader {
            Reader::Generic(lang) => lex(&lang, code),
            Reader::Html => html(code),
            Reader::Diff => diff(code),
            Reader::Http => http(code),
        };
        tokens
            .into_iter()
            .map(|(kind, a, b)| (kind, code[a..b].to_owned()))
            .collect()
    }

    fn has(tokens: &[(Kind, String)], kind: Kind, text: &str) -> bool {
        tokens.iter().any(|(k, t)| *k == kind && t == text)
    }

    #[test]
    fn rust_has_keywords_types_macros_attributes_strings_and_numbers() {
        let code = "#[derive(Debug)]\nfn main() {\n    let x: u32 = 0x1F; // note\n    println!(\"hi {}\", x);\n}\n";
        let tokens = kinds("rust", code);
        assert!(has(&tokens, Kind::Attr, "#[derive(Debug)]"));
        assert!(has(&tokens, Kind::Keyword, "fn"));
        assert!(has(&tokens, Kind::Keyword, "let"));
        assert!(has(&tokens, Kind::Func, "main"));
        assert!(has(&tokens, Kind::Type, "u32"));
        assert!(has(&tokens, Kind::Number, "0x1F"));
        assert!(has(&tokens, Kind::Comment, "// note"));
        assert!(has(&tokens, Kind::Attr, "println"));
        assert!(has(&tokens, Kind::Str, "\"hi {}\""));
    }

    #[test]
    fn rust_lifetimes_chars_and_raw_strings() {
        let tokens = kinds(
            "rust",
            "fn f<'a>(x: &'a str) -> char { 'z' } let r = r#\"a \"quoted\" b\"#;",
        );
        assert!(has(&tokens, Kind::Type, "'a"));
        assert!(has(&tokens, Kind::Str, "'z'"));
        assert!(has(&tokens, Kind::Str, "r#\"a \"quoted\" b\"#"));
    }

    #[test]
    fn a_quote_that_never_closes_is_plain_and_the_rest_is_still_read() {
        let tokens = kinds("typescript", "const a = it's;\nconst b = 1;\n");
        assert!(has(&tokens, Kind::Keyword, "const"));
        assert!(has(&tokens, Kind::Number, "1"));
        assert!(
            !tokens
                .iter()
                .any(|(k, t)| *k == Kind::Str && t.contains("const b"))
        );
    }

    #[test]
    fn typescript_decorators_types_and_template_strings() {
        let tokens = kinds("ts", "@Component() class A { x: string = `a ${b}`; }");
        assert!(has(&tokens, Kind::Attr, "@Component"));
        assert!(has(&tokens, Kind::Keyword, "class"));
        assert!(has(&tokens, Kind::Type, "string"));
        assert!(has(&tokens, Kind::Str, "`a ${b}`"));
    }

    #[test]
    fn json_keys_are_not_values() {
        let tokens = kinds(
            "json",
            "{\"city\": \"Ludhiana\", \"pin\": 141001, \"ok\": true, \"x\": null}",
        );
        assert!(has(&tokens, Kind::Type, "\"city\""));
        assert!(has(&tokens, Kind::Str, "\"Ludhiana\""));
        assert!(has(&tokens, Kind::Number, "141001"));
        assert!(has(&tokens, Kind::Keyword, "true"));
        assert!(has(&tokens, Kind::Keyword, "null"));
    }

    #[test]
    fn toml_tables_and_keys() {
        let tokens = kinds(
            "toml",
            "# a comment\n[server]\nhost = \"0.0.0.0\"\nport = 8080\ndebug = false\n",
        );
        assert!(has(&tokens, Kind::Comment, "# a comment"));
        assert!(has(&tokens, Kind::Keyword, "[server]"));
        assert!(has(&tokens, Kind::Type, "host"));
        assert!(has(&tokens, Kind::Str, "\"0.0.0.0\""));
        assert!(has(&tokens, Kind::Number, "8080"));
        assert!(has(&tokens, Kind::Keyword, "false"));
    }

    #[test]
    fn yaml_keys_and_dotenv_keys() {
        let tokens = kinds("yaml", "services:\n  - name: web\n    port: 8080 # open\n");
        assert!(has(&tokens, Kind::Type, "services"));
        assert!(has(&tokens, Kind::Type, "name"));
        assert!(has(&tokens, Kind::Comment, "# open"));
        let tokens = kinds("env", "DATABASE_URL=postgres://localhost/app\n# x\n");
        assert!(has(&tokens, Kind::Type, "DATABASE_URL"));
        assert!(has(&tokens, Kind::Comment, "# x"));
    }

    #[test]
    fn shell_comments_variables_flags_and_strings() {
        let tokens = kinds(
            "bash",
            "# build\ncargo build --release -p navree \"$HOME/x\" $PATH\n",
        );
        assert!(has(&tokens, Kind::Comment, "# build"));
        assert!(has(&tokens, Kind::Type, "--release"));
        assert!(has(&tokens, Kind::Type, "-p"));
        assert!(has(&tokens, Kind::Type, "$PATH"));
        // A `$#` is a variable's name, not a comment.
        let tokens = kinds("sh", "echo $# done\n");
        assert!(!tokens.iter().any(|(k, _)| *k == Kind::Comment));
    }

    #[test]
    fn sql_keywords_ignore_case() {
        let tokens = kinds(
            "sql",
            "select id, count(*) FROM orders where total > 1000 -- lakh\n",
        );
        assert!(has(&tokens, Kind::Keyword, "select"));
        assert!(has(&tokens, Kind::Keyword, "FROM"));
        assert!(has(&tokens, Kind::Func, "count"));
        assert!(has(&tokens, Kind::Number, "1000"));
        assert!(has(&tokens, Kind::Comment, "-- lakh"));
    }

    #[test]
    fn css_properties_colours_and_at_rules() {
        let tokens = kinds(
            "css",
            "@media (min-width: 40rem) { .a { color: #0b7a3e; margin: 0 4px !important; } }",
        );
        assert!(has(&tokens, Kind::Attr, "@media"));
        assert!(has(&tokens, Kind::Type, "color"));
        assert!(has(&tokens, Kind::Number, "#0b7a3e"));
        assert!(has(&tokens, Kind::Number, "4px"));
        assert!(has(&tokens, Kind::Keyword, "!important"));
    }

    #[test]
    fn html_tags_attributes_and_comments() {
        let tokens = kinds("html", "<!-- hi --><a href=\"/x\" class='b'>go</a>");
        assert!(has(&tokens, Kind::Comment, "<!-- hi -->"));
        assert!(has(&tokens, Kind::Keyword, "a"));
        assert!(has(&tokens, Kind::Type, "href"));
        assert!(has(&tokens, Kind::Str, "\"/x\""));
        assert!(has(&tokens, Kind::Str, "'b'"));
        let tokens = kinds("jinja", "<p>{{ name }}</p>{% if x %}");
        assert!(has(&tokens, Kind::Attr, "{{ name }}"));
        assert!(has(&tokens, Kind::Attr, "{% if x %}"));
    }

    #[test]
    fn diff_lines_are_added_or_removed() {
        let tokens = kinds("diff", "@@ -1 +1 @@\n-old\n+new\n same\n");
        assert!(has(&tokens, Kind::Keyword, "@@ -1 +1 @@"));
        assert!(has(&tokens, Kind::Del, "-old"));
        assert!(has(&tokens, Kind::Add, "+new"));
    }

    #[test]
    fn http_request_lines_and_headers() {
        let tokens = kinds(
            "http",
            "GET /deals HTTP/1.1\nHost: example.com\n\n{\"a\":1}",
        );
        assert!(has(&tokens, Kind::Keyword, "GET"));
        assert!(has(&tokens, Kind::Type, "Host"));
    }

    #[test]
    fn python_triple_quoted_strings_and_decorators() {
        let tokens = kinds(
            "py",
            "@cache\ndef f():\n    \"\"\"doc\nmore\"\"\"\n    return None\n",
        );
        assert!(has(&tokens, Kind::Attr, "@cache"));
        assert!(has(&tokens, Kind::Keyword, "def"));
        assert!(has(&tokens, Kind::Str, "\"\"\"doc\nmore\"\"\""));
        assert!(has(&tokens, Kind::Keyword, "None"));
    }

    #[test]
    fn unknown_languages_and_huge_code_stay_plain() {
        assert!(highlight("klingon", "x").is_none());
        assert!(highlight("rust", &"a".repeat(MAX_CODE + 1)).is_none());
        assert!(highlight("", "x").is_none());
    }

    #[test]
    fn a_fence_with_more_than_a_name_still_finds_the_language() {
        assert!(highlight("rust title=\"src/main.rs\"", "fn x() {}").is_some());
        assert!(highlight("Rust", "fn x() {}").is_some());
    }

    #[test]
    fn the_tokens_cover_the_code_exactly_and_escape_it() {
        let code = "let s = \"<script>alert(1)</script>\"; // <b>& more";
        let spans = highlight("rust", code).unwrap();
        let html = render(&Node::list(spans));
        assert!(
            !html.contains("<script>") && !html.contains("<b>"),
            "{html}"
        );
        // Strip the spans and the entities: the code comes back whole.
        let mut plain = String::new();
        let mut in_tag = false;
        for c in html.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => plain.push(c),
                _ => {}
            }
        }
        let plain = plain
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&amp;", "&")
            .replace("&#39;", "'");
        assert_eq!(plain, code);
    }

    #[test]
    fn hostile_input_neither_panics_nor_loops() {
        let nasty = [
            "'".repeat(5000),
            "\"".repeat(5001),
            "/*".repeat(3000),
            "<".repeat(4000),
            "<a ".repeat(2000),
            "#[".repeat(2000),
            "r#\"".repeat(2000),
            "😀é".repeat(3000),
            "\0\u{7f}\r\n".repeat(1000),
            "\"\"\"".repeat(2000),
            "{{".repeat(2000),
            "[".repeat(3000),
        ];
        for language in [
            "rust", "ts", "py", "go", "sh", "sql", "json", "toml", "yaml", "env", "css", "html",
            "diff", "http",
        ] {
            for code in &nasty {
                let nodes = highlight(language, code);
                assert!(nodes.is_some(), "{language}");
            }
        }
    }
}
