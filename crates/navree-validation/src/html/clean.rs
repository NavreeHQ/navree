//! The sanitizer behind [`HtmlPolicy::clean`](super::HtmlPolicy::clean).
//!
//! 1. html5ever parses the input as the content of a `<div>`.
//! 2. One walk over the tree, in document order, keeps each node or not:
//!    text is kept; comments, doctypes and processing instructions are
//!    removed; an element that is not allowed is removed and its children
//!    take its place; `script` and `style`, and an element that is in a
//!    namespace the HTML parser would not put it in under its new parent
//!    (`<svg><p>` once `svg` is gone), are removed with all they hold. A
//!    kept element keeps only its allowed attributes, and a URL attribute
//!    only with an allowed scheme or a relative URL.
//! 3. The tree is written out and parsed again. The result is returned only
//!    when that parse is the tree that was kept (apart from the `tbody`,
//!    `tr` and `colgroup` the parser adds to a table by itself), so what a
//!    browser builds from it is what was checked. When it isn't (`<p>` in a
//!    `<p>`, which the parser splits, or a blank line a `<pre>` drops), only
//!    the text is kept. So an input is parsed twice at most, whatever it
//!    holds, and costs no more than markup that reads back the same.

use std::collections::{BTreeMap, BTreeSet};

use html5ever::{Attribute, QualName, local_name, ns};

use super::tree::{Data, Id, Limits, Tree, escape_text};
use super::{CONTENT_TAGS, HtmlError, NEVER_ATTRIBUTES};

/// SVG elements that change another element's attributes, which an
/// allow-list of attributes can't vet: never kept.
const SVG_ANIMATION_TAGS: [&str; 5] = [
    "animate",
    "animateColor",
    "animateMotion",
    "animateTransform",
    "set",
];

/// Attributes that hold a URL, on any element: kept only with an allowed
/// scheme or a relative URL.
const URL_ATTRIBUTES: [&str; 16] = [
    "action",
    "background",
    "cite",
    "codebase",
    "data",
    "dynsrc",
    "formaction",
    "href",
    "icon",
    "longdesc",
    "lowsrc",
    "manifest",
    "ping",
    "poster",
    "src",
    "xlink:href",
];

/// A base for reading a relative URL: where it leaves for is what counts.
const SITE: &str = "https://site.invalid/";

/// The elements of SVG 2 and SVG 1.1's `animateColor` (sorted).
#[rustfmt::skip]
const SVG_TAGS: [&str; 65] = [
    "a", "animate", "animateColor", "animateMotion", "animateTransform", "circle",
    "clipPath", "defs", "desc", "discard", "ellipse", "feBlend", "feColorMatrix",
    "feComponentTransfer", "feComposite", "feConvolveMatrix", "feDiffuseLighting",
    "feDisplacementMap", "feDistantLight", "feDropShadow", "feFlood", "feFuncA", "feFuncB",
    "feFuncG", "feFuncR", "feGaussianBlur", "feImage", "feMerge", "feMergeNode",
    "feMorphology", "feOffset", "fePointLight", "feSpecularLighting", "feSpotLight",
    "feTile", "feTurbulence", "filter", "foreignObject", "g", "image", "line",
    "linearGradient", "marker", "mask", "metadata", "mpath", "path", "pattern", "polygon",
    "polyline", "radialGradient", "rect", "script", "set", "stop", "style", "svg",
    "switch", "symbol", "text", "textPath", "title", "tspan", "use", "view",
];

/// The elements of MathML, presentation and content markup (sorted).
#[rustfmt::skip]
const MATHML_TAGS: [&str; 194] = [
    "abs", "and", "annotation", "annotation-xml", "apply", "approx", "arccos", "arccosh",
    "arccot", "arccoth", "arccsc", "arccsch", "arcsec", "arcsech", "arcsin", "arcsinh",
    "arctan", "arctanh", "arg", "bind", "bvar", "card", "cartesianproduct", "cbytes",
    "ceiling", "cerror", "ci", "cn", "codomain", "complexes", "compose", "condition",
    "conjugate", "cos", "cosh", "cot", "coth", "cs", "csc", "csch", "csymbol", "curl",
    "declare", "degree", "determinant", "diff", "divergence", "divide", "domain",
    "domainofapplication", "emptyset", "eq", "equivalent", "eulergamma", "exists", "exp",
    "exponentiale", "factorial", "factorof", "false", "floor", "fn", "forall", "gcd", "geq",
    "grad", "gt", "ident", "image", "imaginary", "imaginaryi", "implies", "in", "infinity",
    "int", "integers", "intersect", "interval", "inverse", "lambda", "laplacian", "lcm",
    "leq", "limit", "list", "ln", "log", "logbase", "lowlimit", "lt", "maction",
    "maligngroup", "malignmark", "math", "matrix", "matrixrow", "max", "mean", "median",
    "menclose", "merror", "mfenced", "mfrac", "mglyph", "mi", "min", "minus", "mlabeledtr",
    "mlongdiv", "mmultiscripts", "mn", "mo", "mode", "moment", "momentabout", "mover",
    "mpadded", "mphantom", "mprescripts", "mroot", "mrow", "ms", "mscarries", "mscarry",
    "msgroup", "msline", "mspace", "msqrt", "msrow", "mstack", "mstyle", "msub", "msubsup",
    "msup", "mtable", "mtd", "mtext", "mtr", "munder", "munderover", "naturalnumbers",
    "neq", "none", "not", "notanumber", "notin", "notprsubset", "notsubset", "or",
    "otherwise", "outerproduct", "partialdiff", "pi", "piece", "piecewise", "plus", "power",
    "primes", "product", "prsubset", "quotient", "rationals", "real", "reals", "reln",
    "rem", "root", "scalarproduct", "sdev", "sec", "sech", "selector", "semantics", "sep",
    "set", "setdiff", "share", "sin", "sinh", "span", "subset", "sum", "tan", "tanh",
    "tendsto", "times", "transpose", "true", "union", "uplimit", "variance", "vector",
    "vectorproduct", "xor",
];

fn is_svg_tag(local: &str) -> bool {
    SVG_TAGS.binary_search(&local).is_ok()
}

fn is_mathml_tag(local: &str) -> bool {
    MATHML_TAGS.binary_search(&local).is_ok()
}

/// Whether an HTML element named `local` is one the parser would make: not
/// an SVG or MathML name, except the names HTML shares with them.
fn is_html_tag(local: &str) -> bool {
    (!is_svg_tag(local) && !is_mathml_tag(local))
        || matches!(local, "title" | "style" | "font" | "a" | "script" | "span")
}

/// `html` as the text an HTML parser would read from it once it is escaped:
/// no NUL, no leading byte order mark, and line ends as a single LF.
pub(crate) fn as_text(html: &str) -> String {
    html.replace('\0', "")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim_start_matches('\u{feff}')
        .to_owned()
}

/// A policy, read for one call: [`HtmlPolicy`](super::HtmlPolicy)'s lists.
pub(crate) struct Sanitizer<'a> {
    pub(crate) tags: &'a BTreeSet<String>,
    pub(crate) generic_attributes: &'a BTreeSet<String>,
    pub(crate) tag_attributes: &'a BTreeMap<String, BTreeSet<String>>,
    pub(crate) url_schemes: &'a BTreeSet<String>,
    /// The `rel` set on every link, `None` for none.
    pub(crate) link_rel: Option<&'a str>,
    pub(crate) limits: Limits,
}

/// What an output parses to (read by the fuzz targets).
#[cfg_attr(not(feature = "fuzzing"), allow(dead_code))]
pub(crate) enum Checked {
    /// The tree that was kept (without the table parts
    /// [`Sanitizer::implied_by_parser`] allows).
    Tree(Tree),
    /// Only this text.
    Text(String),
}

/// A node to visit, and the kept element (or the root) it goes under.
struct Visit {
    id: Id,
    parent: Id,
    /// Whether a kept ancestor is a `<select>`.
    in_select: bool,
}

impl Sanitizer<'_> {
    /// `html` sanitized, or escaped whole when it passes a limit.
    pub(crate) fn clean(&self, html: &str) -> String {
        match self.sanitize(html) {
            Ok((out, _)) => out,
            Err(_) => escape_text(&as_text(html)),
        }
    }

    /// `html` sanitized, and what it parses to.
    pub(crate) fn sanitize(&self, html: &str) -> Result<(String, Checked), HtmlError> {
        let mut tree = Tree::parse(html, self.limits)?;
        // An output may be longer than its input (`&` is `&amp;`). It has
        // passed the input's limit, and the others still apply.
        let output = Limits {
            max_input_bytes: usize::MAX,
            ..self.limits
        };
        self.clean_tree(&mut tree);
        let out = tree.to_html();
        let again = Tree::parse(&out, output)?;
        let same = tree.events(|_, _| false)
            == again.events(|name, attrs| self.implied_by_parser(name, attrs));
        if same {
            return Ok((out, Checked::Tree(tree)));
        }
        // A browser would build another tree from this output, so it is not
        // returned: the text of the tree that was kept is, as a parser reads it.
        let text = as_text(&tree.text());
        Ok((escape_text(&text), Checked::Text(text)))
    }

    fn tag_allowed(&self, local: &str) -> bool {
        self.tags.contains(local) && !CONTENT_TAGS.contains(&local)
    }

    fn attribute_allowed(&self, tag: &str, attribute: &str, value: &str) -> bool {
        let listed = self.generic_attributes.contains(attribute)
            || self
                .tag_attributes
                .get(tag)
                .is_some_and(|attributes| attributes.contains(attribute));
        if !listed
            || (self.link_rel.is_some() && attribute == "rel")
            || NEVER_ATTRIBUTES.contains(&attribute)
            || attribute.starts_with("on")
        {
            return false;
        }
        match attribute {
            // Candidates split by commas, each a URL and maybe a size.
            "srcset" | "imagesrcset" => value.split(',').all(|candidate| {
                let url = candidate.split_ascii_whitespace().next();
                url.is_none_or(|url| self.url_allowed(url))
            }),
            // URLs split by spaces.
            "ping" => value
                .split_ascii_whitespace()
                .all(|url| self.url_allowed(url)),
            _ => !URL_ATTRIBUTES.contains(&attribute) || self.url_allowed(value),
        }
    }

    /// An absolute URL with an allowed scheme, or a relative URL. The
    /// scheme is the one a browser reads (`url` parses as WHATWG URL does:
    /// leading spaces and controls go, tabs and newlines in it are ignored,
    /// case doesn't count).
    ///
    /// A relative URL with a host of its own (`//evil.test`, and `/\evil.test`
    /// and `\\evil.test`, which a browser reads the same) leaves the site with
    /// the page's scheme, which may be `http` or `https`: it stays only when
    /// both are allowed.
    fn url_allowed(&self, value: &str) -> bool {
        match url::Url::parse(value) {
            Ok(url) => self.url_schemes.contains(url.scheme()),
            Err(url::ParseError::RelativeUrlWithoutBase) => {
                let leaves_the_site = url::Url::parse(SITE)
                    .and_then(|site| Ok(site.join(value)?.host_str() != site.host_str()));
                !leaves_the_site.unwrap_or(true)
                    || ["http", "https"]
                        .iter()
                        .all(|scheme| self.url_schemes.contains(*scheme))
            }
            Err(_) => false,
        }
    }

    /// Whether an element is kept; its attributes are filtered when it is.
    fn keep_element(&self, name: &QualName, attrs: &mut Vec<Attribute>) -> bool {
        let local = &*name.local;
        if !self.tag_allowed(local) {
            return false;
        }
        attrs.retain(|attr| self.attribute_allowed(local, &attr.name.local, &attr.value));
        !(name.ns == ns!(svg) && SVG_ANIMATION_TAGS.contains(&local))
    }

    /// The `tbody`, `tr` and `colgroup` the parser puts into a table by
    /// itself, when the policy doesn't keep them: a browser adds them to
    /// the output too.
    pub(crate) fn implied_by_parser(&self, name: &QualName, attrs: &[Attribute]) -> bool {
        name.ns == ns!(html)
            && matches!(&*name.local, "tbody" | "tr" | "colgroup")
            && attrs.is_empty()
            && !self.tag_allowed(&name.local)
    }

    /// Removes from the tree what the policy doesn't keep (see the module
    /// documentation).
    fn clean_tree(&self, tree: &mut Tree) {
        let Some(root) = tree.root else {
            return;
        };
        let Some(top) = tree
            .nodes
            .get_mut(root)
            .map(|n| std::mem::take(&mut n.children))
        else {
            return;
        };
        let mut stack: Vec<Visit> = top
            .into_iter()
            .rev()
            .map(|id| Visit {
                id,
                parent: root,
                in_select: false,
            })
            .collect();
        while let Some(Visit {
            id,
            parent,
            in_select,
        }) = stack.pop()
        {
            let Some(node) = tree.nodes.get_mut(id) else {
                continue;
            };
            // A browser copies the chosen `<option>` into a
            // `<selectedcontent>`: what it held is never shown.
            if in_select && node.is_html("selectedcontent") {
                node.children.clear();
            }
            let (keep, name) = match &mut node.data {
                Data::Text(_) => (true, None),
                Data::Element { name, attrs, .. } => {
                    let keep = self.keep_element(name, attrs);
                    let link = matches!(&*name.local, "a" | "area");
                    if let Some(rel) = self.link_rel.filter(|_| link) {
                        attrs.push(Attribute {
                            name: QualName::new(None, ns!(), local_name!("rel")),
                            value: rel.into(),
                        });
                    }
                    (keep, Some(name.clone()))
                }
                Data::Comment | Data::Document | Data::Other => (false, None),
            };
            if let Some(name) = &name
                && (CONTENT_TAGS.contains(&&*name.local)
                    || !self.expected_namespace(tree, parent, name))
            {
                continue;
            }
            let children = tree
                .nodes
                .get_mut(id)
                .map(|node| std::mem::take(&mut node.children))
                .unwrap_or_default();
            let under = if keep {
                if let Some(parent) = tree.nodes.get_mut(parent) {
                    parent.children.push(id);
                }
                id
            } else {
                parent
            };
            let in_select = in_select
                || (keep
                    && name.is_some_and(|name| name.ns == ns!(html) && &*name.local == "select"));
            stack.extend(children.into_iter().rev().map(|id| Visit {
                id,
                parent: under,
                in_select,
            }));
        }
    }

    /// Whether the HTML parser would put an element named `child` under
    /// `parent` (a kept element or the root), so that the output parses
    /// back with the element in the same namespace: a foreign element's
    /// content that HTML parses as text (`<svg><iframe><a title="</iframe>
    /// <img onerror>">`) must not reach the output as HTML.
    fn expected_namespace(&self, tree: &Tree, parent: Id, child: &QualName) -> bool {
        let Some(Data::Element {
            name: parent,
            attrs: parent_attrs,
            ..
        }) = tree.nodes.get(parent).map(|node| &node.data)
        else {
            return true;
        };
        let local = &*child.local;
        if parent.ns == ns!(html) && child.ns == ns!(svg) {
            // Only `<svg>` enters SVG from HTML,
            local == "svg"
        } else if parent.ns == ns!(html) && child.ns == ns!(mathml) {
            // and only `<math>` enters MathML.
            local == "math"
        } else if parent.ns == ns!(mathml) && child.ns != ns!(mathml) {
            // MathML leaves through a text or an HTML integration point.
            if &*parent.local == "annotation-xml" {
                let mut encodings = parent_attrs
                    .iter()
                    .filter(|attr| attr.name.local == local_name!("encoding"));
                let html_encodings = encodings
                    .clone()
                    .all(|attr| matches!(&*attr.value, "text/html" | "application/xhtml+xml"));
                if child.ns == ns!(html) && html_encodings {
                    is_html_tag(local) && encodings.next().is_some() && encodings.next().is_none()
                } else {
                    local == "svg" && child.ns == ns!(svg)
                }
            } else {
                matches!(&*parent.local, "mi" | "mo" | "mn" | "ms" | "mtext")
                    && (child.ns != ns!(html) || is_html_tag(local))
            }
        } else if parent.ns == ns!(svg) && child.ns != ns!(svg) {
            // SVG leaves through `<foreignObject>`.
            &*parent.local == "foreignObject" && (child.ns != ns!(html) || is_html_tag(local))
        } else if child.ns == ns!(svg) {
            is_svg_tag(local)
        } else if child.ns == ns!(mathml) {
            is_mathml_tag(local)
        } else if child.ns == ns!(html) {
            is_html_tag(local)
        } else {
            parent.ns == child.ns
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::HtmlPolicy;
    use super::super::tree::{Limits, PARSES};

    fn parses(html: &str) -> usize {
        let before = PARSES.with(|parses| parses.get());
        let _ = HtmlPolicy::default().try_clean(html);
        PARSES.with(|parses| parses.get()) - before
    }

    /// Each walk over the tree keeps its own stack: deep nesting can't
    /// overflow the thread's. A policy never gets this deep (`clean` keeps 256
    /// levels at most), so the sanitizer is given limits no policy can set.
    #[test]
    fn deep_nesting_is_cleaned_without_recursion() {
        let depth = 50_000;
        let limits = Limits {
            max_input_bytes: 1 << 20,
            max_nodes: 1 << 20,
            max_depth: depth + 1,
        };
        let html = format!("{}x", "<span>".repeat(depth));
        let policy = HtmlPolicy::default();
        let mut sanitizer = policy.sanitizer();
        sanitizer.limits = limits;
        assert_eq!(
            sanitizer.clean(&html),
            format!("{}x{}", "<span>".repeat(depth), "</span>".repeat(depth))
        );
        let policy = HtmlPolicy::text_only();
        let mut sanitizer = policy.sanitizer();
        sanitizer.limits = limits;
        assert_eq!(sanitizer.clean(&html), "x");
    }

    /// An input is parsed, and its output once: whatever the output reads back
    /// as, no input takes the parser more times than a clean one. A `<pre>`
    /// drops one newline at its start each time it is read, so this one would
    /// be read again for every newline.
    #[test]
    fn no_input_is_parsed_more_than_twice() {
        let newlines = "\n".repeat(8);
        for html in [
            "x".to_owned(),
            "<p>x</p>".to_owned(),
            "&".repeat(100),
            format!("<pre>{newlines}x</pre>"),
            format!("<pre>{newlines}{}</pre>", "&".repeat(1000)),
            "<pre>\n\nx</pre>".to_owned(),
            "<p><button><table></table></button>x</p>".to_owned(),
        ] {
            assert!(parses(&html) <= 2, "{}", &html[..html.len().min(40)]);
        }
    }
}
