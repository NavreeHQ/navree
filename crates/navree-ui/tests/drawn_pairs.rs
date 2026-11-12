//! The contrast gate covers what the stylesheet draws.
//!
//! This reads every rule of the toolkit's stylesheet, follows nesting (a state
//! such as `&:hover` is the same element; anything else nested is drawn on top
//! of it), the custom properties the stylesheet sets (`--tone` and the like),
//! `color-mix()` and tints over a surface, and works out each colour drawn on
//! another. Text of unknown background is taken on every surface. Every pair it
//! finds must be in `theme::pairs()`, so a new rule that draws an unchecked pair
//! fails here before a theme can pass the gate without it.

use std::collections::{BTreeMap, BTreeSet};

use navree_ui::theme::{Paint, PairKind, SURFACES, Token, pairs};

/// A rule: its selector, its own declarations and what is nested in it.
#[derive(Debug, Default)]
struct Rule {
    selector: String,
    decls: Vec<(String, String)>,
    children: Vec<Rule>,
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(at) = rest.find("/*") {
        out.push_str(&rest[..at]);
        rest = rest[at + 2..]
            .split_once("*/")
            .map_or("", |(_, after)| after);
    }
    out.push_str(rest);
    out
}

/// Text up to the next `{`, `;` or `}` outside quotes and brackets.
fn until_stop(text: &[u8], at: &mut usize) -> (String, Option<u8>) {
    let start = *at;
    let (mut depth, mut quote) = (0i32, None::<u8>);
    while *at < text.len() {
        let byte = text[*at];
        match (quote, byte) {
            (Some(q), b) if b == q => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') => quote = Some(byte),
            (None, b'(') => depth += 1,
            (None, b')') => depth -= 1,
            (None, b'{' | b';' | b'}') if depth <= 0 => {
                let found = String::from_utf8_lossy(&text[start..*at]).trim().to_owned();
                return (found, Some(byte));
            }
            _ => {}
        }
        *at += 1;
    }
    (
        String::from_utf8_lossy(&text[start..]).trim().to_owned(),
        None,
    )
}

fn skip_block(text: &[u8], at: &mut usize) {
    let mut depth = 1;
    while *at < text.len() && depth > 0 {
        match text[*at] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        *at += 1;
    }
}

/// The body of a block, after its `{`, to its `}`.
fn parse_block(text: &[u8], at: &mut usize, rule: &mut Rule) {
    loop {
        let (piece, stop) = until_stop(text, at);
        match stop {
            Some(b'{') => {
                *at += 1;
                let lower = piece.to_ascii_lowercase();
                if lower.starts_with("@keyframes")
                    || lower.starts_with("@font-face")
                    || lower.contains("forced-colors")
                {
                    skip_block(text, at);
                } else if lower.starts_with('@') {
                    // A media or supports block: what it holds applies as written.
                    let mut inner = Rule {
                        selector: "&".to_owned(),
                        ..Rule::default()
                    };
                    parse_block(text, at, &mut inner);
                    rule.children.push(inner);
                } else {
                    let mut inner = Rule {
                        selector: piece,
                        ..Rule::default()
                    };
                    parse_block(text, at, &mut inner);
                    rule.children.push(inner);
                }
            }
            Some(b';') => {
                *at += 1;
                if let Some((name, value)) = piece.split_once(':') {
                    rule.decls
                        .push((name.trim().to_owned(), value.trim().to_owned()));
                }
            }
            Some(b'}') => {
                if let Some((name, value)) = piece.split_once(':') {
                    rule.decls
                        .push((name.trim().to_owned(), value.trim().to_owned()));
                }
                *at += 1;
                return;
            }
            _ => return,
        }
    }
}

fn parse(css: &str) -> Rule {
    let css = strip_comments(css);
    let mut root = Rule::default();
    let bytes = css.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        parse_block(bytes, &mut at, &mut root);
        at += 1;
    }
    root
}

/// A colour as a rule writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Value {
    Token(Token),
    Mix(Token, u8, Token),
    Hover(Token, u8),
    /// A token laid thin over whatever is under it.
    Tint(Token, u8),
    /// Nothing of its own: what is under it shows.
    Clear,
    /// Not a colour this check reads (a gradient, a system colour, an image).
    Other,
}

fn token(name: &str) -> Option<Token> {
    Token::ALL.into_iter().find(|t| t.name() == name)
}

/// The `--name` of `var(--name)` or `var(--name,fallback)`, and the fallback.
fn var_of(value: &str) -> Option<(&str, Option<&str>)> {
    let inner = value.strip_prefix("var(--")?.strip_suffix(')')?;
    Some(match inner.split_once(',') {
        Some((name, fallback)) => (name.trim(), Some(fallback.trim())),
        None => (inner.trim(), None),
    })
}

/// The custom properties the stylesheet sets, each with every value it is given.
type Customs = BTreeMap<String, BTreeSet<String>>;
/// The value each custom property has on one element: one choice per name.
type Env = BTreeMap<String, String>;

/// The stylesheet's custom properties `value` reads, through their own values.
fn refs(value: &str, customs: &Customs, out: &mut BTreeSet<String>) {
    let mut rest = value;
    while let Some(at) = rest.find("var(--") {
        rest = &rest[at + 6..];
        let end = rest.find([')', ',']).unwrap_or(rest.len());
        let name = rest[..end].trim().to_owned();
        if let Some(options) = customs.get(&name)
            && out.insert(name)
        {
            for option in options {
                refs(option, customs, out);
            }
        }
    }
}

/// Every colour `value` can be on an element whose custom properties are `env`;
/// one not in `env` is expanded to each value the stylesheet gives it.
fn values(value: &str, customs: &Customs, env: &Env, depth: u8) -> Vec<Value> {
    let value = value.trim().trim_end_matches("!important").trim();
    let customs: &Customs = customs;
    if depth > 4 {
        return vec![Value::Other];
    }
    if matches!(
        value,
        "transparent" | "none" | "inherit" | "currentColor" | "currentcolor"
    ) {
        return vec![Value::Clear];
    }
    // A layered background: its colour is the first layer when that is a token.
    let first = if value.starts_with("var(") {
        let end = value.find(')').map_or(value.len(), |at| at + 1);
        // `var(--a,var(--b))` closes later.
        let close = if value[..end].matches('(').count() > 1 {
            value
                .match_indices(')')
                .nth(1)
                .map_or(end, |(at, _)| at + 1)
        } else {
            end
        };
        &value[..close]
    } else {
        value
    };
    if let Some((name, fallback)) = var_of(first) {
        if let Some(t) = token(name) {
            return vec![Value::Token(t)];
        }
        let mut out = Vec::new();
        if let Some(chosen) = env.get(name) {
            out.extend(values(chosen, customs, env, depth + 1));
        } else if let Some(options) = customs.get(name) {
            for option in options {
                out.extend(values(option, customs, env, depth + 1));
            }
        }
        if let Some(fallback) = fallback {
            out.extend(values(fallback, customs, env, depth + 1));
        }
        if out.is_empty() {
            out.push(Value::Other);
        }
        return out;
    }
    if let Some(inner) = value
        .strip_prefix("color-mix(in oklab,")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        let (a, b) = inner.rsplit_once(',').unwrap_or((inner, ""));
        let (a, percent) = a.trim().rsplit_once(' ').unwrap_or((a, "50%"));
        let percent: u8 = percent.trim_end_matches('%').parse().unwrap_or(50);
        let b = b.trim();
        let mut out = Vec::new();
        for a in values(a, customs, env, depth + 1) {
            let Value::Token(a) = a else {
                out.push(Value::Other);
                continue;
            };
            if b == "transparent" {
                out.push(Value::Tint(a, percent));
            } else if b == "var(--hover-mix)" {
                out.push(Value::Hover(a, percent));
            } else {
                for b in values(b, customs, env, depth + 1) {
                    out.push(match b {
                        Value::Token(b) => Value::Mix(a, percent, b),
                        _ => Value::Other,
                    });
                }
            }
        }
        return out;
    }
    vec![Value::Other]
}

/// What is under an element: a known colour, or any of the surfaces.
#[derive(Clone, Debug)]
struct Under(Vec<Paint>);

impl Under {
    fn any() -> Self {
        Self(SURFACES.into_iter().map(Paint::Token).collect())
    }

    /// `value` painted over this.
    fn with(&self, value: Value) -> Self {
        match value {
            Value::Token(t) => Self(vec![Paint::Token(t)]),
            Value::Mix(a, p, b) => Self(vec![Paint::Mix(a, p, b)]),
            Value::Hover(a, p) => Self(vec![Paint::Hover(a, p)]),
            Value::Tint(a, p) => Self(
                self.0
                    .iter()
                    .map(|under| match under {
                        Paint::Token(surface) => Paint::Tint(a, p, *surface),
                        // A tint on a tint: the surface beneath both.
                        Paint::Tint(_, _, surface) => Paint::Tint(a, p, *surface),
                        other => *other,
                    })
                    .collect(),
            ),
            Value::Clear | Value::Other => self.clone(),
        }
    }
}

fn paint(value: Value) -> Option<Paint> {
    match value {
        Value::Token(t) => Some(Paint::Token(t)),
        Value::Mix(a, p, b) => Some(Paint::Mix(a, p, b)),
        Value::Hover(a, p) => Some(Paint::Hover(a, p)),
        _ => None,
    }
}

/// Whether a nested selector names the same element in another state.
fn same_element(selector: &str) -> bool {
    selector.split(',').all(|alternative| {
        let alternative = alternative.trim();
        alternative == "&"
            || (alternative.starts_with('&')
                && !alternative.contains("::")
                && !alternative[1..].contains([' ', '>', '+', '~']))
    })
}

/// Whether what a rule colours is a mark, not text: an icon, a dot, a pseudo-element.
fn graphic(selector: &str) -> bool {
    [
        "icon",
        "svg",
        "::before",
        "::after",
        "-dot",
        "spinner",
        "indicator",
        "check",
        "thumb",
    ]
    .iter()
    .any(|mark| selector.contains(mark))
}

/// Rules that draw on purpose what a theme may fail: the builder's own samples
/// and swatches, which show a draft as it is.
fn skipped(selector: &str) -> bool {
    selector.contains("theme-contrast-sample")
        || selector.contains("theme-sample")
        || selector.contains("theme-swatch")
        || selector.contains("theme-dot")
        || selector.contains("data-token")
        || selector.contains("data-fg")
}

#[derive(Debug)]
struct Drawn {
    text: Paint,
    on: Paint,
    kind: PairKind,
    selector: String,
}

/// Every way to choose one value for each name in `names`, laid over `env`.
fn choices(names: &BTreeSet<String>, customs: &Customs, env: &Env) -> Vec<Env> {
    let mut out = vec![env.clone()];
    for name in names {
        if env.contains_key(name) {
            continue;
        }
        let options = &customs[name];
        out = out
            .into_iter()
            .flat_map(|env| {
                options.iter().map(move |option| {
                    let mut next = env.clone();
                    next.insert(name.clone(), option.clone());
                    next
                })
            })
            .collect();
    }
    out
}

/// The custom properties `rule` sets on its own element.
fn sets(rule: &Rule, customs: &Customs, env: &Env) -> Env {
    let mut env = env.clone();
    for (name, value) in &rule.decls {
        if let Some(name) = name.strip_prefix("--")
            && customs.contains_key(name)
        {
            env.insert(name.to_owned(), value.clone());
        }
    }
    env
}

/// Whether a shape's colour is a line or a surface: a separator, a track, a box shown
/// by its border. These need no contrast of their own.
fn decorative(value: Value) -> bool {
    matches!(
        value,
        Value::Token(
            Token::Line
                | Token::Line2
                | Token::Line3
                | Token::Bar
                | Token::Ink0
                | Token::Ink1
                | Token::Ink2
                | Token::Ink3
        )
    )
}

/// One element's rule. `beneath` is what is under the element; `bg` the
/// background a state of the same element inherits from its rule.
#[allow(clippy::too_many_arguments)]
fn walk(
    rule: &Rule,
    path: &str,
    colour: Option<Vec<Value>>,
    bg: Option<Vec<Value>>,
    beneath: &Under,
    customs: &Customs,
    env: &Env,
    out: &mut Vec<Drawn>,
) {
    let own = |name: &str| {
        rule.decls
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
    };
    let own_colour = own("color");
    let own_bg = own("background-color").or_else(|| own("background"));
    // A shape drawn by its background: a masked mark or a pseudo-element.
    let shape = rule
        .decls
        .iter()
        .any(|(n, _)| n == "mask" || n == "-webkit-mask" || n == "mask-image")
        || rule.selector.contains("::");
    let mut names = BTreeSet::new();
    for value in own_colour.iter().chain(&own_bg) {
        refs(value, customs, &mut names);
    }
    let here = format!("{path} {}", rule.selector).trim().to_owned();
    for env in choices(&names, customs, &sets(rule, customs, env)) {
        let own_colour = own_colour.as_deref().map(|v| values(v, customs, &env, 0));
        let own_bg = own_bg.as_deref().map(|v| values(v, customs, &env, 0));
        let colour = own_colour.clone().or_else(|| colour.clone());
        // A state's background takes the place of its rule's; either lies on what is beneath.
        let element_bg = own_bg.clone().or_else(|| bg.clone());
        let unders: Vec<Under> = match &element_bg {
            Some(options) => options.iter().map(|v| beneath.with(*v)).collect(),
            None => vec![beneath.clone()],
        };
        if !skipped(&here) {
            let kind = if shape || graphic(&here) {
                PairKind::Graphic
            } else {
                PairKind::Text
            };
            if shape {
                // The colour seen is the shape's own: its background, else its text colour.
                let drawn = own_bg.clone().or_else(|| own_colour.clone());
                for value in drawn.iter().flatten() {
                    if decorative(*value) {
                        continue;
                    }
                    let Some(text) = paint(*value) else { continue };
                    for on in &beneath.0 {
                        out.push(Drawn {
                            text,
                            on: *on,
                            kind,
                            selector: here.clone(),
                        });
                    }
                }
            } else if (own_colour.is_some() || own_bg.is_some())
                && let Some(colours) = &colour
            {
                for c in colours {
                    let Some(text) = paint(*c) else { continue };
                    for u in &unders {
                        for on in &u.0 {
                            out.push(Drawn {
                                text,
                                on: *on,
                                kind,
                                selector: here.clone(),
                            });
                        }
                    }
                }
            }
        }
        for child in &rule.children {
            if same_element(&child.selector) {
                // A state of the same element: the same beneath, this rule's background.
                walk(
                    child,
                    &here,
                    colour.clone(),
                    element_bg.clone(),
                    beneath,
                    customs,
                    &env,
                    out,
                );
            } else {
                // Anything else nested inherits the colour and the custom properties,
                // and is drawn over this element.
                for u in &unders {
                    walk(child, &here, colour.clone(), None, u, customs, &env, out);
                }
            }
        }
    }
}

/// The stylesheet's own custom properties, each with every value it is given.
fn customs(rule: &Rule, out: &mut Customs) {
    for (name, value) in &rule.decls {
        if let Some(name) = name.strip_prefix("--")
            && token(name).is_none()
        {
            out.entry(name.to_owned())
                .or_default()
                .insert(value.clone());
        }
    }
    for child in &rule.children {
        customs(child, out);
    }
}

fn stylesheet() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("styles");
    let mut files: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "css")
                && path.file_name().is_some_and(|name| name != "tokens.css")
        })
        .collect();
    files.sort();
    files.push(root.join("extra/theme-builder.css"));
    // The modules that link a stylesheet of their own draw colours too.
    for name in [
        "markdown",
        "upload",
        "otp",
        "carousel",
        "menubar",
        "parts",
        "app-blocks",
    ] {
        files.push(root.join(format!("extra/{name}.css")));
    }
    files
        .iter()
        .map(|path| std::fs::read_to_string(path).unwrap().replace("\r\n", "\n"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn drawn() -> Vec<Drawn> {
    let sheet = parse(&stylesheet());
    let mut props = BTreeMap::new();
    customs(&sheet, &mut props);
    let mut out = Vec::new();
    for rule in &sheet.children {
        walk(
            rule,
            "",
            None,
            None,
            &Under::any(),
            &props,
            &Env::new(),
            &mut out,
        );
    }
    out
}

#[test]
fn every_pair_the_stylesheet_draws_is_checked() {
    let found = drawn();
    assert!(
        found.len() > 200,
        "the stylesheet was read: {}",
        found.len()
    );
    let mut missing: BTreeMap<String, String> = BTreeMap::new();
    for drawn in &found {
        let covered = pairs().iter().any(|pair| {
            pair.text == drawn.text
                && pair.on == drawn.on
                && (drawn.kind == PairKind::Graphic || pair.kind == PairKind::Text)
        });
        if !covered {
            missing
                .entry(format!(
                    "{:?} {} on {}",
                    drawn.kind,
                    drawn.text.describe(),
                    drawn.on.describe()
                ))
                .or_insert_with(|| drawn.selector.clone());
        }
    }
    assert!(
        missing.is_empty(),
        "{} pairs drawn but not checked:\n{}",
        missing.len(),
        missing
            .iter()
            .map(|(pair, at)| format!("  {pair}   (e.g. {at})"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn the_reader_sees_tints_states_and_custom_properties() {
    let found = drawn();
    let has = |text: Paint, on: Paint| found.iter().any(|d| d.text == text && d.on == on);
    // The destructive button's hover, over a card.
    assert!(has(
        Paint::Token(Token::Danger),
        Paint::Tint(Token::Danger, 18, Token::Ink1)
    ));
    // The primary button's hover, toward the hover mix.
    assert!(has(
        Paint::Token(Token::AccentInk),
        Paint::Hover(Token::Accent, 86)
    ));
    // A tone badge, through `--tone`.
    assert!(has(
        Paint::Mix(Token::Info, 78, Token::Fg),
        Paint::Tint(Token::Info, 13, Token::Ink0)
    ));
    // Body text on the page.
    assert!(has(Paint::Token(Token::Fg1), Paint::Token(Token::Ink0)));
}
