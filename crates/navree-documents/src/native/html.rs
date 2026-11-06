//! A forgiving HTML parser for rendered templates: elements, attributes,
//! text with character references, comments and doctypes skipped,
//! `<style>` and `<script>` kept raw, void elements, and the end tags
//! HTML lets authors leave out (`</p>`, `</li>`, `</td>`, `</tr>`).

/// A parsed node.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Node {
    Element(Element),
    Text(String),
}

/// An element with lower-case name and attribute names.
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct Element {
    pub(crate) name: String,
    pub(crate) attrs: Vec<(String, String)>,
    pub(crate) children: Vec<Node>,
}

impl Element {
    /// The value of attribute `name`.
    pub(crate) fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Whether the `class` attribute lists `class`.
    pub(crate) fn has_class(&self, class: &str) -> bool {
        self.attr("class")
            .is_some_and(|classes| classes.split_whitespace().any(|c| c == class))
    }

    /// The text of the element and its descendants.
    pub(crate) fn text(&self) -> String {
        let mut out = String::new();
        collect_text(&self.children, &mut out);
        out
    }

    /// The first descendant (or itself) named `name`.
    pub(crate) fn find(&self, name: &str) -> Option<&Element> {
        if self.name == name {
            return Some(self);
        }
        self.children.iter().find_map(|child| match child {
            Node::Element(element) => element.find(name),
            Node::Text(_) => None,
        })
    }
}

fn collect_text(nodes: &[Node], out: &mut String) {
    for node in nodes {
        match node {
            Node::Text(text) => out.push_str(text),
            Node::Element(element) => collect_text(&element.children, out),
        }
    }
}

const VOID: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Elements whose content is raw text up to their end tag.
const RAW: [&str; 4] = ["style", "script", "textarea", "title"];

/// Elements whose start closes an open `<p>`.
const CLOSES_P: [&str; 30] = [
    "address",
    "article",
    "aside",
    "blockquote",
    "div",
    "dl",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "ul",
    "li",
    "dd",
    "dt",
];

/// Parses `html` into a document element (`#document`) holding the
/// top-level nodes.
pub(crate) fn parse(html: &str) -> Element {
    let mut builder = Builder {
        stack: vec![Element {
            name: "#document".to_owned(),
            ..Element::default()
        }],
    };
    let bytes = html.as_bytes();
    let mut i = 0;
    let mut text_start = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &html[i..];
        let next = rest.as_bytes().get(1).copied().unwrap_or(0);
        if rest.starts_with("<!--") {
            builder.text(&html[text_start..i]);
            i += rest.find("-->").map_or(rest.len(), |end| end + 3);
            text_start = i;
        } else if next == b'!' || next == b'?' {
            builder.text(&html[text_start..i]);
            i += rest.find('>').map_or(rest.len(), |end| end + 1);
            text_start = i;
        } else if next == b'/' && rest.as_bytes().get(2).is_some_and(u8::is_ascii_alphabetic) {
            builder.text(&html[text_start..i]);
            let end = rest.find('>').map_or(rest.len(), |end| end + 1);
            let name: String = rest[2..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect::<String>()
                .to_ascii_lowercase();
            builder.close(&name);
            i += end;
            text_start = i;
        } else if next.is_ascii_alphabetic() {
            builder.text(&html[text_start..i]);
            let (element, self_closing, consumed) = start_tag(rest);
            i += consumed;
            let name = element.name.clone();
            if RAW.contains(&name.as_str()) {
                // Everything up to the matching end tag is text.
                let lower = html[i..].to_ascii_lowercase();
                let close = format!("</{name}");
                let end = lower.find(&close).unwrap_or(lower.len());
                let raw = &html[i..i + end];
                let mut element = element;
                if !raw.is_empty() {
                    let text = if name == "title" || name == "textarea" {
                        decode(raw)
                    } else {
                        raw.to_owned()
                    };
                    element.children.push(Node::Text(text));
                }
                builder.append(element);
                i += end;
                if i < html.len() {
                    i += html[i..].find('>').map_or(html.len() - i, |e| e + 1);
                }
            } else if self_closing || VOID.contains(&name.as_str()) {
                builder.open_implied(&name);
                builder.append(element);
            } else {
                builder.open(element);
            }
            text_start = i;
        } else {
            i += 1;
        }
    }
    builder.text(&html[text_start..]);
    builder.finish()
}

/// Parses a start tag at the start of `input` (`<name attr=..>`): the
/// element, whether it ends with `/>`, and the bytes it takes.
fn start_tag(input: &str) -> (Element, bool, usize) {
    let bytes = input.as_bytes();
    let mut i = 1;
    let name_end = input[i..]
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .map_or(input.len(), |e| e + i);
    let name = input[i..name_end].to_ascii_lowercase();
    i = name_end;
    let mut attrs = Vec::new();
    let mut self_closing = false;
    loop {
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        match bytes[i] {
            b'>' => {
                i += 1;
                break;
            }
            b'/' => {
                self_closing = bytes.get(i + 1) == Some(&b'>');
                i += 1;
                continue;
            }
            _ => {}
        }
        let attr_end = input[i..]
            .find(|c: char| c.is_whitespace() || c == '=' || c == '>' || c == '/')
            .map_or(input.len(), |e| e + i);
        let attr = input[i..attr_end].to_ascii_lowercase();
        if attr_end == i {
            // A stray character: skip it.
            i += input[i..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        i = attr_end;
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if bytes.get(i) == Some(&b'=') {
            i += 1;
            while i < bytes.len() && (bytes[i] as char).is_whitespace() {
                i += 1;
            }
            match bytes.get(i) {
                Some(&quote) if quote == b'"' || quote == b'\'' => {
                    let end = input[i + 1..]
                        .find(quote as char)
                        .map_or(input.len(), |e| e + i + 1);
                    value = decode(&input[i + 1..end]);
                    i = (end + 1).min(input.len());
                }
                _ => {
                    let end = input[i..]
                        .find(|c: char| c.is_whitespace() || c == '>')
                        .map_or(input.len(), |e| e + i);
                    value = decode(&input[i..end]);
                    i = end;
                }
            }
        }
        if !attrs.iter().any(|(n, _): &(String, String)| *n == attr) {
            attrs.push((attr, value));
        }
    }
    (
        Element {
            name,
            attrs,
            children: Vec::new(),
        },
        self_closing,
        i.min(input.len()),
    )
}

struct Builder {
    stack: Vec<Element>,
}

impl Builder {
    fn current(&mut self) -> &mut Element {
        self.stack.last_mut().expect("the document stays open")
    }

    fn text(&mut self, raw: &str) {
        if raw.is_empty() {
            return;
        }
        let text = decode(raw);
        let current = self.current();
        if let Some(Node::Text(last)) = current.children.last_mut() {
            last.push_str(&text);
        } else {
            current.children.push(Node::Text(text));
        }
    }

    fn append(&mut self, element: Element) {
        self.current().children.push(Node::Element(element));
    }

    /// Closes what the start of `name` implies is over.
    fn open_implied(&mut self, name: &str) {
        if CLOSES_P.contains(&name) {
            self.close_within(
                "p",
                &[
                    "div",
                    "td",
                    "th",
                    "li",
                    "body",
                    "blockquote",
                    "section",
                    "article",
                    "header",
                    "footer",
                ],
            );
        }
        match name {
            "li" => self.close_within("li", &["ul", "ol"]),
            "dt" | "dd" => {
                self.close_within("dt", &["dl"]);
                self.close_within("dd", &["dl"]);
            }
            "tr" => {
                self.close_within("td", &["table"]);
                self.close_within("th", &["table"]);
                self.close_within("tr", &["table"]);
            }
            "td" | "th" => {
                self.close_within("td", &["tr", "table"]);
                self.close_within("th", &["tr", "table"]);
            }
            "thead" | "tbody" | "tfoot" => {
                for name in ["td", "th", "tr", "thead", "tbody", "tfoot"] {
                    self.close_within(name, &["table"]);
                }
            }
            _ => {}
        }
    }

    fn open(&mut self, element: Element) {
        self.open_implied(&element.name);
        self.stack.push(element);
    }

    /// Closes the innermost open `name`, unless one of `scope` is open
    /// inside it.
    fn close_within(&mut self, name: &str, scope: &[&str]) {
        for index in (1..self.stack.len()).rev() {
            let open = &self.stack[index].name;
            if open == name {
                self.pop_to(index);
                return;
            }
            if scope.contains(&open.as_str()) {
                return;
            }
        }
    }

    /// An end tag: closes the innermost open element of that name (and
    /// whatever is open inside it); a stray end tag is ignored.
    fn close(&mut self, name: &str) {
        if let Some(index) = (1..self.stack.len())
            .rev()
            .find(|&i| self.stack[i].name == name)
        {
            self.pop_to(index);
        }
    }

    /// Pops elements down to and including `index`.
    fn pop_to(&mut self, index: usize) {
        while self.stack.len() > index {
            let element = self.stack.pop().expect("an open element");
            self.current().children.push(Node::Element(element));
        }
    }

    fn finish(mut self) -> Element {
        self.pop_to(1);
        self.stack.pop().expect("the document")
    }
}

/// Decodes character references (`&amp;`, `&#39;`, `&#x2014;`, the common
/// named ones); anything else stays as written.
pub(crate) fn decode(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let end = rest[1..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '#'))
            .map(|e| e + 1)
            .unwrap_or(rest.len());
        let name = &rest[1..end];
        let with_semicolon = rest[end..].starts_with(';');
        let decoded = if let Some(number) = name.strip_prefix('#') {
            let value = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => number.parse::<u32>().ok(),
            };
            value.map(|v| char::from_u32(v).unwrap_or('\u{fffd}').to_string())
        } else {
            named(name).map(str::to_owned)
        };
        match decoded {
            Some(decoded) if !name.is_empty() => {
                out.push_str(&decoded);
                rest = &rest[end + usize::from(with_semicolon)..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn named(name: &str) -> Option<&'static str> {
    Some(match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => "\u{a0}",
        "shy" => "\u{ad}",
        "copy" => "©",
        "reg" => "®",
        "trade" => "™",
        "mdash" => "—",
        "ndash" => "–",
        "hellip" => "…",
        "laquo" => "«",
        "raquo" => "»",
        "lsquo" => "\u{2018}",
        "rsquo" => "\u{2019}",
        "ldquo" => "\u{201c}",
        "rdquo" => "\u{201d}",
        "bull" => "•",
        "middot" => "·",
        "euro" => "€",
        "pound" => "£",
        "yen" => "¥",
        "cent" => "¢",
        "sect" => "§",
        "para" => "¶",
        "deg" => "°",
        "plusmn" => "±",
        "times" => "×",
        "divide" => "÷",
        "frac12" => "½",
        "frac14" => "¼",
        "frac34" => "¾",
        "rarr" => "→",
        "larr" => "←",
        "check" => "✓",
        "zwj" => "\u{200d}",
        "zwnj" => "\u{200c}",
        "lrm" => "\u{200e}",
        "rlm" => "\u{200f}",
        "ensp" => "\u{2002}",
        "emsp" => "\u{2003}",
        "thinsp" => "\u{2009}",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(node: &Node) -> &Element {
        match node {
            Node::Element(element) => element,
            Node::Text(text) => panic!("text {text:?}"),
        }
    }

    #[test]
    fn elements_attributes_and_text() {
        let doc = parse(
            "<!doctype html><html lang=\"ar\" dir=rtl><body><p class='lead x'>Hi <b>there</b></p></body></html>",
        );
        let html = doc.find("html").unwrap();
        assert_eq!(html.attr("lang"), Some("ar"));
        assert_eq!(html.attr("dir"), Some("rtl"));
        let p = doc.find("p").unwrap();
        assert!(p.has_class("lead") && p.has_class("x") && !p.has_class("le"));
        assert_eq!(p.text(), "Hi there");
        assert_eq!(element(&p.children[1]).name, "b");
    }

    #[test]
    fn references_decode() {
        assert_eq!(
            decode("Tej &amp; Jerry &lt;3 &#39;x&#x27; &mdash; &copy;&nbsp;2026 &bogus; & done"),
            "Tej & Jerry <3 'x' — ©\u{a0}2026 &bogus; & done"
        );
        let doc = parse("<p title=\"a &quot;b&quot;\">x</p>");
        assert_eq!(doc.find("p").unwrap().attr("title"), Some("a \"b\""));
    }

    #[test]
    fn omitted_end_tags_are_implied() {
        let doc =
            parse("<ul><li>One<li>Two</ul><p>First<p>Second<table><tr><td>a<td>b<tr><td>c</table>");
        let ul = doc.find("ul").unwrap();
        assert_eq!(ul.children.len(), 2);
        assert_eq!(element(&ul.children[1]).text(), "Two");
        let table = doc.find("table").unwrap();
        let rows: Vec<&Element> = table.children.iter().map(element).collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].children.len(), 2);
        assert_eq!(element(&rows[1].children[0]).text(), "c");
        // The two paragraphs are siblings, and the table is not inside one.
        let paragraphs = doc
            .children
            .iter()
            .filter(|n| matches!(n, Node::Element(e) if e.name == "p"))
            .count();
        assert_eq!(paragraphs, 2);
    }

    #[test]
    fn void_raw_and_skipped_parts() {
        let doc = parse(
            "<head><title>A &amp; B</title><style>p > b { color: red }</style></head>\
             <!-- a comment with <p> --><p>Line<br>two<img src=\"x.png\" alt=logo /></p><script>if (a < b) {}</script>",
        );
        assert_eq!(doc.find("title").unwrap().text(), "A & B");
        assert_eq!(doc.find("style").unwrap().text(), "p > b { color: red }");
        let p = doc.find("p").unwrap();
        assert_eq!(p.children.len(), 4, "{p:?}");
        assert_eq!(element(&p.children[3]).attr("alt"), Some("logo"));
        assert_eq!(doc.find("script").unwrap().text(), "if (a < b) {}");
    }

    #[test]
    fn stray_end_tags_and_unclosed_elements_are_tolerated() {
        let doc = parse("</div><p>Open <b>bold <i>both</p>after");
        let p = doc.find("p").unwrap();
        assert_eq!(p.text(), "Open bold both");
        assert!(matches!(doc.children.last(), Some(Node::Text(t)) if t == "after"));
        assert_eq!(
            parse("a < b and c").children,
            vec![Node::Text("a < b and c".into())]
        );
    }
}
