//! The scoper behind `extension_sheet!`.
//!
//! A plugin's stylesheet is read with the tokenizer browsers use, checked against what a plugin's
//! sheet may hold, and written back out from what was read, never from the text it came in. The
//! written sheet is the hoisted at-rules followed by one wrapper, `:where([data-slot^="ns:"])`,
//! that holds every other rule, so a rule can only reach an element that is a component's root or
//! inside one. The written sheet is read again, and it must be what was checked.
//!
//! The steps, each a function below:
//!
//! 1. `tokenize` reads every token of the sheet into a tree: a block holds the tokens inside it,
//!    a comment is dropped, a run of white space is one token. A block that is never closed, a
//!    closing bracket that matches nothing, a string that breaks across lines and a malformed
//!    `url(` are refused here.
//! 2. `parse_rules` and `parse_members` give the tokens their structure: rules, at-rules and
//!    declarations, as the browsers' parsing rules cut them. An at-rule outside the list is refused
//!    here. A declaration a browser would drop is dropped.
//! 3. `Scoper` checks every selector, name and address, hoists the at-rules that name something
//!    global, rewrites `:root` and builds the wrapper.
//! 4. `write_rules` writes the result. Every token is written again from what was read: names and
//!    strings with the escapes cssparser writes, numbers as they were written (they were checked to
//!    be only digits, a point, an exponent and a sign), white space as one space, comments never.
//! 5. `rules` reads the written text again and `scope` compares it with what was checked.

use cssparser::{
    CowRcStr, ParseError, ParseErrorKind, Parser, ToCss, Token, TokenSerializationType,
};

/// The most a namespace may hold.
const NAMESPACE_LENGTH: usize = 64;

/// The CSS-wide keywords and the ones the properties that name things across the page take.
const NAME_KEYWORDS: [&str; 8] = [
    "none",
    "auto",
    "match-element",
    "inherit",
    "initial",
    "unset",
    "revert",
    "revert-layer",
];

// ------------------------------------------------------------------------------------------ data

/// Where a token sits in the source: its line (from 1), its byte range, and whether a comment sat
/// right after the token before it with no white space between. Never compared: a sheet and the
/// text it was written as are equal when they hold the same things.
#[derive(Clone, Copy, Debug, Default)]
struct Meta {
    line: u32,
    start: usize,
    end: usize,
    glued: bool,
}

impl PartialEq for Meta {
    fn eq(&self, _other: &Meta) -> bool {
        true
    }
}

/// One token of the sheet, or a block with the tokens inside it.
#[derive(Clone, Debug, PartialEq)]
struct Tok {
    meta: Meta,
    kind: Kind,
}

impl Tok {
    /// A token that was never in the source.
    fn new(kind: Kind) -> Tok {
        Tok {
            meta: Meta::default(),
            kind,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    /// A run of white space and comments with white space in it.
    Space,
    /// A name, unescaped.
    Ident(String),
    /// The word after an `@`, unescaped.
    AtKeyword(String),
    /// A `#` and a name; `id` when the name is an identifier.
    Hash {
        name: String,
        id: bool,
    },
    /// A string, unescaped.
    Str(String),
    /// An address written as `url(` and no quote, unescaped.
    Url(String),
    Delim(char),
    /// The digits of a number as they were written.
    Num(String),
    /// The digits of a percentage as they were written.
    Pct(String),
    /// The digits as they were written, and the unit unescaped.
    Dim(String, String),
    Colon,
    Semi,
    Comma,
    IncludeMatch,
    DashMatch,
    PrefixMatch,
    SuffixMatch,
    SubstringMatch,
    /// The opener of an HTML comment, which a sheet's top level skips between its rules.
    Cdo,
    /// The closer of an HTML comment, which a sheet's top level skips between its rules.
    Cdc,
    /// A function's name and what is between its brackets.
    Function(String, Vec<Tok>),
    Paren(Vec<Tok>),
    Square(Vec<Tok>),
    Curly(Vec<Tok>),
}

/// A rule of a rule list: the sheet's own, or the body of an at-rule that holds rules.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Rule {
    Style(Style),
    At(At),
}

/// A rule with a selector list and a block.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Style {
    meta: Meta,
    prelude: Vec<Tok>,
    body: Vec<Member>,
}

/// What a block holds: declarations, nested rules and nested at-rules.
#[derive(Clone, Debug, PartialEq)]
enum Member {
    Decl(Decl),
    Style(Style),
    At(At),
}

#[derive(Clone, Debug, PartialEq)]
struct Decl {
    meta: Meta,
    /// The property's name, unescaped and in the case it was written.
    name: String,
    value: Vec<Tok>,
    important: bool,
}

/// An at-rule: its name, what comes before its block, and its block when it has one.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct At {
    meta: Meta,
    name: String,
    prelude: Vec<Tok>,
    body: Option<Body>,
}

#[derive(Clone, Debug, PartialEq)]
enum Body {
    Rules(Vec<Rule>),
    Members(Vec<Member>),
}

/// Why a sheet is refused, with the line when there is one.
#[derive(Debug)]
enum Fail {
    Unbalanced,
    /// Something the sheet holds that would not be read the same once written. A line of 0 is the
    /// whole sheet.
    NotPlain {
        line: u32,
    },
    AtRule {
        name: String,
        line: u32,
    },
    Selector {
        text: String,
        line: u32,
    },
    Global {
        name: String,
        form: String,
        line: u32,
    },
    Url {
        line: u32,
    },
}

impl Fail {
    /// The compile error for `file`.
    fn text(&self, file: &str) -> String {
        const PLAIN: &str = "the sheet does not parse the same once served; write it in plain CSS";
        match self {
            Fail::Unbalanced => format!("extension_sheet!: {file}: the blocks do not balance"),
            Fail::NotPlain { line: 0 } => format!("extension_sheet!: {file}: {PLAIN}"),
            Fail::NotPlain { line } => format!("extension_sheet!: {file}:{line}: {PLAIN}"),
            Fail::AtRule { name, line } => format!(
                "extension_sheet!: {file}:{line}: @{name} is not taken: a plugin's sheet styles its own components, and bundles what it needs"
            ),
            Fail::Selector { text, line } => format!(
                "extension_sheet!: {file}:{line}: the selector `{text}` can style outside the component: & appears once, outside :is/:where/:not/:has, and never before + or ~"
            ),
            Fail::Global { name, form, line } => format!(
                "extension_sheet!: {file}:{line}: `{name}` is a global name and must be {form}"
            ),
            Fail::Url { line } => format!(
                "extension_sheet!: {file}:{line}: url(..) in a plugin's sheet is a data: URL; ship pictures as AppImage and pass their address"
            ),
        }
    }
}

// ----------------------------------------------------------------------------------- entry points

/// The text of the refusal when `namespace` is not a namespace: a lower-case letter followed by up
/// to 63 lower-case letters, digits or `_`.
pub(crate) fn namespace_error(namespace: &str) -> Option<String> {
    let bytes = namespace.as_bytes();
    let fine = !bytes.is_empty()
        && bytes.len() <= NAMESPACE_LENGTH
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_'));
    (!fine).then(|| {
        format!(
            "extension_sheet!: `{namespace}` is not a namespace (a lower-case letter followed by up to 63 lower-case letters, digits or _)"
        )
    })
}

/// The sheet as it is served: `source` checked, scoped to `namespace` and written from what was
/// read. `file` is the path as the macro's caller wrote it, for the messages. The `Err` is the
/// whole compile error.
pub(crate) fn scope(source: &str, namespace: &str, file: &str) -> Result<String, String> {
    scope_written(source, namespace, file, write_rules)
}

/// `scope` with the writer given, so that a test can show that a writer that loses something is
/// caught by reading its text again.
pub(crate) fn scope_written(
    source: &str,
    namespace: &str,
    file: &str,
    write: fn(&[Rule]) -> String,
) -> Result<String, String> {
    let scoped = scoped(source, namespace, file)?;
    let served = write(&scoped);
    // What was written must be read back as what was checked.
    match rules(&served, file) {
        Ok(again) if again == scoped => Ok(served),
        _ => Err(Fail::NotPlain { line: 0 }.text(file)),
    }
}

/// The rules of the served sheet before they are written: the hoisted at-rules and the wrapper.
pub(crate) fn scoped(source: &str, namespace: &str, file: &str) -> Result<Vec<Rule>, String> {
    if let Some(text) = namespace_error(namespace) {
        return Err(text);
    }
    // A byte order mark is the file's encoding, not the sheet's first token.
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let run = || -> Result<Vec<Rule>, Fail> {
        let tokens = tokenize(source)?;
        let rules = parse_rules(&tokens, Place::Top)?;
        Scoper { namespace, source }.sheet(rules)
    };
    run().map_err(|fail| fail.text(file))
}

/// The rules of `css`, read as a sheet's top level reads them and nothing checked: what the served
/// text is compared by. White space and comments make no difference between two sheets; where
/// white space separates two tokens it is one space.
pub(crate) fn rules(css: &str, file: &str) -> Result<Vec<Rule>, String> {
    let run = || -> Result<Vec<Rule>, Fail> {
        let tokens = tokenize(css)?;
        parse_rules(&tokens, Place::Top)
    };
    run().map_err(|fail| fail.text(file))
}

// ----------------------------------------------------------------------------------- tokenizing

/// Every token of `source`, in a tree.
fn tokenize(source: &str) -> Result<Vec<Tok>, Fail> {
    let mut input = Parser::new(source);
    read(&mut input, source).map_err(|error| match error.kind {
        ParseErrorKind::Custom(fail) => fail,
        ParseErrorKind::Basic(_) => Fail::NotPlain { line: 0 },
    })
}

/// The tokens up to the end of the block `input` is in, or of the sheet.
fn read<'i>(input: &mut Parser<'i>, source: &'i str) -> Result<Vec<Tok>, ParseError<Fail>> {
    let mut out: Vec<Tok> = Vec::new();
    // A comment came right after the last token, with no white space between.
    let mut glued = false;
    loop {
        let start = input.position();
        let line = input.current_source_location().line + 1;
        let token = match input.next_including_whitespace_and_comments() {
            Ok(token) => token.clone(),
            Err(_) => return Ok(out),
        };
        let raw = input.slice(start..input.position());
        let plain = || -> ParseError<Fail> { ParseError::custom(Fail::NotPlain { line }) };
        let kind = match token {
            Token::WhiteSpace(_) => Kind::Space,
            Token::Comment(_) => {
                glued = !matches!(out.last().map(|t| &t.kind), Some(Kind::Space) | None);
                continue;
            }
            Token::Ident(name) => Kind::Ident(name.to_string()),
            Token::AtKeyword(name) => Kind::AtKeyword(name.to_string()),
            Token::Hash(name) => Kind::Hash {
                name: name.to_string(),
                id: false,
            },
            Token::IDHash(name) => Kind::Hash {
                name: name.to_string(),
                id: true,
            },
            Token::QuotedString(value) => Kind::Str(value.to_string()),
            Token::UnquotedUrl(value) => Kind::Url(value.to_string()),
            // A backslash that escapes nothing: written back it could join the next token.
            Token::Delim('\\') => return Err(plain()),
            Token::Delim(c) => Kind::Delim(c),
            Token::Number { .. } => match numeric_length(raw) {
                Some(length) if length == raw.len() => Kind::Num(raw.to_owned()),
                _ => return Err(plain()),
            },
            Token::Percentage { .. } => match raw.strip_suffix('%') {
                Some(digits) if numeric_length(digits) == Some(digits.len()) => {
                    Kind::Pct(digits.to_owned())
                }
                _ => return Err(plain()),
            },
            Token::Dimension { unit, .. } => match numeric_length(raw) {
                Some(length) if length < raw.len() => {
                    Kind::Dim(raw[..length].to_owned(), unit.to_string())
                }
                _ => return Err(plain()),
            },
            Token::Colon => Kind::Colon,
            Token::Semicolon => Kind::Semi,
            Token::Comma => Kind::Comma,
            Token::IncludeMatch => Kind::IncludeMatch,
            Token::DashMatch => Kind::DashMatch,
            Token::PrefixMatch => Kind::PrefixMatch,
            Token::SuffixMatch => Kind::SuffixMatch,
            Token::SubstringMatch => Kind::SubstringMatch,
            Token::CDO => Kind::Cdo,
            Token::CDC => Kind::Cdc,
            Token::Function(name) => Kind::Function(name.to_string(), nested(input, source, line)?),
            Token::ParenthesisBlock => Kind::Paren(nested(input, source, line)?),
            Token::SquareBracketBlock => Kind::Square(nested(input, source, line)?),
            Token::CurlyBracketBlock => Kind::Curly(nested(input, source, line)?),
            Token::BadUrl(_) => return Err(ParseError::custom(Fail::Url { line })),
            Token::BadString(_) => return Err(plain()),
            Token::CloseParenthesis | Token::CloseSquareBracket | Token::CloseCurlyBracket => {
                return Err(ParseError::custom(Fail::Unbalanced));
            }
        };
        let space = matches!(kind, Kind::Space);
        if !(space && matches!(out.last().map(|t| &t.kind), Some(Kind::Space))) {
            out.push(Tok {
                meta: Meta {
                    line,
                    start: start.byte_index(),
                    end: input.position().byte_index(),
                    glued: glued && !space,
                },
                kind,
            });
        }
        glued = false;
    }
}

/// The tokens of the block or function `input` has just opened. A block still open at the end of
/// the sheet is refused: the parser would close it, and a browser does the same.
fn nested<'i>(
    input: &mut Parser<'i>,
    source: &'i str,
    line: u32,
) -> Result<Vec<Tok>, ParseError<Fail>> {
    input
        .parse_nested_block(|inner| -> Result<Vec<Tok>, ParseError<Fail>> {
            let tokens = read(inner, source)?;
            if inner.position().byte_index() >= source.len() {
                return Err(ParseError::custom(Fail::Unbalanced));
            }
            Ok(tokens)
        })
        .map_err(|error| match error.kind {
            // Nested too deep for the parser.
            ParseErrorKind::Basic(_) => ParseError::custom(Fail::NotPlain { line }),
            ParseErrorKind::Custom(_) => error,
        })
}

/// How many bytes of `text` are a number: a sign, digits, a point and digits, an exponent. `None`
/// when `text` does not start with one. This is the tokenizer's own reading of a number.
fn numeric_length(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let digit = |at: usize| bytes.get(at).is_some_and(u8::is_ascii_digit);
    let mut at = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        at += 1;
    }
    let digits_from = at;
    while digit(at) {
        at += 1;
    }
    if bytes.get(at) == Some(&b'.') && digit(at + 1) {
        at += 1;
        while digit(at) {
            at += 1;
        }
    }
    if at == digits_from {
        return None;
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        let signed = matches!(bytes.get(at + 1), Some(b'+' | b'-'));
        if digit(at + 1) || (signed && digit(at + 2)) {
            at += if signed { 2 } else { 1 };
            while digit(at) {
                at += 1;
            }
        }
    }
    Some(at)
}

// ------------------------------------------------------------------------------------- structure

/// Where a list of rules or a block is, which says which at-rules may appear in it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    /// The sheet's own rules.
    Top,
    /// Inside a rule, or inside an at-rule that groups rules.
    Block,
    /// The frames of `@keyframes`.
    Keyframes,
    /// The declarations of `@font-face`, `@property`, `@counter-style` or a keyframe.
    Declarations,
}

/// How the block an at-rule opens is read: as a list of rules, as a sheet's top level reads it, or
/// as a block of declarations and nested rules, as it is read inside a rule.
#[derive(Clone, Copy)]
enum Mode {
    Rules,
    Members,
}

/// What an at-rule is, by the name it is taken under.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// `@keyframes`: frames.
    Keyframes,
    /// `@property`, `@font-face` and `@counter-style`: declarations.
    Descriptors,
    /// `@media`, `@supports`, `@container`, `@starting-style` and `@layer`.
    Group,
}

fn is_space(token: &Tok) -> bool {
    matches!(token.kind, Kind::Space)
}

/// `tokens` without the white space at either end.
fn trimmed(tokens: &[Tok]) -> &[Tok] {
    let start = tokens
        .iter()
        .position(|token| !is_space(token))
        .unwrap_or(tokens.len());
    let end = tokens
        .iter()
        .rposition(|token| !is_space(token))
        .map_or(start, |last| last + 1);
    &tokens[start..end]
}

/// `tokens` split at the commas between them, blocks being whole tokens.
fn split_commas(tokens: &[Tok]) -> Vec<&[Tok]> {
    tokens
        .split(|token| matches!(token.kind, Kind::Comma))
        .collect()
}

/// The first `{}` block at or after `from`, with where it is.
fn find_block(tokens: &[Tok], from: usize) -> Option<(usize, &[Tok])> {
    tokens
        .iter()
        .enumerate()
        .skip(from)
        .find_map(|(at, token)| match &token.kind {
            Kind::Curly(inner) => Some((at, inner.as_slice())),
            _ => None,
        })
}

/// What follows an at-keyword: the prelude up to a semicolon or a block, the block's contents when
/// there is one, and where the next item starts.
fn at_rule_parts(tokens: &[Tok], from: usize) -> (&[Tok], Option<&[Tok]>, usize) {
    for (at, token) in tokens.iter().enumerate().skip(from) {
        match &token.kind {
            Kind::Semi => return (&tokens[from..at], None, at + 1),
            Kind::Curly(inner) => return (&tokens[from..at], Some(inner.as_slice()), at + 1),
            _ => {}
        }
    }
    (&tokens[from..], None, tokens.len())
}

/// A list of rules: the sheet's, or what an at-rule that groups rules holds.
fn parse_rules(tokens: &[Tok], place: Place) -> Result<Vec<Rule>, Fail> {
    let mut rules = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        let token = &tokens[at];
        match &token.kind {
            Kind::Space => at += 1,
            // A sheet that was once inside a page's style element can start and end with these.
            Kind::Cdo | Kind::Cdc if place == Place::Top => at += 1,
            Kind::AtKeyword(name) => {
                let (prelude, body, next) = at_rule_parts(tokens, at + 1);
                let rule = at_rule(token, name, prelude, body, place, Mode::Rules)?;
                rules.push(Rule::At(rule));
                at = next;
            }
            _ => {
                let Some((open, inner)) = find_block(tokens, at) else {
                    // A prelude that never opens a block: a browser drops it, and so would the
                    // rule after it have been dropped with it.
                    return Err(Fail::NotPlain {
                        line: token.meta.line,
                    });
                };
                let body_place = if place == Place::Keyframes {
                    Place::Declarations
                } else {
                    Place::Block
                };
                rules.push(Rule::Style(Style {
                    meta: token.meta,
                    prelude: trimmed(&tokens[at..open]).to_vec(),
                    body: parse_members(inner, body_place)?,
                }));
                at = open + 1;
            }
        }
    }
    Ok(rules)
}

/// A block of declarations and nested rules.
fn parse_members(tokens: &[Tok], place: Place) -> Result<Vec<Member>, Fail> {
    let mut members = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        let token = &tokens[at];
        match &token.kind {
            Kind::Space | Kind::Semi => at += 1,
            Kind::AtKeyword(name) => {
                let (prelude, body, next) = at_rule_parts(tokens, at + 1);
                let rule = at_rule(token, name, prelude, body, place, Mode::Members)?;
                members.push(Member::At(rule));
                at = next;
            }
            _ => {
                // An item ends at its first semicolon or block: with a block it is a nested rule
                // (unless it is a custom property, whose value may hold a block), without one it
                // is a declaration.
                let stop = tokens[at..]
                    .iter()
                    .position(|t| matches!(t.kind, Kind::Semi | Kind::Curly(_)))
                    .map(|found| at + found);
                let end = stop.unwrap_or(tokens.len());
                match stop.map(|found| &tokens[found].kind) {
                    Some(Kind::Curly(inner)) if !opens_a_custom_property(&tokens[at..end]) => {
                        if place == Place::Declarations {
                            return Err(Fail::NotPlain {
                                line: token.meta.line,
                            });
                        }
                        members.push(Member::Style(Style {
                            meta: token.meta,
                            prelude: trimmed(&tokens[at..end]).to_vec(),
                            body: parse_members(inner, Place::Block)?,
                        }));
                        at = end + 1;
                    }
                    Some(Kind::Curly(_)) => {
                        // The value of a custom property runs to the next semicolon, and a block
                        // in it is one token.
                        let end = tokens[at..]
                            .iter()
                            .position(|t| matches!(t.kind, Kind::Semi))
                            .map_or(tokens.len(), |found| at + found);
                        members.extend(declaration(&tokens[at..end]).map(Member::Decl));
                        at = end + 1;
                    }
                    _ => {
                        members.extend(declaration(&tokens[at..end]).map(Member::Decl));
                        at = end + 1;
                    }
                }
            }
        }
    }
    Ok(members)
}

/// Whether `prefix` starts with a custom property's name and a colon.
fn opens_a_custom_property(prefix: &[Tok]) -> bool {
    let mut tokens = prefix.iter().filter(|token| !is_space(token));
    matches!(
        tokens.next(),
        Some(Tok { kind: Kind::Ident(name), .. }) if name.starts_with("--")
    ) && matches!(
        tokens.next(),
        Some(Tok {
            kind: Kind::Colon,
            ..
        })
    )
}

/// A declaration, or `None` for what a browser drops: an item that is not a name, a colon and a
/// value (`*zoom: 1` is one).
fn declaration(tokens: &[Tok]) -> Option<Decl> {
    // Only the white space before the name goes: what follows the colon is kept whole, so that a
    // value of white space only can be told from no value.
    let tokens = match tokens.first() {
        Some(token) if is_space(token) => &tokens[1..],
        _ => tokens,
    };
    let (first, rest) = tokens.split_first()?;
    let Kind::Ident(name) = &first.kind else {
        return None;
    };
    let rest = match rest.first() {
        Some(token) if is_space(token) => &rest[1..],
        _ => rest,
    };
    let (colon, raw) = rest.split_first()?;
    if !matches!(colon.kind, Kind::Colon) {
        return None;
    }
    let mut value = trimmed(raw);
    let mut important = false;
    if let Some(word) = value.last()
        && matches!(&word.kind, Kind::Ident(text) if text.eq_ignore_ascii_case("important"))
    {
        let before = trimmed(&value[..value.len() - 1]);
        if let Some(bang) = before.last()
            && matches!(bang.kind, Kind::Delim('!'))
        {
            important = true;
            value = trimmed(&before[..before.len() - 1]);
        }
    }
    // A custom property whose value is white space only keeps it (`--off: ;`, which is not the
    // same as `--off:;`). An ordinary property with no value has none.
    let spaced = name.starts_with("--") && !raw.is_empty() && raw.iter().all(is_space);
    let value = if value.is_empty() && spaced {
        vec![Tok::new(Kind::Space)]
    } else {
        value.to_vec()
    };
    Some(Decl {
        meta: first.meta,
        name: name.clone(),
        value,
        important,
    })
}

/// The prelude of an at-rule: the white space after its name is kept, the white space before its
/// block is not.
fn at_prelude(tokens: &[Tok]) -> Vec<Tok> {
    let end = tokens
        .iter()
        .rposition(|token| !is_space(token))
        .map_or(0, |last| last + 1);
    tokens[..end].to_vec()
}

/// An at-rule of the list, read for where it is: at the top of the sheet the at-rules that name
/// something global and the ones that group rules, inside a block only the ones that group rules.
fn at_rule(
    token: &Tok,
    name: &str,
    prelude: &[Tok],
    body: Option<&[Tok]>,
    place: Place,
    mode: Mode,
) -> Result<At, Fail> {
    let line = token.meta.line;
    let lower = name.to_ascii_lowercase();
    let shape = match (place, lower.as_str()) {
        (Place::Top, "keyframes") => Shape::Keyframes,
        (Place::Top, "property" | "font-face" | "counter-style") => Shape::Descriptors,
        (
            Place::Top | Place::Block,
            "media" | "supports" | "container" | "starting-style" | "layer",
        ) => Shape::Group,
        _ => {
            return Err(Fail::AtRule {
                name: name.to_owned(),
                line,
            });
        }
    };
    let body = match (shape, body) {
        (Shape::Keyframes, Some(inner)) => Some(Body::Rules(parse_rules(inner, Place::Keyframes)?)),
        (Shape::Descriptors, Some(inner)) => {
            Some(Body::Members(parse_members(inner, Place::Declarations)?))
        }
        (Shape::Group, Some(inner)) => Some(match mode {
            Mode::Rules => Body::Rules(parse_rules(inner, Place::Block)?),
            Mode::Members => Body::Members(parse_members(inner, Place::Block)?),
        }),
        // A statement: only a layer's, and only at the top.
        (Shape::Group, None) if lower == "layer" && place == Place::Top => None,
        _ => return Err(Fail::NotPlain { line }),
    };
    Ok(At {
        meta: token.meta,
        name: name.to_owned(),
        prelude: at_prelude(prelude),
        body,
    })
}

// ---------------------------------------------------------------------------------------- scoping

/// The checks and the rewriting of a sheet read for `namespace`.
struct Scoper<'a> {
    namespace: &'a str,
    source: &'a str,
}

impl Scoper<'_> {
    /// The served rules: the at-rules that name something global, in source order, then the
    /// wrapper that holds everything else.
    fn sheet(&self, items: Vec<Rule>) -> Result<Vec<Rule>, Fail> {
        let mut served = Vec::new();
        let mut inside = Vec::new();
        for item in items {
            match item {
                Rule::Style(style) => inside.push(Member::Style(self.style(style, true)?)),
                Rule::At(at) => match at.name.to_ascii_lowercase().as_str() {
                    "keyframes" => {
                        self.keyframes(&at)?;
                        served.push(Rule::At(at));
                    }
                    "property" => {
                        self.property(&at)?;
                        served.push(Rule::At(at));
                    }
                    "font-face" => {
                        self.font_face(&at)?;
                        served.push(Rule::At(at));
                    }
                    "counter-style" => {
                        self.counter_style(&at)?;
                        served.push(Rule::At(at));
                    }
                    "layer" if at.body.is_none() => {
                        self.layer_names(&at, true)?;
                        served.push(Rule::At(at));
                    }
                    _ => inside.push(Member::At(self.group(at, true)?)),
                },
            }
        }
        served.push(Rule::Style(Style {
            meta: Meta::default(),
            prelude: wrapper_prelude(self.namespace),
            body: inside,
        }));
        Ok(served)
    }

    /// A style rule: its selectors checked, and its block. `rooted` when no rule holds it, so a
    /// selector that starts with `:root` means the component's root.
    fn style(&self, style: Style, rooted: bool) -> Result<Style, Fail> {
        let Style {
            meta,
            prelude,
            body,
        } = style;
        walk(&prelude, false)?;
        let prelude = self.selectors(&prelude, meta.line, rooted)?;
        let body = self.members(body)?;
        Ok(Style {
            meta,
            prelude,
            body,
        })
    }

    /// The members of a block, each checked.
    fn members(&self, members: Vec<Member>) -> Result<Vec<Member>, Fail> {
        members
            .into_iter()
            .map(|member| match member {
                Member::Decl(decl) => self.decl(&decl).map(|()| Member::Decl(decl)),
                Member::Style(style) => self.style(style, false).map(Member::Style),
                Member::At(at) => self.group(at, false).map(Member::At),
            })
            .collect()
    }

    /// An at-rule that groups rules (`@media`, `@supports`, `@container`, `@starting-style`,
    /// `@layer` with a block), kept inside the wrapper. The block comes out read as a block inside
    /// a rule, which is where it is now.
    fn group(&self, at: At, rooted: bool) -> Result<At, Fail> {
        let At {
            meta,
            name,
            prelude,
            body,
        } = at;
        walk(&prelude, false)?;
        if name.eq_ignore_ascii_case("layer") {
            self.layer_block_name(&prelude, meta.line)?;
        }
        let body = match body {
            Some(Body::Rules(items)) => items
                .into_iter()
                .map(|item| match item {
                    Rule::Style(style) => self.style(style, rooted).map(Member::Style),
                    Rule::At(inner) => self.group(inner, rooted).map(Member::At),
                })
                .collect::<Result<Vec<_>, _>>()?,
            Some(Body::Members(members)) => self.members(members)?,
            None => return Err(Fail::NotPlain { line: meta.line }),
        };
        Ok(At {
            meta,
            name,
            prelude,
            body: Some(Body::Members(body)),
        })
    }

    /// The selector list of a rule, written as it is to be served.
    fn selectors(&self, prelude: &[Tok], line: u32, rooted: bool) -> Result<Vec<Tok>, Fail> {
        let mut out: Vec<Tok> = Vec::new();
        for (n, part) in split_commas(trimmed(prelude)).into_iter().enumerate() {
            let part = trimmed(part);
            let Some(selector) = self.selector(part, rooted) else {
                return Err(Fail::Selector {
                    text: self.text(part),
                    line: part.first().map_or(line, |token| token.meta.line),
                });
            };
            if n > 0 {
                out.push(Tok::new(Kind::Comma));
                out.push(Tok::new(Kind::Space));
            }
            out.extend(selector);
        }
        Ok(out)
    }

    /// One complex selector as it is to be served, or `None` when it can style outside the
    /// component. Once the `&` of the rule is added where the selector does not write one, the
    /// selector must hold exactly one, outside any function, and no `+` or `~` may follow the
    /// compound that holds it; what comes before it is the selector's own business.
    fn selector(&self, part: &[Tok], rooted: bool) -> Option<Vec<Tok>> {
        let first = part.first()?;
        if !part.iter().all(|token| selector_token(&token.kind))
            || part.iter().any(|token| reaches_outside(&token.kind))
            || opens_a_custom_property(part)
        {
            return None;
        }
        let ampersands = part
            .iter()
            .filter(|token| matches!(token.kind, Kind::Delim('&')))
            .count();
        if ampersands > 1 {
            return None;
        }
        // What is served, and what is checked: the rule's own `&` is not written in front of a
        // selector that has none, but it is there when the selector is checked.
        let (served, probe) = if ampersands == 1 {
            (part.to_vec(), part.to_vec())
        } else if rooted && starts_with_root(part) {
            // `:root` as the whole first compound is the component's root.
            let rewritten: Vec<Tok> = std::iter::once(Tok::new(Kind::Delim('&')))
                .chain(part[2..].iter().cloned())
                .collect();
            (rewritten.clone(), rewritten)
        } else {
            // The rule's own `&`, then a descendant combinator unless the selector starts with
            // one.
            let mut probe = vec![Tok::new(Kind::Delim('&'))];
            if !matches!(first.kind, Kind::Delim('>' | '+' | '~')) {
                probe.push(Tok::new(Kind::Space));
            }
            probe.extend(part.iter().cloned());
            (part.to_vec(), probe)
        };
        follows_the_ampersand_safely(&probe).then_some(served)
    }

    /// The selector as written in the source, on one line.
    fn text(&self, part: &[Tok]) -> String {
        let (Some(first), Some(last)) = (part.first(), part.last()) else {
            return String::new();
        };
        let written = self
            .source
            .get(first.meta.start..last.meta.end)
            .unwrap_or_default();
        written.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// A declaration: its addresses, and the names it defines when it is one of the properties that
    /// name things across the page.
    fn decl(&self, decl: &Decl) -> Result<(), Fail> {
        walk(&decl.value, false)?;
        match decl.name.to_ascii_lowercase().as_str() {
            "anchor-name"
            | "position-anchor"
            | "view-transition-name"
            | "scroll-timeline-name"
            | "view-timeline-name" => self.global_values(decl, false, false),
            "timeline-scope" | "anchor-scope" => self.global_values(decl, true, false),
            // The shorthands that set a name and then more.
            "scroll-timeline" | "view-timeline" => self.global_values(decl, false, true),
            _ => Ok(()),
        }
    }

    /// Each item of the value is `none`, `auto`, `match-element`, a CSS-wide keyword (and `all`
    /// when `all`) or a name under this namespace; with `first_only` it is the first token of the
    /// item that is one of those.
    fn global_values(&self, decl: &Decl, all: bool, first_only: bool) -> Result<(), Fail> {
        let value = trimmed(&decl.value);
        if value.is_empty() {
            return Ok(());
        }
        for item in split_commas(value) {
            let item = trimmed(item);
            let allowed = match item.first().map(|token| &token.kind) {
                Some(Kind::Ident(word)) if first_only || item.len() == 1 => {
                    self.may_name(word, all)
                }
                _ => false,
            };
            if !allowed {
                return Err(Fail::Global {
                    name: written(item),
                    form: format!("--{}--\u{2026}", self.namespace),
                    line: decl.meta.line,
                });
            }
        }
        Ok(())
    }

    fn may_name(&self, word: &str, all: bool) -> bool {
        let lower = word.to_ascii_lowercase();
        let prefix = format!("--{}--", self.namespace);
        NAME_KEYWORDS.contains(&lower.as_str())
            || (all && lower == "all")
            || (word.len() > prefix.len() && word.starts_with(&prefix))
    }

    /// `name` must be this namespace, two dashes and something; a property's name (`property`)
    /// two dashes before that.
    fn global(&self, name: &str, property: bool, line: u32) -> Result<(), Fail> {
        let prefix = if property {
            format!("--{}--", self.namespace)
        } else {
            format!("{}--", self.namespace)
        };
        if name.len() > prefix.len() && name.starts_with(&prefix) {
            return Ok(());
        }
        Err(Fail::Global {
            name: name.to_owned(),
            form: format!("{prefix}\u{2026}"),
            line,
        })
    }

    /// The name of a `@keyframes` rule and its frames.
    fn keyframes(&self, at: &At) -> Result<(), Fail> {
        walk(&at.prelude, false)?;
        match trimmed(&at.prelude) {
            [
                Tok {
                    kind: Kind::Ident(name) | Kind::Str(name),
                    meta,
                },
            ] => self.global(name, false, meta.line)?,
            _ => {
                return Err(Fail::NotPlain { line: at.meta.line });
            }
        }
        let Some(Body::Rules(frames)) = &at.body else {
            return Err(Fail::NotPlain { line: at.meta.line });
        };
        for frame in frames {
            let Rule::Style(frame) = frame else {
                return Err(Fail::NotPlain { line: at.meta.line });
            };
            if !is_keyframe_selector(&frame.prelude) {
                return Err(Fail::NotPlain {
                    line: frame.meta.line,
                });
            }
            self.declarations(&frame.body)?;
        }
        Ok(())
    }

    /// The name of a `@property` rule, and its descriptors.
    fn property(&self, at: &At) -> Result<(), Fail> {
        walk(&at.prelude, false)?;
        match trimmed(&at.prelude) {
            [
                Tok {
                    kind: Kind::Ident(name),
                    meta,
                },
            ] => self.global(name, true, meta.line)?,
            _ => {
                return Err(Fail::NotPlain { line: at.meta.line });
            }
        }
        self.descriptors(at)
    }

    /// The name of a `@counter-style` rule, and its descriptors.
    fn counter_style(&self, at: &At) -> Result<(), Fail> {
        walk(&at.prelude, false)?;
        match trimmed(&at.prelude) {
            [
                Tok {
                    kind: Kind::Ident(name),
                    meta,
                },
            ] => self.global(name, false, meta.line)?,
            _ => {
                return Err(Fail::NotPlain { line: at.meta.line });
            }
        }
        self.descriptors(at)
    }

    /// A `@font-face` rule: no prelude, and each family it names under this namespace.
    fn font_face(&self, at: &At) -> Result<(), Fail> {
        if !trimmed(&at.prelude).is_empty() {
            return Err(Fail::NotPlain { line: at.meta.line });
        }
        self.descriptors(at)?;
        let Some(Body::Members(members)) = &at.body else {
            return Ok(());
        };
        for member in members {
            if let Member::Decl(decl) = member
                && decl.name.eq_ignore_ascii_case("font-family")
            {
                self.family(decl)?;
            }
        }
        Ok(())
    }

    /// A family name is a string, or names written one after the other.
    fn family(&self, decl: &Decl) -> Result<(), Fail> {
        let value = trimmed(&decl.value);
        let named = match value {
            [
                Tok {
                    kind: Kind::Str(name),
                    ..
                },
            ] => Some(name.clone()),
            _ if !value.is_empty()
                && value
                    .iter()
                    .all(|token| matches!(token.kind, Kind::Ident(_) | Kind::Space)) =>
            {
                let words: Vec<&str> = value
                    .iter()
                    .filter_map(|token| match &token.kind {
                        Kind::Ident(word) => Some(word.as_str()),
                        _ => None,
                    })
                    .collect();
                Some(words.join(" "))
            }
            _ => None,
        };
        match named {
            Some(name) => self.global(&name, false, decl.meta.line),
            None => Err(Fail::Global {
                name: written(value),
                form: format!("{}--\u{2026}", self.namespace),
                line: decl.meta.line,
            }),
        }
    }

    /// The block of an at-rule that holds declarations: declarations only, each checked.
    fn descriptors(&self, at: &At) -> Result<(), Fail> {
        match &at.body {
            Some(Body::Members(members)) => self.declarations(members),
            _ => Err(Fail::NotPlain { line: at.meta.line }),
        }
    }

    fn declarations(&self, members: &[Member]) -> Result<(), Fail> {
        for member in members {
            match member {
                Member::Decl(decl) => self.decl(decl)?,
                Member::Style(Style { meta, .. }) | Member::At(At { meta, .. }) => {
                    return Err(Fail::NotPlain { line: meta.line });
                }
            }
        }
        Ok(())
    }

    /// The names of a `@layer` statement: each a name under this namespace, with sub-layers after
    /// dots.
    fn layer_names(&self, at: &At, statement: bool) -> Result<(), Fail> {
        walk(&at.prelude, false)?;
        let tokens = trimmed(&at.prelude);
        let names = split_commas(tokens);
        if tokens.is_empty() || (!statement && names.len() > 1) {
            return Err(Fail::NotPlain { line: at.meta.line });
        }
        for name in names {
            self.layer_name(trimmed(name), at.meta.line)?;
        }
        Ok(())
    }

    /// The name of a `@layer` block: none, or one name.
    fn layer_block_name(&self, prelude: &[Tok], line: u32) -> Result<(), Fail> {
        let tokens = trimmed(prelude);
        if tokens.is_empty() {
            return Ok(());
        }
        if split_commas(tokens).len() > 1 {
            return Err(Fail::NotPlain { line });
        }
        self.layer_name(tokens, line)
    }

    /// A layer's name: the first part under this namespace, then a dot and a part for each
    /// sub-layer.
    fn layer_name(&self, tokens: &[Tok], line: u32) -> Result<(), Fail> {
        let mut parts = tokens.iter();
        let Some(Tok {
            kind: Kind::Ident(first),
            meta,
        }) = parts.next()
        else {
            return Err(Fail::NotPlain { line });
        };
        self.global(first, false, meta.line)?;
        while let Some(dot) = parts.next() {
            let sub = parts.next().map(|token| &token.kind);
            if !matches!(dot.kind, Kind::Delim('.')) || !matches!(sub, Some(Kind::Ident(_))) {
                return Err(Fail::NotPlain { line });
            }
        }
        Ok(())
    }
}

/// `:where([data-slot^="namespace:"])`, the selector every rule of the sheet sits under.
fn wrapper_prelude(namespace: &str) -> Vec<Tok> {
    let attribute = vec![
        Tok::new(Kind::Ident("data-slot".to_owned())),
        Tok::new(Kind::PrefixMatch),
        Tok::new(Kind::Str(format!("{namespace}:"))),
    ];
    vec![
        Tok::new(Kind::Colon),
        Tok::new(Kind::Function(
            "where".to_owned(),
            vec![Tok::new(Kind::Square(attribute))],
        )),
    ]
}

/// What a complex selector may be made of at its own level.
fn selector_token(kind: &Kind) -> bool {
    matches!(
        kind,
        Kind::Space
            | Kind::Ident(_)
            | Kind::Hash { id: true, .. }
            | Kind::Colon
            | Kind::Function(..)
            | Kind::Square(_)
            | Kind::Delim('.' | '*' | '&' | '>' | '+' | '~')
    )
}

/// What makes up a compound selector.
fn compound_member(kind: &Kind) -> bool {
    matches!(
        kind,
        Kind::Ident(_)
            | Kind::Hash { id: true, .. }
            | Kind::Colon
            | Kind::Function(..)
            | Kind::Square(_)
            | Kind::Delim('.' | '*' | '&')
    )
}

/// Whether a block of a selector (the arguments of a function, an attribute's brackets) holds a
/// `&` or a `{}` block at any depth.
fn reaches_outside(kind: &Kind) -> bool {
    match kind {
        Kind::Function(_, inner) | Kind::Paren(inner) | Kind::Square(inner) => {
            inner.iter().any(|token| {
                matches!(token.kind, Kind::Delim('&') | Kind::Curly(_))
                    || reaches_outside(&token.kind)
            })
        }
        _ => false,
    }
}

/// Whether the whole first compound of the selector is `:root`.
fn starts_with_root(part: &[Tok]) -> bool {
    matches!(
        (part.first().map(|t| &t.kind), part.get(1).map(|t| &t.kind)),
        (Some(Kind::Colon), Some(Kind::Ident(word))) if word.eq_ignore_ascii_case("root")
    ) && !part
        .get(2)
        .is_some_and(|token| compound_member(&token.kind))
}

/// Whether what follows the compound that holds the selector's `&` is nothing, a descendant
/// combinator or a child combinator: not a `+`, a `~` or anything else.
fn follows_the_ampersand_safely(probe: &[Tok]) -> bool {
    let Some(ampersand) = probe
        .iter()
        .position(|token| matches!(token.kind, Kind::Delim('&')))
    else {
        return false;
    };
    let end = probe[ampersand + 1..]
        .iter()
        .position(|token| !compound_member(&token.kind))
        .map_or(probe.len(), |found| ampersand + 1 + found);
    match probe.get(end).map(|token| &token.kind) {
        None | Some(Kind::Delim('>')) => true,
        Some(Kind::Space) => !matches!(
            probe[end..]
                .iter()
                .find(|token| !is_space(token))
                .map(|token| &token.kind),
            Some(Kind::Delim('+' | '~'))
        ),
        Some(_) => false,
    }
}

/// `from`, `to` or a percentage, for each frame of a list.
fn is_keyframe_selector(prelude: &[Tok]) -> bool {
    let tokens = trimmed(prelude);
    !tokens.is_empty()
        && split_commas(tokens)
            .into_iter()
            .all(|part| match trimmed(part) {
                [
                    Tok {
                        kind: Kind::Ident(word),
                        ..
                    },
                ] => word.eq_ignore_ascii_case("from") || word.eq_ignore_ascii_case("to"),
                [
                    Tok {
                        kind: Kind::Pct(_), ..
                    },
                ] => true,
                _ => false,
            })
}

// ---------------------------------------------------------------------------------------- values

/// Whether `address` is a `data:` URL.
fn is_data(address: &str) -> bool {
    address
        .as_bytes()
        .get(..5)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case(b"data:"))
}

/// Whether the tokens of an `attr()` ask for an address: a type of `url`, or a `type()`.
fn reads_an_address(tokens: &[Tok]) -> bool {
    tokens.iter().any(|token| match &token.kind {
        Kind::Ident(word) => word.eq_ignore_ascii_case("url"),
        Kind::Function(name, inner) => name.eq_ignore_ascii_case("type") || reads_an_address(inner),
        Kind::Paren(inner) | Kind::Square(inner) | Kind::Curly(inner) => reads_an_address(inner),
        _ => false,
    })
}

/// Every address in the tokens must be a `data:` URL: `url()` and `src()` with one, every string
/// inside `image-set()`, `-webkit-image-set()` and `image()`. A value that is only known when the
/// page is drawn (`var()`) cannot stand where an address does, and `attr()` cannot ask for one.
/// The tokens that only a sheet's top level may hold between its rules are refused anywhere else.
fn walk(tokens: &[Tok], in_address_function: bool) -> Result<(), Fail> {
    for token in tokens {
        let line = token.meta.line;
        match &token.kind {
            Kind::Url(address) if !is_data(address) => return Err(Fail::Url { line }),
            Kind::Str(address) if in_address_function && !is_data(address) => {
                return Err(Fail::Url { line });
            }
            Kind::Function(name, inner) => match name.to_ascii_lowercase().as_str() {
                "url" | "src" => {
                    let address = inner.iter().find(|inside| !is_space(inside));
                    let data = matches!(
                        address.map(|inside| &inside.kind),
                        Some(Kind::Str(text)) if is_data(text)
                    );
                    if !data {
                        return Err(Fail::Url { line });
                    }
                    walk(inner, false)?;
                }
                "image-set" | "-webkit-image-set" | "image" => walk(inner, true)?,
                "var" if in_address_function => return Err(Fail::Url { line }),
                "attr" if reads_an_address(inner) => return Err(Fail::Url { line }),
                _ => walk(inner, in_address_function)?,
            },
            Kind::Paren(inner) | Kind::Square(inner) | Kind::Curly(inner) => {
                walk(inner, in_address_function)?;
            }
            // Only the top of a sheet, between rules, may hold these.
            Kind::Cdo | Kind::Cdc => return Err(Fail::NotPlain { line }),
            _ => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------- writing

/// The rules as CSS text.
pub(crate) fn write_rules(rules: &[Rule]) -> String {
    let mut out = String::new();
    rules_into(&mut out, rules);
    out
}

/// The tokens as CSS text, for a message.
fn written(tokens: &[Tok]) -> String {
    let mut out = String::new();
    tokens_into(&mut out, trimmed(tokens), TokenSerializationType::Nothing);
    out
}

fn rules_into(out: &mut String, rules: &[Rule]) {
    for rule in rules {
        match rule {
            Rule::Style(style) => style_into(out, style),
            Rule::At(at) => at_into(out, at),
        }
    }
}

fn style_into(out: &mut String, style: &Style) {
    tokens_into(out, &style.prelude, TokenSerializationType::Nothing);
    out.push('{');
    members_into(out, &style.body);
    out.push('}');
}

/// A semicolon follows each declaration that something follows; a block needs none.
fn members_into(out: &mut String, members: &[Member]) {
    for (n, member) in members.iter().enumerate() {
        match member {
            Member::Decl(decl) => {
                decl_into(out, decl);
                if n + 1 < members.len() {
                    out.push(';');
                }
            }
            Member::Style(style) => style_into(out, style),
            Member::At(at) => at_into(out, at),
        }
    }
}

fn decl_into(out: &mut String, decl: &Decl) {
    leaf(out, &Token::Ident(CowRcStr::from(decl.name.as_str())));
    out.push(':');
    tokens_into(out, &decl.value, TokenSerializationType::Other);
    if decl.important {
        out.push_str("!important");
    }
}

fn at_into(out: &mut String, at: &At) {
    leaf(out, &Token::AtKeyword(CowRcStr::from(at.name.as_str())));
    tokens_into(out, &at.prelude, TokenSerializationType::AtKeywordOrHash);
    match &at.body {
        None => out.push(';'),
        Some(Body::Rules(rules)) => {
            out.push('{');
            rules_into(out, rules);
            out.push('}');
        }
        Some(Body::Members(members)) => {
            out.push('{');
            members_into(out, members);
            out.push('}');
        }
    }
}

/// Writes `token` as cssparser writes it.
fn leaf(out: &mut String, token: &Token<'_>) {
    // Writing to a string cannot fail.
    let _ = token.to_css(out);
}

/// The unit of a dimension as cssparser writes it, escaped where it would be read as a number.
fn unit_text(unit: &str) -> String {
    let written = Token::Dimension {
        has_sign: false,
        value: 0.0,
        int_value: Some(0),
        unit: CowRcStr::from(unit),
    }
    .to_css_string();
    written
        .strip_prefix('0')
        .unwrap_or(written.as_str())
        .to_owned()
}

/// The cssparser token a token that holds no block is written as.
fn leaf_token(kind: &Kind) -> Option<Token<'_>> {
    Some(match kind {
        Kind::Ident(name) => Token::Ident(CowRcStr::from(name.as_str())),
        Kind::AtKeyword(name) => Token::AtKeyword(CowRcStr::from(name.as_str())),
        Kind::Hash { name, id: true } => Token::IDHash(CowRcStr::from(name.as_str())),
        Kind::Hash { name, id: false } => Token::Hash(CowRcStr::from(name.as_str())),
        Kind::Str(text) => Token::QuotedString(CowRcStr::from(text.as_str())),
        Kind::Url(address) => Token::UnquotedUrl(CowRcStr::from(address.as_str())),
        Kind::Delim(c) => Token::Delim(*c),
        Kind::Colon => Token::Colon,
        Kind::Semi => Token::Semicolon,
        Kind::Comma => Token::Comma,
        Kind::IncludeMatch => Token::IncludeMatch,
        Kind::DashMatch => Token::DashMatch,
        Kind::PrefixMatch => Token::PrefixMatch,
        Kind::SuffixMatch => Token::SuffixMatch,
        Kind::SubstringMatch => Token::SubstringMatch,
        Kind::Cdo => Token::CDO,
        Kind::Cdc => Token::CDC,
        _ => return None,
    })
}

/// What a token is, for deciding whether two tokens written one after the other would be read as
/// one.
fn class(kind: &Kind) -> TokenSerializationType {
    use TokenSerializationType as T;
    match kind {
        Kind::Space => T::WhiteSpace,
        Kind::Ident(_) => T::Ident,
        Kind::AtKeyword(_) | Kind::Hash { .. } => T::AtKeywordOrHash,
        Kind::Url(_) => T::UrlOrBadUrl,
        Kind::Delim(c) => Token::Delim(*c).serialization_type(),
        Kind::Num(_) => T::Number,
        Kind::Pct(_) => T::Percentage,
        Kind::Dim(..) => T::Dimension,
        Kind::DashMatch => T::DashMatch,
        Kind::SubstringMatch => T::SubstringMatch,
        Kind::Cdc => T::CDC,
        Kind::Function(..) => T::Function,
        Kind::Paren(_) => T::OpenParen,
        Kind::Str(_)
        | Kind::Colon
        | Kind::Semi
        | Kind::Comma
        | Kind::IncludeMatch
        | Kind::PrefixMatch
        | Kind::SuffixMatch
        | Kind::Cdo
        | Kind::Square(_)
        | Kind::Curly(_) => T::Other,
    }
}

/// Writes the tokens after one of the class `previous`. Two tokens that had a comment and no space
/// between them in the source are kept apart with an empty comment when they would be read as one.
fn tokens_into(
    out: &mut String,
    tokens: &[Tok],
    mut previous: TokenSerializationType,
) -> TokenSerializationType {
    for token in tokens {
        let this = class(&token.kind);
        if token.meta.glued && previous.needs_separator_when_before(this) {
            out.push_str("/**/");
        }
        previous = this;
        match &token.kind {
            Kind::Space => out.push(' '),
            Kind::Num(digits) => out.push_str(digits),
            Kind::Pct(digits) => {
                out.push_str(digits);
                out.push('%');
            }
            Kind::Dim(digits, unit) => {
                out.push_str(digits);
                out.push_str(&unit_text(unit));
            }
            Kind::Function(name, inner) => {
                leaf(out, &Token::Function(CowRcStr::from(name.as_str())));
                tokens_into(out, inner, TokenSerializationType::Function);
                out.push(')');
                previous = TokenSerializationType::Other;
            }
            Kind::Paren(inner) => {
                out.push('(');
                tokens_into(out, inner, TokenSerializationType::OpenParen);
                out.push(')');
                previous = TokenSerializationType::Other;
            }
            Kind::Square(inner) => {
                out.push('[');
                tokens_into(out, inner, TokenSerializationType::Other);
                out.push(']');
            }
            Kind::Curly(inner) => {
                out.push('{');
                tokens_into(out, inner, TokenSerializationType::Other);
                out.push('}');
            }
            other => {
                if let Some(simple) = leaf_token(other) {
                    leaf(out, &simple);
                }
            }
        }
    }
    previous
}
