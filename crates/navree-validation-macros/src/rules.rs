//! The `#[validate(...)]` grammar: container options and field rules,
//! parsed and checked into a model the expansion reads.

use proc_macro2::Span;
use syn::ext::IdentExt;
#[cfg(test)]
use syn::parse::Parse;
use syn::parse::ParseStream;
use syn::spanned::Spanned;
use syn::{Expr, Ident, LitStr, Token, parenthesized};

/// Options on the struct or enum.
#[derive(Default)]
pub(crate) struct Container {
    /// `crate = "path"`: the runtime crate's path, when it can't be found.
    pub(crate) krate: Option<syn::Path>,
    /// `skip_unmarked`: fields without `#[validate]` have no rules.
    pub(crate) skip_unmarked: bool,
}

/// The rules of one field (or of each item, inside `each`).
#[derive(Default)]
pub(crate) struct FieldRules {
    /// `skip`: the field has no rules.
    pub(crate) skip: bool,
    /// `rename = "name"`: the field's name in paths.
    pub(crate) rename: Option<LitStr>,
    pub(crate) rules: Vec<Rule>,
}

/// One rule, with the span its errors point at.
pub(crate) struct Rule {
    pub(crate) span: Span,
    pub(crate) kind: Kind,
}

/// The rules.
pub(crate) enum Kind {
    Required,
    Length {
        bytes: bool,
        min: Option<Expr>,
        max: Option<Expr>,
    },
    Range {
        min: Option<Expr>,
        max: Option<Expr>,
    },
    /// A rule on text with no arguments (`email`, `url`, `ip`, ...): the
    /// name of its check function.
    Text(&'static str),
    /// A rule on text with a string argument (`contains`, `starts_with`,
    /// `ends_with`).
    TextWith(&'static str, LitStr),
    /// The regular expression, and whether the page may state it so the browser checks
    /// it too (`pattern("...", browser)`).
    Pattern(LitStr, bool),
    SameAs(Ident),
    Custom(Expr),
    Nested,
    Each(Vec<Rule>),
}

impl Kind {
    /// The rule's name, for messages about duplicates.
    fn name(&self) -> &'static str {
        match self {
            Kind::Required => "required",
            Kind::Length { .. } => "length",
            Kind::Range { .. } => "range",
            Kind::Text(name) | Kind::TextWith(name, _) => name,
            Kind::Pattern(..) => "pattern",
            Kind::SameAs(_) => "same_as",
            Kind::Custom(_) => "custom",
            Kind::Nested => "nested",
            Kind::Each(_) => "each",
        }
    }
}

/// Rules with no arguments that check text, and their check functions.
const TEXT_RULES: &[&str] = &[
    "email",
    "url",
    "ip",
    "ipv4",
    "ipv6",
    "ascii",
    "alphanumeric",
    "credit_card",
];

/// Rules that check text against a string.
const TEXT_WITH_RULES: &[&str] = &["contains", "starts_with", "ends_with"];

/// Every field rule, for the unknown-rule message.
const FIELD_RULES: &str = "skip, rename, required, length, range, email, url, ip, ipv4, ipv6, \
     ascii, alphanumeric, credit_card, contains, starts_with, ends_with, pattern, same_as, \
     custom, nested, each";

/// Where a list of rules appears.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Field,
    Each,
}

impl Container {
    /// Reads one `#[validate(...)]` on the struct or enum.
    pub(crate) fn parse_into(&mut self, attr: &syn::Attribute) -> syn::Result<()> {
        attr.parse_args_with(|input: ParseStream| {
            while !input.is_empty() {
                let key = input.call(Ident::parse_any)?;
                match key.to_string().as_str() {
                    "crate" => {
                        input.parse::<Token![=]>()?;
                        let path: LitStr = input.parse()?;
                        if self.krate.is_some() {
                            return Err(syn::Error::new(key.span(), "duplicate `crate`"));
                        }
                        self.krate = Some(path.parse()?);
                    }
                    "skip_unmarked" => self.skip_unmarked = true,
                    _ => {
                        return Err(syn::Error::new(
                            key.span(),
                            format!(
                                "unknown option `{key}` on a type: expected `crate = \"...\"` or `skip_unmarked` (rules go on fields)"
                            ),
                        ));
                    }
                }
                if !input.is_empty() {
                    input.parse::<Token![,]>()?;
                }
            }
            Ok(())
        })
    }
}

impl FieldRules {
    /// Reads one `#[validate(...)]` on a field (a field may have several).
    pub(crate) fn parse_into(&mut self, attr: &syn::Attribute) -> syn::Result<()> {
        if matches!(attr.meta, syn::Meta::Path(_)) {
            return Err(syn::Error::new(
                attr.span(),
                "`#[validate]` needs its rules: `#[validate(length(max = 255))]`, or `#[validate(skip)]`",
            ));
        }
        attr.parse_args_with(|input: ParseStream| {
            let parsed = parse_rules(input, Level::Field)?;
            if let Some(span) = parsed.skip {
                if self.skip {
                    return Err(syn::Error::new(span, "duplicate `skip`"));
                }
                self.skip = true;
            }
            if let Some(rename) = parsed.rename {
                if self.rename.is_some() {
                    return Err(syn::Error::new(rename.span(), "duplicate `rename`"));
                }
                self.rename = Some(rename);
            }
            self.rules.extend(parsed.rules);
            Ok(())
        })
    }

    /// Checks the field's rules as a whole: `skip` alone, no rule twice
    /// (except `custom`), `each` not empty.
    pub(crate) fn check(&self) -> syn::Result<()> {
        if self.skip
            && let Some(rule) = self.rules.first()
        {
            return Err(syn::Error::new(
                rule.span,
                "a field with `skip` has no other rules: remove `skip` or this rule",
            ));
        }
        check_list(&self.rules)
    }
}

/// No rule twice in one list (except `custom`), recursively.
fn check_list(rules: &[Rule]) -> syn::Result<()> {
    for (i, rule) in rules.iter().enumerate() {
        if !matches!(rule.kind, Kind::Custom(_))
            && rules[..i]
                .iter()
                .any(|earlier| earlier.kind.name() == rule.kind.name())
        {
            return Err(syn::Error::new(
                rule.span,
                format!(
                    "duplicate `{}`: give each rule once (only `custom` may repeat)",
                    rule.kind.name()
                ),
            ));
        }
        if let Kind::Each(inner) = &rule.kind {
            check_list(inner)?;
        }
    }
    Ok(())
}

/// A parsed list of rules.
struct Parsed {
    skip: Option<Span>,
    rename: Option<LitStr>,
    rules: Vec<Rule>,
}

fn parse_rules(input: ParseStream, level: Level) -> syn::Result<Parsed> {
    let mut parsed = Parsed {
        skip: None,
        rename: None,
        rules: Vec::new(),
    };
    while !input.is_empty() {
        let name = input.call(Ident::parse_any)?;
        let span = name.span();
        let text = name.to_string();
        let kind = match text.as_str() {
            "skip" | "rename" if level == Level::Each => {
                return Err(syn::Error::new(
                    span,
                    format!("`{text}` applies to the field, not to its items"),
                ));
            }
            "skip" => {
                no_arguments(input, &text)?;
                parsed.skip = Some(span);
                None
            }
            "rename" => {
                input.parse::<Token![=]>()?;
                parsed.rename = Some(input.parse()?);
                None
            }
            "required" => {
                no_arguments(input, &text)?;
                Some(Kind::Required)
            }
            "nested" => {
                no_arguments(input, &text)?;
                Some(Kind::Nested)
            }
            "length" => Some(parse_length(input, span)?),
            "range" => {
                let (min, max, _) = parse_bounds(input, span, "range", false)?;
                Some(Kind::Range { min, max })
            }
            "pattern" => {
                let content = arguments(input, span, "pattern(\"^[a-z]+$\")")?;
                let pattern: LitStr = content.parse()?;
                let browser = if content.peek(Token![,]) {
                    content.parse::<Token![,]>()?;
                    let flag = content.call(Ident::parse_any)?;
                    if flag != "browser" {
                        return Err(syn::Error::new(
                            flag.span(),
                            "the only option is `browser`: `pattern(\"^[a-z]+$\", browser)`",
                        ));
                    }
                    true
                } else {
                    false
                };
                no_more(&content)?;
                if let Err(error) = regex::Regex::new(&pattern.value()) {
                    return Err(syn::Error::new(
                        pattern.span(),
                        format!("invalid regular expression: {error}"),
                    ));
                }
                if browser && let Some(problem) = browser_problem(&pattern.value()) {
                    return Err(syn::Error::new(pattern.span(), problem));
                }
                Some(Kind::Pattern(pattern, browser))
            }
            "same_as" => {
                if level == Level::Each {
                    return Err(syn::Error::new(
                        span,
                        "`same_as` compares two fields, so it can't apply to items",
                    ));
                }
                let content = arguments(input, span, "same_as(other_field)")?;
                let other: Ident = content.parse()?;
                no_more(&content)?;
                Some(Kind::SameAs(other))
            }
            "custom" => {
                let content = arguments(input, span, "custom(path::to::rule)")?;
                let expr: Expr = content.parse()?;
                no_more(&content)?;
                Some(Kind::Custom(expr))
            }
            "each" => {
                let content = arguments(input, span, "each(length(max = 30))")?;
                let inner = parse_rules(&content, Level::Each)?;
                if inner.rules.is_empty() {
                    return Err(syn::Error::new(
                        span,
                        "`each` needs the rules its items follow: `each(length(max = 30))`",
                    ));
                }
                Some(Kind::Each(inner.rules))
            }
            other => {
                if let Some(rule) = TEXT_RULES.iter().find(|rule| **rule == other) {
                    no_arguments(input, other)?;
                    Some(Kind::Text(rule))
                } else if let Some(rule) = TEXT_WITH_RULES.iter().find(|rule| **rule == other) {
                    let content = arguments(input, span, &format!("{other}(\"text\")"))?;
                    let value: LitStr = content.parse()?;
                    no_more(&content)?;
                    Some(Kind::TextWith(rule, value))
                } else {
                    return Err(unknown_rule(&name));
                }
            }
        };
        if let Some(kind) = kind {
            parsed.rules.push(Rule { span, kind });
        }
        if !input.is_empty() {
            input.parse::<Token![,]>()?;
        }
    }
    Ok(parsed)
}

/// An unknown rule, with the rule to use when the name is a familiar one.
fn unknown_rule(name: &Ident) -> syn::Error {
    let hint = match name.to_string().as_str() {
        "dive" => Some("`nested`, to validate a field that derives `Validate`"),
        "inner" => Some("`each(...)`, to give the items of a collection rules"),
        "prefix" => Some("`starts_with(\"...\")`"),
        "suffix" => Some("`ends_with(\"...\")`"),
        "matches" => Some("`same_as(other_field)`"),
        "regex" => Some("`pattern(\"...\")`"),
        "allow_unvalidated" => Some("`skip_unmarked` on the struct"),
        "context" => Some("a `custom` rule that captures what it needs"),
        "min" | "max" => Some("`length(min = ...)` or `range(min = ...)`"),
        _ => None,
    };
    let message = match hint {
        Some(hint) => format!("unknown rule `{name}`: use {hint}"),
        None => format!("unknown rule `{name}`: expected one of {FIELD_RULES}"),
    };
    syn::Error::new(name.span(), message)
}

/// The parenthesized arguments of a rule that needs them.
fn arguments<'a>(
    input: ParseStream<'a>,
    span: Span,
    example: &str,
) -> syn::Result<syn::parse::ParseBuffer<'a>> {
    if !input.peek(syn::token::Paren) {
        return Err(syn::Error::new(
            span,
            format!("this rule needs arguments: `{example}`"),
        ));
    }
    let content;
    parenthesized!(content in input);
    Ok(content)
}

/// A rule that takes no arguments has none.
fn no_arguments(input: ParseStream, rule: &str) -> syn::Result<()> {
    if input.peek(syn::token::Paren) || input.peek(Token![=]) {
        return Err(input.error(format!("`{rule}` takes no arguments")));
    }
    Ok(())
}

/// Why a Rust pattern can't be stated to a browser as it is: the browser would accept or
/// refuse other values than the server. `None` when it reads it the same way.
///
/// A browser checks `pattern` against the whole value, in JavaScript's `v` mode, where
/// `\w`, `\d`, `\s`, `\b` and `.` mean less than in Rust's Unicode-aware syntax, a class
/// is stricter about `-` and some punctuation, and an invalid pattern is silently
/// ignored. So only a pattern written plainly in the syntax both read the same way gets
/// through: anchored with `^` and `$`, choices inside a group, characters written out,
/// and every part valid in `v` mode, so that the browser checks it at all.
fn browser_problem(pattern: &str) -> Option<String> {
    /// What came last, for what may follow: only something to repeat takes a count, and
    /// a count takes one `?` that makes it lazy.
    #[derive(Clone, Copy, PartialEq)]
    enum Last {
        Nothing,
        Atom,
        Count,
        Lazy,
    }
    // What may follow `\` outside a class, and inside one, in both dialects.
    const SYNTAX: &str = r"^$\.*+?()[]{}|/";
    const CLASS_ESCAPES: &str = r"^$\.*+?()[]{}|/-&!#%,:;<=>@`~";
    // In a class, these doubled are operators in the browser's dialect.
    const DOUBLED: &str = "&!#$%*+,.:;<=>?@^`~";
    let leave = |why: &str| Some(format!("{why}, or leave out `browser`"));
    let chars: Vec<char> = pattern.chars().collect();
    let n = chars.len();
    if n < 2 || chars[0] != '^' || chars[n - 1] != '$' || chars[n - 2] == '\\' {
        return leave(
            "a browser checks a pattern against the whole value: write it the same way for \
             the server, from `^` to `$`",
        );
    }
    let repeat = || {
        leave(
            "a `*`, `+`, `?` or `{n}` repeats the one thing just before it, once: a browser \
             refuses the whole pattern for `a+*`, `a{2}*`, `^*` or `(*`",
        )
    };
    let mut depth = 0usize;
    let mut last = Last::Nothing;
    let mut i = 0;
    while i < n {
        match chars[i] {
            '\\' => {
                let next = chars.get(i + 1).copied().unwrap_or(' ');
                if !SYNTAX.contains(next) {
                    return leave(&format!(
                        "`\\{next}` means something else in a browser's patterns, or nothing \
                         (on the server `\\w`, `\\d` and `\\s` take any script's letters, \
                         digits and spaces): write the characters out, like `[0-9]`"
                    ));
                }
                i += 2;
                last = Last::Atom;
                continue;
            }
            '[' => {
                i += 1;
                if chars.get(i) == Some(&'^') {
                    i += 1;
                }
                if chars.get(i) == Some(&']') {
                    return leave("a `]` first in a class needs a `\\` in a browser's patterns");
                }
                // The last item, when it is one character that can start a range.
                let mut single: Option<char> = None;
                loop {
                    let Some(&c) = chars.get(i) else {
                        return leave("a class needs its `]`");
                    };
                    match c {
                        ']' => {
                            i += 1;
                            break;
                        }
                        '\\' => {
                            let next = chars.get(i + 1).copied().unwrap_or(' ');
                            if !CLASS_ESCAPES.contains(next) {
                                return leave(&format!(
                                    "`\\{next}` means something else in a browser's patterns, \
                                     or nothing: write the characters out, like `[0-9]`"
                                ));
                            }
                            single = Some(next);
                            i += 2;
                        }
                        '-' => {
                            let next = chars.get(i + 1).copied();
                            let Some(start) =
                                single.filter(|_| !matches!(next, None | Some(']' | '-')))
                            else {
                                return leave(
                                    "in a browser's patterns a `-` in a class only joins a \
                                     range, like `a-z`: write `\\-` for a dash",
                                );
                            };
                            let end = match next {
                                Some('\\') => {
                                    let escaped = chars.get(i + 2).copied().unwrap_or(' ');
                                    if !CLASS_ESCAPES.contains(escaped) {
                                        return leave("a range ends at one character");
                                    }
                                    i += 3;
                                    escaped
                                }
                                Some(end) if "()[{}/|".contains(end) => {
                                    return leave(
                                        "`(`, `)`, `[`, `{`, `}`, `/` and `|` in a class need a \
                                         `\\` in a browser's patterns",
                                    );
                                }
                                Some(end) => {
                                    i += 2;
                                    end
                                }
                                None => ' ',
                            };
                            if end < start {
                                return leave(&format!(
                                    "a range goes up, like `a-z`: a browser refuses `{start}-{end}`"
                                ));
                            }
                            single = None;
                        }
                        '(' | ')' | '[' | '{' | '}' | '/' | '|' => {
                            return leave(
                                "`(`, `)`, `[`, `{`, `}`, `/` and `|` in a class need a `\\` in \
                                 a browser's patterns",
                            );
                        }
                        c => {
                            if DOUBLED.contains(c) && chars.get(i + 1) == Some(&c) {
                                return leave(&format!(
                                    "`{c}{c}` in a class means something else in a browser's \
                                     patterns: write one `\\{c}` and the other"
                                ));
                            }
                            single = Some(c);
                            i += 1;
                        }
                    }
                }
                last = Last::Atom;
                continue;
            }
            '(' => {
                if chars.get(i + 1) == Some(&'?') {
                    if chars.get(i + 2) != Some(&':') {
                        return leave(
                            "a browser reads `(?` groups other than `(?:` differently: write \
                             the pattern without inline flags or named groups",
                        );
                    }
                    i += 3;
                } else {
                    i += 1;
                }
                depth += 1;
                last = Last::Nothing;
                continue;
            }
            ')' => {
                if depth == 0 {
                    return leave("a `)` needs its `(`");
                }
                depth -= 1;
                last = Last::Atom;
            }
            '|' if depth == 0 => {
                return leave(
                    "a browser checks `^a|b$` as one whole choice, the server as two half-anchored \
                     ones: put the choice in a group, `^(?:a|b)$`",
                );
            }
            '|' => last = Last::Nothing,
            '.' => {
                return leave(
                    "`.` matches less in a browser than on the server: write the characters it \
                     may match, like `[a-z0-9 ]`",
                );
            }
            ']' | '}' => {
                return leave("a `]` or `}` on its own needs a `\\` in a browser's patterns");
            }
            '{' => {
                // Only a count after something to count: `{n}`, `{n,}`, `{n,m}`.
                let close = chars[i..].iter().position(|c| *c == '}').map(|at| i + at);
                let inner: String =
                    close.map_or_else(String::new, |close| chars[i + 1..close].iter().collect());
                let mut parts = inner.splitn(2, ',');
                let first = parts.next().unwrap_or_default();
                let second = parts.next();
                let digits =
                    |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
                if close.is_none()
                    || !digits(first)
                    || second.is_some_and(|part| !part.is_empty() && !digits(part))
                {
                    return leave("a `{` that isn't a count, like `{3}` or `{2,5}`, needs a `\\`");
                }
                if last != Last::Atom {
                    return repeat();
                }
                let number = |part: &str| part.parse::<u64>().unwrap_or(u64::MAX);
                if second.is_some_and(|part| !part.is_empty() && number(part) < number(first)) {
                    return leave("a count goes up, like `{2,5}`: a browser refuses `{5,2}`");
                }
                i = close.unwrap_or(n) + 1;
                last = Last::Count;
                continue;
            }
            '*' | '+' | '?' => {
                last = match (last, chars[i]) {
                    (Last::Atom, _) => Last::Count,
                    (Last::Count, '?') => Last::Lazy,
                    _ => return repeat(),
                }
            }
            // An anchor is nothing to repeat.
            '^' | '$' => last = Last::Nothing,
            _ => last = Last::Atom,
        }
        i += 1;
    }
    if depth != 0 {
        return leave("a `(` needs its `)`");
    }
    None
}

/// Nothing follows the argument.
fn no_more(content: ParseStream) -> syn::Result<()> {
    if content.is_empty() {
        Ok(())
    } else {
        Err(content.error("unexpected argument: this rule takes one"))
    }
}

fn parse_length(input: ParseStream, span: Span) -> syn::Result<Kind> {
    let (min, max, bytes) = parse_bounds(input, span, "length", true)?;
    Ok(Kind::Length { bytes, min, max })
}

/// `min = expr`, `max = expr` (and, for `length`, `bytes`): at least one
/// bound, and `min` not above `max` when both are integer literals.
fn parse_bounds(
    input: ParseStream,
    span: Span,
    rule: &str,
    length: bool,
) -> syn::Result<(Option<Expr>, Option<Expr>, bool)> {
    let example = format!("{rule}(min = 1, max = 100)");
    let content = arguments(input, span, &example)?;
    let mut min: Option<Expr> = None;
    let mut max: Option<Expr> = None;
    let mut bytes = false;
    while !content.is_empty() {
        let key = content.call(Ident::parse_any)?;
        match key.to_string().as_str() {
            "min" | "max" => {
                content.parse::<Token![=]>()?;
                let value: Expr = content.parse()?;
                let slot = if key == "min" { &mut min } else { &mut max };
                if slot.is_some() {
                    return Err(syn::Error::new(key.span(), format!("duplicate `{key}`")));
                }
                *slot = Some(value);
            }
            "bytes" if length => bytes = true,
            "chars" if length => {
                return Err(syn::Error::new(
                    key.span(),
                    "`length` counts characters already: remove `chars` (write `bytes` to count bytes)",
                ));
            }
            _ => {
                let expected = if length {
                    "`min = ...`, `max = ...` or `bytes`"
                } else {
                    "`min = ...` or `max = ...`"
                };
                return Err(syn::Error::new(
                    key.span(),
                    format!("unknown `{rule}` argument `{key}`: expected {expected}"),
                ));
            }
        }
        if !content.is_empty() {
            content.parse::<Token![,]>()?;
        }
    }
    if min.is_none() && max.is_none() {
        return Err(syn::Error::new(
            span,
            format!("`{rule}` needs a bound: `{example}`"),
        ));
    }
    if let (Some(low), Some(high)) = (literal(min.as_ref()), literal(max.as_ref()))
        && low > high
    {
        return Err(syn::Error::new(
            max.as_ref().map_or(span, Spanned::span),
            format!("`{rule}` can never pass: `min` ({low}) is above `max` ({high})"),
        ));
    }
    Ok((min, max, bytes))
}

/// The value of an integer literal bound (`10`, `-1`, `10_000`).
fn literal(expr: Option<&Expr>) -> Option<i128> {
    match expr? {
        Expr::Lit(lit) => match &lit.lit {
            syn::Lit::Int(int) => int.base10_parse().ok(),
            _ => None,
        },
        Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Neg(_)) => {
            literal(Some(&unary.expr)).map(|value| -value)
        }
        _ => None,
    }
}

/// So `syn::parse2::<FieldRules>` works in the unit tests.
#[cfg(test)]
impl Parse for FieldRules {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let parsed = parse_rules(input, Level::Field)?;
        Ok(FieldRules {
            skip: parsed.skip.is_some(),
            rename: parsed.rename,
            rules: parsed.rules,
        })
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::*;

    fn parse(tokens: proc_macro2::TokenStream) -> syn::Result<FieldRules> {
        let rules: FieldRules = syn::parse2(tokens)?;
        rules.check()?;
        Ok(rules)
    }

    fn error(tokens: proc_macro2::TokenStream) -> String {
        match parse(tokens) {
            Ok(_) => panic!("expected an error"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn rules_parse() {
        let rules = parse(quote!(
            required,
            length(min = 1, max = MAX),
            each(length(bytes, max = 30), custom(f)),
            custom(g),
            custom(h)
        ))
        .unwrap();
        assert_eq!(rules.rules.len(), 5);
        assert!(matches!(rules.rules[2].kind, Kind::Each(ref inner) if inner.len() == 2));
        assert!(parse(quote!(skip)).unwrap().skip);
        assert_eq!(
            parse(quote!(rename = "firstName", email))
                .unwrap()
                .rename
                .unwrap()
                .value(),
            "firstName"
        );
    }

    #[test]
    fn mistakes_are_named() {
        assert!(error(quote!(skip, email)).contains("a field with `skip` has no other rules"));
        assert!(error(quote!(email, email)).contains("duplicate `email`"));
        assert!(error(quote!(length)).contains("this rule needs arguments"));
        assert!(error(quote!(length())).contains("`length` needs a bound"));
        assert!(error(quote!(length(min = 5, max = 1))).contains("can never pass"));
        assert!(error(quote!(range(min = -1, max = -5))).contains("can never pass"));
        assert!(error(quote!(length(chars, max = 5))).contains("counts characters already"));
        assert!(error(quote!(range(bytes, max = 5))).contains("unknown `range` argument"));
        assert!(error(quote!(email(x))).contains("takes no arguments"));
        assert!(error(quote!(dive)).contains("use `nested`"));
        assert!(error(quote!(inner(email))).contains("use `each(...)`"));
        assert!(error(quote!(frobnicate)).contains("expected one of"));
        assert!(error(quote!(pattern("("))).contains("invalid regular expression"));
        assert!(error(quote!(each())).contains("`each` needs the rules"));
        assert!(error(quote!(each(skip))).contains("applies to the field"));
        assert!(error(quote!(each(same_as(x)))).contains("can't apply to items"));
        assert!(error(quote!(each(email, email))).contains("duplicate `email`"));
    }
}

#[cfg(test)]
mod browser_patterns {
    use super::browser_problem;

    #[test]
    fn patterns_a_browser_reads_as_the_server_does_pass() {
        for pattern in [
            r"^[a-z_]+$",
            r"^[A-Z]{3}$",
            r"^[a-z0-9]+(?:-[a-z0-9]+)*$",
            r"^[^@ ]+@example\.com$",
            r"^(?:[0-9]{3}|[A-Z]{2}-[0-9]{2}|[a\-z])$",
            r"^[0-9]{2,}$",
            r"^José$",
            r"^a+?$",
            r"^a{2}?$",
            r"^(?:a)*?$",
            r"^[a-z]{2,3}?$",
            r"^(a)$",
            r"^(?:a|b)c$",
        ] {
            assert_eq!(browser_problem(pattern), None, "{pattern}");
        }
    }

    #[test]
    fn patterns_a_browser_reads_otherwise_are_refused() {
        for pattern in [
            r"[0-9]",
            r"^[0-9]",
            r"^\w+$",
            r"^\d{4}$",
            r"^\s*$",
            r"^.+$",
            r"^a|b$",
            r"^[a-z_-]+$",
            r"^[-a]$",
            r"^[a(]$",
            r"^[a&&b]$",
            r"^(?i)a$",
            r"^(?P<x>a)$",
            r"^a{x}$",
            r"^{2}$",
            r"^a]$",
            r"^\-$",
            r"^[\d]$",
            r"^a\$",
            // Invalid in a browser's `v` mode, where an invalid pattern checks nothing.
            r"^a+*$",
            r"^a?+$",
            r"^a*+$",
            r"^*a$",
            r"^a{2}*$",
            r"^a{1,2}{3}$",
            r"^a+??$",
            r"^(*a)$",
            r"^(?:|*)$",
            r"^a{3,2}$",
            r"^[z-a]$",
            r"^(a$",
            r"^a)$",
        ] {
            assert!(browser_problem(pattern).is_some(), "{pattern}");
        }
    }
}
