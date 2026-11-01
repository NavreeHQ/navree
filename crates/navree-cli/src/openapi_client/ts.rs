//! The TypeScript the generator writes: a small tree of type nodes, the
//! helpers that build them, and a printer that lays them out the way
//! TypeScript's own printer does (four-space indent, `;` after members,
//! `(A | B)[]` where a union sits in an array, JSDoc comments re-indented).

use super::json::{Value, js_number};

/// A type.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Ty {
    /// `string`, `number`, `boolean`, `never`, `unknown`, `undefined`.
    Keyword(&'static str),
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A numeric literal, with its sign: `5`, `-1.5`.
    Number(String),
    /// A string literal as JSON writes it: `"lead"`.
    Json(String),
    /// `Name` or `Name<A, B>`.
    Reference(String, Vec<Ty>),
    /// `T["key"]`.
    Indexed(Box<Ty>, String),
    /// `T[]`.
    Array(Box<Ty>),
    /// `[A, B]`.
    Tuple(Vec<Ty>),
    /// `A | B`.
    Union(Vec<Ty>),
    /// `A & B`.
    Intersection(Vec<Ty>),
    /// `{ members }`.
    Literal(Vec<Member>),
}

/// A property or index signature of an object type.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Member {
    /// The text between `/*` and `*/` of the JSDoc comment above it.
    pub(crate) comment: Option<String>,
    pub(crate) readonly: bool,
    pub(crate) key: Key,
    pub(crate) optional: bool,
    pub(crate) ty: Ty,
}

/// What a member is called.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Key {
    /// `name`.
    Ident(String),
    /// `200`.
    Number(String),
    /// `"/api/users"`.
    Quoted(String),
    /// `[name: string]`.
    Index(&'static str),
}

pub(crate) const STRING: Ty = Ty::Keyword("string");
pub(crate) const NUMBER: Ty = Ty::Keyword("number");
pub(crate) const BOOLEAN: Ty = Ty::Keyword("boolean");
pub(crate) const NEVER: Ty = Ty::Keyword("never");
pub(crate) const UNKNOWN: Ty = Ty::Keyword("unknown");

impl Member {
    /// A property with no comment or modifiers.
    pub(crate) fn new(key: Key, ty: Ty) -> Member {
        Member {
            comment: None,
            readonly: false,
            key,
            optional: false,
            ty,
        }
    }

    /// The same property, optional.
    pub(crate) fn optional(mut self, optional: bool) -> Member {
        self.optional = optional;
        self
    }
}

/// `Record<K, V>`.
pub(crate) fn record(key: Ty, value: Ty) -> Ty {
    Ty::Reference("Record".to_owned(), vec![key, value])
}

/// A reference to a type by name.
pub(crate) fn named(name: &str) -> Ty {
    Ty::Reference(name.to_owned(), Vec::new())
}

/// Whether `ty` is a primitive for the purposes of `oneOf` and de-duplication.
pub(crate) fn is_primitive(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Keyword("boolean" | "never" | "number" | "string" | "undefined") | Ty::Null
    )
}

/// Drops a primitive that an earlier member of the list already was.
fn dedupe(types: Vec<Ty>) -> Vec<Ty> {
    let mut seen: Vec<&'static str> = Vec::new();
    let mut kept = Vec::with_capacity(types.len());
    for ty in types {
        let kind = match &ty {
            Ty::Keyword(name) if is_primitive(&ty) => Some(*name),
            Ty::Null => Some("null"),
            _ => None,
        };
        if let Some(kind) = kind {
            if seen.contains(&kind) {
                continue;
            }
            seen.push(kind);
        }
        kept.push(ty);
    }
    kept
}

/// `A | B`, `A` for one member, `never` for none.
pub(crate) fn union(types: Vec<Ty>) -> Ty {
    let mut types = dedupe(types);
    match types.len() {
        0 => NEVER,
        1 => types.remove(0),
        _ => Ty::Union(types),
    }
}

/// `A & B`, `A` for one member, `never` for none.
pub(crate) fn intersection(types: Vec<Ty>) -> Ty {
    let mut types = dedupe(types);
    match types.len() {
        0 => NEVER,
        1 => types.remove(0),
        _ => Ty::Intersection(types),
    }
}

/// `T | null`, with no de-duplication.
pub(crate) fn nullable(ty: Ty) -> Ty {
    Ty::Union(vec![ty, Ty::Null])
}

/// A number as a literal type.
pub(crate) fn number_literal(number: f64) -> Ty {
    if number < 0.0 {
        Ty::Number(format!("-{}", js_number(-number)))
    } else {
        Ty::Number(js_number(number))
    }
}

/// The type of one JSON value: `"x"`, `5`, `true`, `null`, `[...]` as a
/// tuple, an object as a type with the same keys.
pub(crate) fn literal(value: &Value) -> Ty {
    match value {
        Value::Str(text) => Ty::Json(super::json::json_quote(text)),
        Value::Num(number) => number_literal(*number),
        Value::Bool(flag) => Ty::Bool(*flag),
        Value::Null => Ty::Null,
        Value::Arr(items) if items.is_empty() => Ty::Array(Box::new(NEVER)),
        Value::Arr(items) => Ty::Tuple(items.iter().map(literal).collect()),
        Value::Obj(object) => {
            let members: Vec<Member> = object
                .js_entries()
                .into_iter()
                .map(|(key, value)| Member::new(property_key(key), literal(value)))
                .collect();
            if members.is_empty() {
                record(STRING, NEVER)
            } else {
                Ty::Literal(members)
            }
        }
    }
}

/// A string literal type for a name.
pub(crate) fn literal_str(text: &str) -> Ty {
    Ty::Json(super::json::json_quote(text))
}

/// How a property is named: a number when the name reads back as itself
/// through JavaScript's number printing, a bare name when it is an identifier,
/// a quoted string otherwise.
pub(crate) fn property_key(name: &str) -> Key {
    if is_canonical_number(name) {
        return Key::Number(name.to_owned());
    }
    if is_identifier(name) {
        return Key::Ident(name.to_owned());
    }
    Key::Quoted(name.to_owned())
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Whether `String(Number(name)) === name` and `name` does not start with `-`.
fn is_canonical_number(name: &str) -> bool {
    if name == "NaN" || name == "Infinity" {
        return true;
    }
    if name.starts_with('-')
        || name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
    {
        return false;
    }
    name.parse::<f64>()
        .is_ok_and(|number| js_number(number) == name)
}

/// A string literal as TypeScript prints a synthesized one: double quotes,
/// the usual escapes, every non-ASCII code unit as `\uXXXX`.
pub(crate) fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    let chars: Vec<char> = text.chars().collect();
    for (n, ch) in chars.iter().enumerate() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\u{b}' => out.push_str("\\v"),
            '\u{c}' => out.push_str("\\f"),
            '\u{8}' => out.push_str("\\b"),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            '\0' => {
                if chars.get(n + 1).is_some_and(char::is_ascii_digit) {
                    out.push_str("\\x00");
                } else {
                    out.push_str("\\0");
                }
            }
            c if (*c as u32) < 0x20 || (*c as u32) > 0x7f => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04X}"));
                }
            }
            c => out.push(*c),
        }
    }
    out.push('"');
    out
}

/// The JSDoc comment text (between `/*` and `*/`) for a block of lines.
pub(crate) fn doc_comment(lines: &[String]) -> Option<String> {
    let multi_line = lines.iter().any(|line| line.contains('\n'));
    let text = match lines {
        [] => return None,
        [only] if !multi_line => format!("* {only} "),
        _ => format!("*\n * {}\n ", lines.join("\n * ")),
    };
    Some(text.replace("*/", "*\\/"))
}

/// A top-level declaration.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Statement {
    /// `export interface name { members }`.
    Interface(String, Vec<Member>),
    /// `export type name = ty;`.
    Alias(String, Ty),
    /// A declaration kept as written (without `export`).
    Helper(&'static str),
}

/// The text of a file of declarations.
pub(crate) fn print_file(header: &str, statements: &[Statement]) -> String {
    let mut out = String::from(header);
    for statement in statements {
        match statement {
            Statement::Interface(name, members) => {
                out.push_str(&format!("export interface {name} {{\n"));
                print_members(&mut out, members, 1);
                out.push_str("}\n");
            }
            Statement::Alias(name, ty) => {
                out.push_str(&format!("export type {name} = "));
                print_type(&mut out, ty, 0);
                out.push_str(";\n");
            }
            Statement::Helper(text) => {
                out.push_str(text);
                out.push('\n');
            }
        }
    }
    out
}

fn indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("    ");
    }
}

fn print_members(out: &mut String, members: &[Member], level: usize) {
    for member in members {
        if let Some(comment) = &member.comment {
            print_comment(out, comment, level);
        }
        indent(out, level);
        if member.readonly {
            out.push_str("readonly ");
        }
        match &member.key {
            Key::Ident(name) | Key::Number(name) => out.push_str(name),
            Key::Quoted(name) => out.push_str(&quote(name)),
            Key::Index(name) => {
                out.push_str(&format!("[{name}: string]"));
            }
        }
        if member.optional {
            out.push('?');
        }
        out.push_str(": ");
        print_type(out, &member.ty, level);
        out.push_str(";\n");
    }
}

/// `text` between `/*` and `*/`, one line at a time: the first at the
/// member's indent, the rest at that indent plus the spaces they start with.
fn print_comment(out: &mut String, text: &str, level: usize) {
    let full = format!("/*{text}*/");
    let mut starts = vec![0usize];
    let mut chars = full.char_indices().peekable();
    while let Some((at, ch)) = chars.next() {
        match ch {
            '\r' => {
                if let Some((_, '\n')) = chars.peek() {
                    chars.next();
                    starts.push(at + 2);
                } else {
                    starts.push(at + 1);
                }
            }
            '\n' | '\u{2028}' | '\u{2029}' => starts.push(at + ch.len_utf8()),
            _ => {}
        }
    }
    for (n, start) in starts.iter().enumerate() {
        let next = starts.get(n + 1).copied();
        // The line ends where its break character starts.
        let end = next.map_or(full.len(), |next| {
            full.get(..next)
                .and_then(|upto| upto.char_indices().next_back())
                .map_or(next, |(at, _)| at)
        });
        let line = full.get(*start..end.max(*start)).unwrap_or("");
        let trimmed = js_trim(line);
        if n == 0 {
            indent(out, level);
        } else {
            let lead = leading_space(full.get(*start..).unwrap_or(""));
            for _ in 0..(level * 4 + lead) {
                out.push(' ');
            }
        }
        if trimmed.is_empty() {
            out.push('\n');
            continue;
        }
        out.push_str(trimmed);
        if next.is_some() {
            out.push('\n');
        }
    }
    out.push('\n');
}

/// The width of the whitespace a line starts with, tabs to the next stop of 4.
fn leading_space(line: &str) -> usize {
    let mut width = 0;
    for ch in line.chars() {
        if ch == '\t' {
            width += 4 - (width % 4);
        } else if is_inline_space(ch) {
            width += 1;
        } else {
            break;
        }
    }
    width
}

fn is_inline_space(ch: char) -> bool {
    matches!(ch, '\u{b}' | '\u{c}' | '\u{feff}' | '\u{200b}')
        || (ch.is_whitespace() && !is_line_break(ch))
}

fn is_line_break(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// JavaScript's `String.prototype.trim`.
pub(crate) fn js_trim(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

/// Whether a union or intersection member, array element or indexed object
/// needs parentheses.
fn needs_parens(ty: &Ty) -> bool {
    matches!(ty, Ty::Union(_) | Ty::Intersection(_))
}

fn print_wrapped(out: &mut String, ty: &Ty, level: usize) {
    if needs_parens(ty) {
        out.push('(');
        print_type(out, ty, level);
        out.push(')');
    } else {
        print_type(out, ty, level);
    }
}

fn print_type(out: &mut String, ty: &Ty, level: usize) {
    match ty {
        Ty::Keyword(name) => out.push_str(name),
        Ty::Null => out.push_str("null"),
        Ty::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Ty::Number(text) | Ty::Json(text) => out.push_str(text),
        Ty::Reference(name, arguments) => {
            out.push_str(name);
            if !arguments.is_empty() {
                out.push('<');
                for (n, argument) in arguments.iter().enumerate() {
                    if n > 0 {
                        out.push_str(", ");
                    }
                    print_type(out, argument, level);
                }
                out.push('>');
            }
        }
        Ty::Indexed(object, key) => {
            print_wrapped(out, object, level);
            out.push('[');
            out.push_str(&quote(key));
            out.push(']');
        }
        Ty::Array(element) => {
            print_wrapped(out, element, level);
            out.push_str("[]");
        }
        Ty::Tuple(elements) => {
            out.push_str("[\n");
            for (n, element) in elements.iter().enumerate() {
                indent(out, level + 1);
                print_type(out, element, level + 1);
                if n + 1 < elements.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            indent(out, level);
            out.push(']');
        }
        Ty::Union(members) => print_joined(out, members, " | ", level),
        Ty::Intersection(members) => print_joined(out, members, " & ", level),
        Ty::Literal(members) => {
            out.push_str("{\n");
            print_members(out, members, level + 1);
            indent(out, level);
            out.push('}');
        }
    }
}

fn print_joined(out: &mut String, members: &[Ty], separator: &str, level: usize) {
    for (n, member) in members.iter().enumerate() {
        if n > 0 {
            out.push_str(separator);
        }
        print_wrapped(out, member, level);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn printed(ty: &Ty) -> String {
        let mut out = String::new();
        print_type(&mut out, ty, 0);
        out
    }

    #[test]
    fn names_properties_the_way_a_declaration_needs() {
        assert_eq!(property_key("id"), Key::Ident("id".to_owned()));
        assert_eq!(property_key("$ref"), Key::Ident("$ref".to_owned()));
        assert_eq!(property_key("200"), Key::Number("200".to_owned()));
        assert_eq!(property_key("1.5"), Key::Number("1.5".to_owned()));
        assert_eq!(property_key("1e21"), Key::Quoted("1e21".to_owned()));
        assert_eq!(property_key("1e+21"), Key::Number("1e+21".to_owned()));
        assert_eq!(property_key("-1"), Key::Quoted("-1".to_owned()));
        assert_eq!(property_key("01"), Key::Quoted("01".to_owned()));
        assert_eq!(property_key("2XX"), Key::Quoted("2XX".to_owned()));
        assert_eq!(property_key("/api/x"), Key::Quoted("/api/x".to_owned()));
        assert_eq!(property_key(""), Key::Quoted(String::new()));
        assert_eq!(property_key("NaN"), Key::Number("NaN".to_owned()));
    }

    #[test]
    fn escapes_strings_the_way_the_printer_does() {
        assert_eq!(quote("a\"b\\c\n\t"), "\"a\\\"b\\\\c\\n\\t\"");
        assert_eq!(quote("é\u{1F600}"), "\"\\u00E9\\uD83D\\uDE00\"");
        assert_eq!(quote("\0"), "\"\\0\"");
        assert_eq!(quote("\u{0}1"), "\"\\x001\"");
        assert_eq!(quote("\u{1}"), "\"\\u0001\"");
        assert_eq!(quote("\u{7f}"), "\"\u{7f}\"");
        assert_eq!(quote("'`"), "\"'`\"");
    }

    #[test]
    fn puts_parentheses_where_a_union_sits_inside_another_type() {
        let list = Ty::Array(Box::new(union(vec![STRING, NUMBER])));
        assert_eq!(printed(&list), "(string | number)[]");
        assert_eq!(
            printed(&nullable(union(vec![STRING, NUMBER]))),
            "(string | number) | null"
        );
        assert_eq!(
            printed(&intersection(vec![
                named("A"),
                union(vec![named("B"), named("C")])
            ])),
            "A & (B | C)"
        );
    }

    #[test]
    fn drops_a_repeated_primitive_but_not_a_repeated_literal() {
        assert_eq!(
            printed(&union(vec![STRING, STRING, Ty::Null, Ty::Null])),
            "string | null"
        );
        assert_eq!(
            printed(&union(vec![literal_str("a"), literal_str("a")])),
            "\"a\" | \"a\""
        );
        assert_eq!(printed(&union(vec![UNKNOWN, UNKNOWN])), "unknown | unknown");
    }

    #[test]
    fn lays_out_members_and_comments() {
        let mut member = Member::new(Key::Ident("id".to_owned()), STRING);
        member.comment =
            doc_comment(&["Format: uuid".to_owned(), "@description The id.".to_owned()]);
        let mut single = Member::new(Key::Quoted("a-b".to_owned()), NUMBER).optional(true);
        single.comment = doc_comment(&["@description One.".to_owned()]);
        single.readonly = true;
        let file = print_file(
            "",
            &[Statement::Interface("x".to_owned(), vec![member, single])],
        );
        assert_eq!(
            file,
            "export interface x {\n    /**\n     * Format: uuid\n     * @description The id.\n     */\n    id: string;\n    /** @description One. */\n    readonly \"a-b\"?: number;\n}\n"
        );
    }

    #[test]
    fn prints_a_literal_type_across_lines() {
        let ty = Ty::Array(Box::new(Ty::Literal(vec![Member::new(
            Key::Ident("a".to_owned()),
            Ty::Literal(vec![Member::new(Key::Index("key"), UNKNOWN)]),
        )])));
        assert_eq!(
            printed(&ty),
            "{\n    a: {\n        [key: string]: unknown;\n    };\n}[]"
        );
    }

    #[test]
    fn literal_types_of_json_values() {
        let value =
            crate::openapi_client::json::parse(r#"["a",-2,true,null,{"1":[]},[]]"#).unwrap();
        assert_eq!(
            printed(&literal(&value)),
            "[\n    \"a\",\n    -2,\n    true,\n    null,\n    {\n        1: never[];\n    },\n    never[]\n]"
        );
    }
}
