//! `navree_validation`'s HTML sanitizer, under policies built from the
//! input.
//!
//! Input: a flags byte (bit 0: start from the default policy, else from no
//! tags; bit 1: set `rel="noopener"` on links; bits 2 to 7: the URL schemes
//! `http`, `https`, `mailto`, `tel`, `javascript`, `data`), a count and that
//! many bytes naming tags to allow, a count and that many pairs of bytes
//! naming an attribute to allow on a tag (or on every tag), then the HTML.
//!
//! Invariants: the output parses back to the tree the sanitizer checked
//! (apart from the `tbody`, `tr` and `colgroup` the parser adds to a table
//! itself); that parse holds no element, attribute or URL scheme the policy
//! doesn't allow, no `script` or `style`, no SVG animation and no comment;
//! sanitizing the output again changes nothing; what was kept nests within
//! the policy's limit; text-only output has no raw `<`; and all of that holds
//! for an input over a limit too, which comes back escaped.
#![no_main]

use std::collections::{BTreeMap, BTreeSet};

use libfuzzer_sys::fuzz_target;
use navree_validation::HtmlPolicy;
use navree_validation::fuzzing::{Node, parse, sanitize};

const HTML: &str = "http://www.w3.org/1999/xhtml";
const SVG: &str = "http://www.w3.org/2000/svg";

#[rustfmt::skip]
const TAGS: [&str; 72] = [
    "a", "b", "p", "div", "span", "table", "tbody", "tr", "td", "th", "thead", "colgroup", "col",
    "caption", "pre", "textarea", "title", "iframe", "noscript", "noembed", "noframes", "xmp",
    "plaintext", "listing", "template", "select", "option", "selectedcontent", "form", "button",
    "input", "img", "object", "embed", "video", "svg", "g", "circle", "foreignObject", "desc",
    "image", "use", "animate", "set", "animateTransform", "math", "mi", "mo", "mtext",
    "annotation-xml", "mglyph", "malignmark", "style", "script", "li", "ul", "ol", "h1",
    "blockquote", "font", "center", "nobr", "marquee", "area", "map", "link", "meta", "base",
    "frameset", "body", "html", "isindex",
];

#[rustfmt::skip]
const ATTRIBUTES: [&str; 28] = [
    "href", "src", "title", "action", "formaction", "data", "poster", "cite", "ping",
    "background", "style", "class", "id", "rel", "encoding", "attributeName", "values", "to",
    "from", "lang", "dir", "xlink:href", "srcdoc", "name", "value", "type", "onclick",
    "shadowrootmode",
];

const SCHEMES: [&str; 6] = ["http", "https", "mailto", "tel", "javascript", "data"];

/// Attributes that hold a URL: their values must have an allowed scheme or
/// be relative.
#[rustfmt::skip]
const URL_ATTRIBUTES: [&str; 10] = [
    "href", "src", "action", "formaction", "data", "poster", "cite", "ping", "background",
    "xlink:href",
];

const SVG_ANIMATIONS: [&str; 5] = [
    "animate",
    "animateColor",
    "animateMotion",
    "animateTransform",
    "set",
];

/// The policy the input describes, and the HTML.
fn read(data: &[u8]) -> Option<(HtmlPolicy, &str)> {
    let (&flags, rest) = data.split_first()?;
    let (&count, rest) = rest.split_first()?;
    let (tags, rest) = rest.split_at_checked(usize::from(count % 16))?;
    let (&count, rest) = rest.split_first()?;
    let (pairs, rest) = rest.split_at_checked(usize::from(count % 16) * 2)?;
    let html = std::str::from_utf8(rest).ok()?;

    let mut policy = if flags & 1 == 1 {
        HtmlPolicy::default()
    } else {
        HtmlPolicy::text_only()
    };
    policy.link_rel = if flags & 2 == 2 {
        "noopener".to_owned()
    } else {
        String::new()
    };
    policy.url_schemes = SCHEMES
        .iter()
        .enumerate()
        .filter(|(i, _)| flags & (4 << i) != 0)
        .map(|(_, scheme)| (*scheme).to_owned())
        .collect();
    for &tag in tags {
        policy
            .tags
            .insert(TAGS[usize::from(tag) % TAGS.len()].to_owned());
    }
    for pair in pairs.chunks_exact(2) {
        let attribute = ATTRIBUTES[usize::from(pair[1]) % ATTRIBUTES.len()].to_owned();
        match TAGS.get(usize::from(pair[0]) % (TAGS.len() + 1)) {
            Some(tag) => {
                policy
                    .tag_attributes
                    .entry((*tag).to_owned())
                    .or_default()
                    .insert(attribute);
            }
            None => {
                policy.generic_attributes.insert(attribute);
            }
        }
    }
    Some((policy, html))
}

/// What the policy keeps, the way `HtmlPolicy::clean` reads it: `script`
/// and `style` never, `rel` not while `link_rel` is set.
struct Allowed<'a> {
    tags: &'a BTreeSet<String>,
    generic: &'a BTreeSet<String>,
    per_tag: &'a BTreeMap<String, BTreeSet<String>>,
    schemes: &'a BTreeSet<String>,
    rel: &'a str,
}

impl Allowed<'_> {
    fn tag(&self, name: &str) -> bool {
        self.tags.contains(name) && name != "script" && name != "style"
    }

    /// The `tbody`, `tr` and `colgroup` an HTML parser adds to a table.
    fn implied(
        &self,
        namespace: &str,
        name: &str,
        attributes: &[(String, String, String)],
    ) -> bool {
        namespace == HTML
            && matches!(name, "tbody" | "tr" | "colgroup")
            && attributes.is_empty()
            && !self.tag(name)
    }

    fn attribute(&self, tag: &str, name: &str, value: &str) -> bool {
        if name == "rel" && !self.rel.is_empty() {
            return matches!(tag, "a" | "area") && value == self.rel;
        }
        if matches!(name, "srcdoc" | "http-equiv") || name.starts_with("on") {
            return false;
        }
        let listed = self.generic.contains(name)
            || self
                .per_tag
                .get(tag)
                .is_some_and(|attributes| attributes.contains(name));
        listed && (!URL_ATTRIBUTES.contains(&name) || self.url(value))
    }

    fn url(&self, value: &str) -> bool {
        let scheme_ok = match url::Url::parse(value) {
            Ok(url) => self.schemes.contains(url.scheme()),
            // With a host of its own, a relative URL leaves with the page's
            // scheme.
            Err(url::ParseError::RelativeUrlWithoutBase) => {
                let site = url::Url::parse("https://site.invalid/").ok();
                let host = site.as_ref().and_then(|site| site.join(value).ok());
                host.is_some_and(|url| url.host_str() == Some("site.invalid"))
                    || (self.schemes.contains("http") && self.schemes.contains("https"))
            }
            Err(_) => false,
        };
        // Independently of the URL parser: the scheme a browser reads,
        // without leading spaces and controls, tabs and newlines.
        let squeezed: String = value
            .trim_start_matches(|c: char| c <= ' ')
            .chars()
            .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
            .collect::<String>()
            .to_ascii_lowercase();
        let dangerous = ["javascript:", "vbscript:", "data:"]
            .iter()
            .find(|prefix| squeezed.starts_with(*prefix))
            .is_none_or(|prefix| self.schemes.contains(prefix.trim_end_matches(':')));
        scheme_ok && dangerous
    }
}

/// `nodes` without the elements `drop` returns `true` for (their content
/// stays), with adjacent text joined again.
fn without(
    nodes: Vec<Node>,
    drop: impl Fn(&str, &str, &[(String, String, String)]) -> bool,
) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::new();
    let mut open = Vec::new();
    for node in nodes {
        match node {
            Node::Open {
                namespace,
                name,
                attributes,
            } => {
                let dropped = drop(&namespace, &name, &attributes);
                open.push(dropped);
                if !dropped {
                    out.push(Node::Open {
                        namespace,
                        name,
                        attributes,
                    });
                }
            }
            Node::Close => {
                if !open.pop().unwrap_or(false) {
                    out.push(Node::Close);
                }
            }
            Node::Text(text) => match out.last_mut() {
                Some(Node::Text(previous)) => previous.push_str(&text),
                _ => out.push(Node::Text(text)),
            },
            other => out.push(other),
        }
    }
    out
}

fn check(policy: &HtmlPolicy, html: &str) {
    let allowed = Allowed {
        tags: &policy.tags,
        generic: &policy.generic_attributes,
        per_tag: &policy.tag_attributes,
        schemes: &policy.url_schemes,
        rel: &policy.link_rel,
    };
    let (out, checked) = sanitize(policy, html);
    assert_eq!(policy.clean(html), out, "clean and sanitize agree");

    let parsed = without(parse(&out), |namespace, name, attributes| {
        allowed.implied(namespace, name, attributes)
    });
    assert_eq!(
        parsed, checked,
        "{out:?} parses to another tree than the one checked"
    );

    for node in &parsed {
        match node {
            Node::Open {
                namespace,
                name,
                attributes,
            } => {
                assert!(allowed.tag(name), "<{name}> kept in {out:?}");
                assert!(
                    !(namespace == SVG && SVG_ANIMATIONS.contains(&name.as_str())),
                    "<{name}> kept in {out:?}"
                );
                for (_, attribute, value) in attributes {
                    assert!(
                        allowed.attribute(name, attribute, value),
                        "{attribute}={value:?} kept on <{name}> in {out:?}"
                    );
                }
            }
            Node::Comment | Node::Other => panic!("a comment or doctype kept in {out:?}"),
            Node::Close | Node::Text(_) => {}
        }
    }

    // An output over a limit is the escaped input, or is longer than the input.
    if policy.try_clean(&out).is_ok() {
        assert_eq!(
            policy.clean(&out),
            out,
            "sanitizing {out:?} again changes it"
        );
    }

    // What was kept nests within the limit.
    let mut depth = 0usize;
    for node in &checked {
        match node {
            Node::Open { .. } => {
                depth += 1;
                assert!(depth <= policy.max_depth, "{out:?} nests too deep");
            }
            Node::Close => depth -= 1,
            _ => {}
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((policy, html)) = read(data) else {
        return;
    };
    // The same with limits an input of this size can reach.
    check(&policy.clone().limits(1024, 64, 8), html);
    let text_only = HtmlPolicy::text_only().clean(html);
    assert!(!text_only.contains('<'), "raw `<` in {text_only:?}");
    check(&policy, html);
});
